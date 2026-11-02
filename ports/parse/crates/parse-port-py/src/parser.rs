//! Python objects for the parser task: `Parser`, `Result`, `Match`,
//! `ResultIterator`, `parse`/`search`/`findall`/`compile`, `extract_format`,
//! plus the internal `re.Match`-like object our regexes produce.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use parse_port_core::convert::{self as core_convert, DateArgs};
use parse_port_core::format::{
    self as core_format, bracket_keys, Conversion, DateKw, ExtraType, ExtraTypes, GroupKey,
};
use parse_port_core::pyre::clamp_pos;
use parse_port_core::{pyre::unicode15 as pystr, strptime as core_strptime, Error};
use pyo3::exceptions::{
    PyAttributeError, PyKeyError, PyOverflowError, PyRecursionError, PyTypeError,
};
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{
    PyBool, PyByteArray, PyBytes, PyDict, PyFloat, PyInt, PyList, PyMemoryView, PySlice, PyString,
    PyTuple, PyType,
};

use crate::convert::temporal_to_py;
use crate::errors::to_pyerr;
use crate::strptime::result_to_py;

mod re_match;

use re_match::ReMatch;

fn type_name(obj: &Bound<'_, PyAny>) -> PyResult<String> {
    Ok(obj.get_type().name()?.to_string())
}

/// The code points of a Python `str`. Lone surrogates (not Rust `char`s)
/// become their `pystr::surrogate_standin`; `py_text` maps them back.
pub fn str_chars(s: &Bound<'_, PyString>) -> PyResult<Vec<char>> {
    if let Ok(t) = s.to_str() {
        return Ok(t.chars().collect());
    }
    let raw = s.call_method1("encode", ("utf-32-le", "surrogatepass"))?;
    let bytes = raw.cast::<PyBytes>()?.as_bytes();
    Ok(bytes
        .as_chunks::<4>()
        .0
        .iter()
        .filter_map(|&b| {
            let cp = u32::from_le_bytes(b);
            char::from_u32(cp).or_else(|| pystr::surrogate_standin(cp))
        })
        .collect())
}

/// [`str_chars`] as a `String`.
pub fn str_text(s: &Bound<'_, PyString>) -> PyResult<String> {
    if let Ok(t) = s.to_str() {
        return Ok(t.to_owned());
    }
    Ok(str_chars(s)?.into_iter().collect())
}

/// A Python `str` for core text, turning surrogate stand-ins back into
/// lone surrogates.
pub fn py_text<'py>(py: Python<'py>, s: &str) -> PyResult<Bound<'py, PyString>> {
    if !s.chars().any(|c| pystr::standin_surrogate(c).is_some()) {
        return Ok(PyString::new(py, s));
    }
    let bytes: Vec<u8> = s
        .chars()
        .flat_map(|c| {
            pystr::standin_surrogate(c)
                .unwrap_or(c as u32)
                .to_le_bytes()
        })
        .collect();
    Ok(PyBytes::new(py, &bytes)
        .call_method1("decode", ("utf-32-le", "surrogatepass"))?
        .cast_into::<PyString>()?)
}

/// Convert a core error, as `errors::to_pyerr` does, plus what that frozen
/// mapping can't express: `RecursionError`, and messages holding surrogates.
fn core_err(py: Python<'_>, e: Error) -> PyErr {
    if parse_port_core::pyre::is_recursion(&e) {
        return PyRecursionError::new_err(e.to_string());
    }
    let msg = e.to_string();
    let regex = match &e {
        Error::Regex(r) => Some((r.msg.clone(), r.pattern.clone())),
        _ => None,
    };
    let err = to_pyerr(py, e);
    if msg.chars().any(|c| pystr::standin_surrogate(c).is_some()) {
        let v = err.value(py);
        let fixed = (|| -> PyResult<()> {
            v.setattr("args", (py_text(py, &msg)?,))?;
            if let Some((m, p)) = regex {
                v.setattr("msg", py_text(py, &m)?)?;
                v.setattr("pattern", py_text(py, &p)?)?;
            }
            Ok(())
        })();
        if let Err(e) = fixed {
            return e;
        }
    }
    err
}

/// `Result<T, core::Error>` → `PyResult<T>` through [`core_err`].
trait CoreResult<T> {
    fn pyr(self, py: Python<'_>) -> PyResult<T>;
}

impl<T> CoreResult<T> for Result<T, Error> {
    fn pyr(self, py: Python<'_>) -> PyResult<T> {
        self.map_err(|e| core_err(py, e))
    }
}

/// Upstream Python frames between a caller and `re.compile` for the
/// `Parser.parse`/`Parser.search`/`ResultIterator.__next__` methods (the
/// method and the `_match_re`/`_search_re` property)...
const METHOD_FRAMES: usize = 2;
/// ...and for the module functions `parse()`/`search()`, one more.
const FUNCTION_FRAMES: usize = 3;
/// Frames from `re.compile` to `_parse_sub(nested=0)` plus the deepest
/// helper calls below the innermost `_parse` (calibrated on CPython 3.12).
const SRE_FRAMES: usize = 8;

/// CPython's default `sys.getrecursionlimit()` (`sys` may not be imported
/// here, so a changed limit isn't seen).
const RECURSION_LIMIT: usize = 1000;

/// The deepest sre parser `nested` level upstream can reach before
/// `RecursionError`: the recursion limit minus the Python frames already on
/// the stack, the `frames` upstream adds, and [`SRE_FRAMES`].
fn nesting_limit(py: Python<'_>, frames: usize) -> usize {
    let depth = python_depth(py).unwrap_or(0);
    RECURSION_LIMIT.saturating_sub(depth + frames + SRE_FRAMES)
}

/// How many Python frames are on the stack below this call. Read from the
/// traceback of a `ValueError` raised inside the pure-Python
/// `fractions.Fraction.__new__`: that frame's `f_back` chain is our callers.
fn python_depth(py: Python<'_>) -> PyResult<usize> {
    let fraction = py.import("fractions")?.getattr("Fraction")?;
    let Err(err) = fraction.call1(("",)) else {
        return Ok(0);
    };
    let Some(tb) = err.traceback(py) else {
        return Ok(0);
    };
    let mut frame = tb.getattr("tb_frame")?.getattr("f_back")?;
    let mut depth = 0usize;
    while !frame.is_none() {
        depth += 1;
        frame = frame.getattr("f_back")?;
    }
    Ok(depth)
}

/// The text `sre` would match against, with its argument errors.
fn text_of(obj: &Bound<'_, PyAny>) -> PyResult<Arc<[char]>> {
    let s = str_of(obj)?;
    if let Ok(t) = s.to_str() {
        if t.is_ascii() {
            // An exact-size iterator: one allocation, no intermediate `Vec`.
            return Ok(t.bytes().map(char::from).collect());
        }
    }
    Ok(str_chars(s)?.into())
}

/// `obj` as a `str`, or the error `sre` raises for it.
fn str_of<'a, 'py>(obj: &'a Bound<'py, PyAny>) -> PyResult<&'a Bound<'py, PyString>> {
    if let Ok(s) = obj.cast::<PyString>() {
        return Ok(s);
    }
    if obj.is_instance_of::<PyBytes>()
        || obj.is_instance_of::<PyByteArray>()
        || obj.is_instance_of::<PyMemoryView>()
    {
        return Err(PyTypeError::new_err(
            "cannot use a string pattern on a bytes-like object",
        ));
    }
    Err(PyTypeError::new_err(format!(
        "expected string or bytes-like object, got '{}'",
        type_name(obj)?
    )))
}

/// A `Py_ssize_t` argument (`pos`/`endpos`).
fn ssize(obj: &Bound<'_, PyAny>) -> PyResult<isize> {
    obj.extract::<isize>().map_err(|e| {
        if e.is_instance_of::<PyOverflowError>(obj.py()) {
            PyOverflowError::new_err("Python int too large to convert to C ssize_t")
        } else {
            e
        }
    })
}

/// `len(obj)`
fn py_len(obj: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    let n = obj.len()?;
    Ok(n.into_pyobject(obj.py())?.into_any().unbind())
}

// ---------------------------------------------------------------- extra types

/// Error while building a parser: a core error or a Python exception.
enum BuildError {
    Core(Error),
    Py(PyErr),
}

impl From<Error> for BuildError {
    fn from(e: Error) -> Self {
        BuildError::Core(e)
    }
}

impl From<PyErr> for BuildError {
    fn from(e: PyErr) -> Self {
        BuildError::Py(e)
    }
}

impl BuildError {
    fn into_py(self, py: Python<'_>) -> PyErr {
        match self {
            BuildError::Core(e) => core_err(py, e),
            BuildError::Py(e) => e,
        }
    }
}

/// `extra_types` seen through Python's `in` and `[]`.
struct PyExtra<'a, 'py> {
    obj: &'a Bound<'py, PyAny>,
    converters: HashMap<String, Py<PyAny>>,
}

fn attr_or_none<'py>(obj: &Bound<'py, PyAny>, name: &str) -> PyResult<Option<Bound<'py, PyAny>>> {
    match obj.getattr(name) {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.is_instance_of::<PyAttributeError>(obj.py()) => Ok(None),
        Err(e) => Err(e),
    }
}

impl ExtraTypes for PyExtra<'_, '_> {
    type Error = BuildError;

    fn contains(&mut self, name: &str) -> Result<bool, BuildError> {
        Ok(self.obj.contains(py_text(self.obj.py(), name)?)?)
    }

    fn get(&mut self, name: &str) -> Result<ExtraType, BuildError> {
        let conv = self.obj.get_item(py_text(self.obj.py(), name)?)?;
        let pattern = match attr_or_none(&conv, "pattern")? {
            Some(p) => str_text(&p.str()?)?,
            None => ".+?".to_owned(),
        };
        let group_count = match attr_or_none(&conv, "regex_group_count")? {
            None => 0,
            Some(c) if c.is_none() => 0,
            Some(c) if c.is_instance_of::<PyInt>() => c.extract::<i64>()?,
            Some(c) => {
                return Err(PyTypeError::new_err(format!(
                    "unsupported operand type(s) for +=: 'int' and '{}'",
                    type_name(&c)?
                ))
                .into())
            }
        };
        self.converters.insert(name.to_owned(), conv.unbind());
        Ok(ExtraType {
            pattern,
            group_count,
        })
    }
}

// ---------------------------------------------------------------- Parser

/// `parse.Parser`.
#[pyclass(module = "parse", name = "Parser", subclass, frozen, dict)]
pub struct Parser {
    core: Arc<core_format::Parser>,
    format: Py<PyAny>,
    converters: HashMap<String, Py<PyAny>>,
    /// The `extra_types` mapping as given (upstream keeps it for pickling).
    extra_types: Py<PyAny>,
    case_sensitive: bool,
}

/// `__reduce__` matching what `object.__reduce_ex__` gives upstream's plain
/// classes: rebuild via `cls.__new__(cls, *args)` (no `__init__`, like
/// `copyreg.__newobj__`; a builtin method pickles as `getattr(cls, '__new__')`),
/// then restore any instance `__dict__`.
fn newobj_reduce<'py>(
    slf: &Bound<'py, PyAny>,
    args: Vec<Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyTuple>> {
    let py = slf.py();
    let cls = slf.get_type().into_any();
    let newobj = cls.getattr("__new__")?;
    let mut all = vec![cls];
    all.extend(args);
    let state = match slf.getattr("__dict__") {
        Ok(d) if d.is_truthy()? => d,
        _ => py.None().into_bound(py),
    };
    PyTuple::new(py, [newobj, PyTuple::new(py, all)?.into_any(), state])
}

fn decimal_type(py: Python<'_>) -> PyResult<&Bound<'_, PyAny>> {
    static DECIMAL: PyOnceLock<Py<PyAny>> = PyOnceLock::new();
    DECIMAL.import(py, "decimal", "Decimal")
}

fn not_subscriptable(v: &Bound<'_, PyAny>) -> PyResult<PyErr> {
    Ok(PyTypeError::new_err(format!(
        "'{}' object is not subscriptable",
        type_name(v)?
    )))
}

fn date_args(kw: &DateKw) -> DateArgs {
    let ok = |v: Option<i64>| v.map(Ok);
    DateArgs {
        mm_dd: match (kw.mm, kw.dd) {
            (Some(mm), Some(dd)) if mm != 0 && dd != 0 => Some((Ok(mm), Ok(dd))),
            _ => None,
        },
        ymd: ok(kw.ymd),
        mdy: ok(kw.mdy),
        dmy: ok(kw.dmy),
        d_m_y: kw.d_m_y.map(|v| Ok(v.iter().map(|&i| Ok(i)).collect())),
        hms: ok(kw.hms),
        am: ok(kw.am),
        tz: ok(kw.tz),
    }
}

/// The parser behind a call: a Python object, or (in the module-level
/// functions) a plain value that becomes one only when a `parse.Match`
/// must hold it. Nothing else can observe the parser object.
enum Owner<'a, 'py> {
    Obj(&'a Bound<'py, Parser>),
    Value(Parser),
}

impl<'py> Owner<'_, 'py> {
    fn get(&self) -> &Parser {
        match self {
            Owner::Obj(p) => p.get(),
            Owner::Value(p) => p,
        }
    }

    fn into_obj(self, py: Python<'py>) -> PyResult<Bound<'py, Parser>> {
        match self {
            Owner::Obj(p) => Ok(p.clone()),
            Owner::Value(p) => Bound::new(py, p),
        }
    }
}

/// What `evaluate` needs from a parser beyond the core accessors, derived
/// once per core parser (and so shared by cached ones).
struct EvalPlan {
    /// Per `named_fields()` entry: its index in `keys`, and whether it has a
    /// conversion.
    named: Vec<(usize, bool)>,
    /// The `named` dict keys (original field names), in first-seen order.
    keys: Vec<Py<PyString>>,
    /// Whether any field name has a `[` (needs `_expand_named_fields`).
    needs_expand: bool,
    /// Per `fixed_fields()` entry: whether it has a conversion.
    fixed_conv: Vec<bool>,
}

/// A (possibly foreign) match object.
enum Src<'a, 'py> {
    Own(&'a ReMatch),
    Foreign(&'a Bound<'py, PyAny>),
}

impl Parser {
    fn build(
        py: Python<'_>,
        format: &Bound<'_, PyAny>,
        extra_types: Option<&Bound<'_, PyAny>>,
        case_sensitive: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let extra_obj = match extra_types {
            Some(e) if !e.is_none() => e.clone(),
            _ => PyDict::new(py).into_any(),
        };
        let case_sensitive = match case_sensitive {
            Some(c) => c.is_truthy()?,
            None => false,
        };
        let format_str = str_of(format)?;
        let text: Cow<'_, str> = match format_str.to_str() {
            Ok(t) => Cow::Borrowed(t),
            Err(_) => Cow::Owned(str_text(format_str)?),
        };
        let mut extra = PyExtra {
            obj: &extra_obj,
            converters: HashMap::new(),
        };
        // With no `extra_types`, upstream consults only a fresh empty dict,
        // so the build is pure and can be shared.
        let core = if extra_types.is_none_or(|e| e.is_none()) {
            core_format::Parser::new_cached(&text, case_sensitive).pyr(py)?
        } else {
            Arc::new(
                core_format::Parser::new(&text, &mut extra, case_sensitive)
                    .map_err(|e| e.into_py(py))?,
            )
        };
        Ok(Parser {
            core,
            format: format.clone().unbind(),
            converters: extra.converters,
            extra_types: extra_obj.clone().unbind(),
            case_sensitive,
        })
    }

    fn convert<'py>(
        &self,
        py: Python<'py>,
        conv: &Conversion,
        value: Bound<'py, PyAny>,
        src: &Src<'_, 'py>,
    ) -> PyResult<Bound<'py, PyAny>> {
        match conv {
            Conversion::Int(base) => {
                let Ok(s) = value.cast::<PyString>() else {
                    return Err(not_subscriptable(&value)?);
                };
                int_to_py(py, &str_text(s)?, *base)
            }
            Conversion::Percentage => {
                let Ok(s) = value.cast::<PyString>() else {
                    return Err(not_subscriptable(&value)?);
                };
                let v = core_convert::percentage(&str_text(s)?).pyr(py)?;
                Ok(PyFloat::new(py, v).into_any())
            }
            Conversion::Float => py.get_type::<PyFloat>().call1((value,)),
            Conversion::Decimal => decimal_type(py)?.call1((value,)),
            Conversion::Date(kw) => {
                // Our own `re.Match` is frozen and final, so its `groups()`
                // is exactly its captured texts.
                // `date_convert` reads only the groups `kw` names, so the
                // others stay `None` (unread) instead of being copied.
                let groups: Vec<Option<Cow<'_, str>>> = match src {
                    Src::Own(r) => {
                        let len = r.regex.groups();
                        let string = r.string.bind(py);
                        // ASCII text: code-point spans are byte spans of the
                        // `str` the match ran on, so groups can borrow it.
                        let ascii = string
                            .cast::<PyString>()
                            .ok()
                            .and_then(|s| s.to_str().ok())
                            .filter(|t| t.is_ascii() && t.len() == r.text.len());
                        (1..=len)
                            .map(|i| {
                                if !kw.reads_group(i - 1, len) {
                                    return None;
                                }
                                match (ascii, r.caps.span(i)) {
                                    (Some(t), Some((s, e))) => t.get(s..e).map(Cow::Borrowed),
                                    _ => r.caps.text(&r.text, i).map(Cow::Owned),
                                }
                            })
                            .collect()
                    }
                    Src::Foreign(m) => {
                        let groups: Vec<Option<String>> = m.call_method0("groups")?.extract()?;
                        groups.into_iter().map(|g| g.map(Cow::Owned)).collect()
                    }
                };
                let v = core_convert::date_convert(&groups, &date_args(kw)).pyr(py)?;
                Ok(temporal_to_py(py, &v)?.into_bound(py))
            }
            Conversion::Strf(ty) => {
                let x = if let Ok(s) = value.cast::<PyString>() {
                    str_text(s)?
                } else if ty.contains("%y") || ty.contains("%Y") {
                    let name = if value.is_none() {
                        "None".to_owned()
                    } else {
                        type_name(&value)?
                    };
                    return Err(PyTypeError::new_err(format!(
                        "strptime() argument 1 must be str, not {name}"
                    )));
                } else {
                    str_text(&value.str()?)?
                };
                let r = core_strptime::strf_date_convert(&x, ty).pyr(py)?;
                Ok(result_to_py(py, &r)?.into_bound(py))
            }
            Conversion::Custom(name) => match self.converters.get(name) {
                Some(c) => c.bind(py).call1((value,)),
                None => Err(PyTypeError::new_err("converter missing")),
            },
        }
    }

    /// Upstream's fixed-field step: `fixed_fields = list(m.groups())`,
    /// convert the fixed indexes in place, then take them as a tuple.
    fn fixed_list<'py>(
        &self,
        py: Python<'py>,
        src: &Src<'_, 'py>,
        fixed_idx: &[i64],
    ) -> PyResult<Bound<'py, PyTuple>> {
        let fixed_list = match src {
            Src::Own(r) => r.group_list(py)?,
            Src::Foreign(m) => py
                .get_type::<PyList>()
                .call1((m.call_method0("groups")?,))?
                .cast_into::<PyList>()?,
        };
        let fixed_list = fixed_list.into_any();
        for &n in fixed_idx {
            if let Some(conv) = self.core.conversion(&GroupKey::Index(n)) {
                let v = fixed_list.get_item(n)?;
                let v = self.convert(py, conv, v, src)?;
                fixed_list.set_item(n, v)?;
            }
        }
        let mut fixed = Vec::with_capacity(fixed_idx.len());
        for &n in fixed_idx {
            fixed.push(fixed_list.get_item(n)?);
        }
        PyTuple::new(py, fixed)
    }

    /// [`Parser::fixed_list`] for our own match when every index is a
    /// distinct in-range group: each list slot is read (and converted) at
    /// most once, so only those groups need to become Python strings.
    fn fixed_own<'py>(
        &self,
        py: Python<'py>,
        r: &ReMatch,
        fixed_idx: &[i64],
        plan: &EvalPlan,
    ) -> PyResult<Bound<'py, PyTuple>> {
        let mut fixed = Vec::with_capacity(fixed_idx.len());
        let ascii = r.ascii_text(py);
        for (&n, &has_conv) in fixed_idx.iter().zip(&plan.fixed_conv) {
            let g = usize::try_from(n).map_or(usize::MAX, |n| n.saturating_add(1));
            let conv = if has_conv {
                self.core.conversion(&GroupKey::Index(n))
            } else {
                None
            };
            fixed.push(self.convert_group(py, conv, r, g, ascii)?);
        }
        PyTuple::new(py, fixed)
    }

    /// Group `g` of our own match, converted by `conv`. `int`/`percentage`
    /// only read the text back from the `str`, so for ASCII text (`ascii`,
    /// see [`ReMatch::ascii_text`]) they convert the matched slice directly.
    fn convert_group<'py>(
        &self,
        py: Python<'py>,
        conv: Option<&Conversion>,
        r: &ReMatch,
        g: usize,
        ascii: Option<&str>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let text = || ascii?.get(r.caps.span(g).map(|(s, e)| s..e)?);
        match conv {
            Some(Conversion::Int(base)) => {
                if let Some(t) = text() {
                    return int_to_py(py, t, *base);
                }
            }
            Some(Conversion::Percentage) => {
                if let Some(t) = text() {
                    let v = core_convert::percentage(t).pyr(py)?;
                    return Ok(PyFloat::new(py, v).into_any());
                }
            }
            Some(Conversion::Float) => {
                if let Some(v) = text().and_then(plain_float) {
                    return Ok(PyFloat::new(py, v).into_any());
                }
            }
            _ => {}
        }
        // ASCII text: the matched slice is the group's `str` exactly.
        let v = match (conv, text()) {
            (None, Some(t)) => return Ok(PyString::new(py, t).into_any()),
            _ => r.group_text(py, g)?.into_bound(py),
        };
        match conv {
            Some(conv) => self.convert(py, conv, v, &Src::Own(r)),
            None => Ok(v),
        }
    }

    fn evaluate<'py>(&self, py: Python<'py>, m: &Bound<'py, PyAny>) -> PyResult<ParseResult> {
        match m.cast::<ReMatch>() {
            Ok(r) => self.evaluate_src(py, &Src::Own(r.get())),
            Err(_) => self.evaluate_src(py, &Src::Foreign(m)),
        }
    }

    /// The parser's [`EvalPlan`], built on first use.
    fn plan(&self, py: Python<'_>) -> PyResult<&EvalPlan> {
        if let Some(plan) = self.core.binding_data::<EvalPlan>() {
            return Ok(plan);
        }
        let mut names: Vec<&str> = Vec::new();
        let mut keys = Vec::new();
        let mut named = Vec::with_capacity(self.core.named_fields().len());
        let mut needs_expand = false;
        for k in self.core.named_fields() {
            let korig = self.core.group_name(k).unwrap_or(k);
            needs_expand |= korig.contains('[');
            let slot = match names.iter().position(|o| *o == korig) {
                Some(i) => i,
                None => {
                    names.push(korig);
                    keys.push(py_text(py, korig)?.unbind());
                    names.len() - 1
                }
            };
            named.push((slot, self.core.named_conversion(k).is_some()));
        }
        let fixed_conv = self
            .core
            .fixed_fields()
            .iter()
            .map(|&n| self.core.conversion(&GroupKey::Index(n)).is_some())
            .collect();
        let plan = EvalPlan {
            named,
            keys,
            needs_expand,
            fixed_conv,
        };
        self.core
            .set_binding_data(plan)
            .ok_or_else(|| PyTypeError::new_err("parser binding data has another type"))
    }

    /// Upstream `evaluate_result` on a match (ours or a foreign `re.Match`).
    fn evaluate_src<'py>(&self, py: Python<'py>, src: &Src<'_, 'py>) -> PyResult<ParseResult> {
        let plan = self.plan(py)?;
        let fixed_idx = self.core.fixed_fields();
        let fixed = match src {
            Src::Own(r) if distinct_in_range(fixed_idx, r.regex.groups()) => {
                self.fixed_own(py, r, fixed_idx, plan)?
            }
            _ => self.fixed_list(py, src, fixed_idx)?,
        };

        let groupdict = match src {
            Src::Own(_) => None,
            Src::Foreign(m) => Some(m.call_method0("groupdict")?),
        };
        let named = PyDict::new(py);
        let ascii = match src {
            Src::Own(r) => r.ascii_text(py),
            Src::Foreign(_) => None,
        };
        // Per dict key: the last field (group name, our group index) stored there.
        let mut last: Vec<(&str, usize)> = vec![("", 0); plan.keys.len()];
        for (k, &(slot, has_conv)) in self.core.named_fields().iter().zip(&plan.named) {
            let mut group = 0;
            let conv = if has_conv {
                self.core.named_conversion(k)
            } else {
                None
            };
            let raw = match (src, &groupdict) {
                (Src::Own(r), _) => match r.regex.groupindex().get(k.as_str()) {
                    Some(&i) => {
                        group = i;
                        Ok(self.convert_group(py, conv, r, i, ascii)?)
                    }
                    None => {
                        return Err(pyo3::exceptions::PyKeyError::new_err(k.clone()));
                    }
                },
                (_, Some(gd)) => Err(gd.get_item(k.as_str())?),
                _ => Err(py.None().into_bound(py)),
            };
            // `Ok`: already converted; `Err`: the raw foreign value.
            let value = match (raw, conv) {
                (Ok(v), _) => v,
                (Err(raw), Some(conv)) => self.convert(py, conv, raw, src)?,
                (Err(raw), None) => raw,
            };
            let key = plan
                .keys
                .get(slot)
                .ok_or_else(|| PyKeyError::new_err(k.clone()))?;
            named.set_item(key.bind(py), value)?;
            if let Some(l) = last.get_mut(slot) {
                *l = (k.as_str(), group);
            }
        }

        let spans = PyDict::new(py);
        for (key, &(k, group)) in plan.keys.iter().zip(&last) {
            let span = match src {
                Src::Own(r) => r.span_of(group).into_pyobject(py)?.into_any(),
                Src::Foreign(m) => m.call_method1("span", (k,))?,
            };
            spans.set_item(key.bind(py), span)?;
        }
        for (i, &n) in fixed_idx.iter().enumerate() {
            let g = n.saturating_add(1);
            let span = match src {
                Src::Own(r) => r.span_index(g)?.into_pyobject(py)?.into_any(),
                Src::Foreign(m) => m.call_method1("span", (g,))?,
            };
            spans.set_item(i, span)?;
        }
        // Without `[` in any name, upstream's expansion rebuilds `named`
        // key by key in the same order; `named` is private, so reuse it.
        let named = if plan.needs_expand {
            expand_named_fields(py, &named)?
        } else {
            named.into_any()
        };
        Ok(ParseResult {
            fixed: fixed.into_any().unbind(),
            named: named.unbind(),
            spans: spans.into_any().unbind(),
        })
    }

    /// A `Result` (evaluated) or a `parse.Match` for our match `m`. When
    /// evaluating, neither the parser nor the `re.Match` object is visible
    /// to Python, so they become objects only for a `parse.Match`.
    fn wrap_match(
        py: Python<'_>,
        owner: Owner<'_, '_>,
        m: ReMatch,
        evaluate_result: bool,
    ) -> PyResult<Py<PyAny>> {
        if evaluate_result {
            let r = owner.get().evaluate_src(py, &Src::Own(&m))?;
            Ok(Bound::new(py, r)?.into_any().unbind())
        } else {
            let mm = ParseMatch {
                parser: owner.into_obj(py)?.into_any().unbind(),
                r#match: Bound::new(py, m)?.into_any().unbind(),
            };
            Ok(Bound::new(py, mm)?.into_any().unbind())
        }
    }

    /// `Parser.parse`, reached through `frames` upstream Python frames.
    fn parse_from(
        py: Python<'_>,
        owner: Owner<'_, '_>,
        string: &Bound<'_, PyAny>,
        evaluate_result: Option<&Bound<'_, PyAny>>,
        frames: usize,
    ) -> PyResult<Py<PyAny>> {
        let regex = owner
            .get()
            .core
            .match_regex_nested(|| nesting_limit(py, frames))
            .pyr(py)?;
        let text: Arc<[char]> = text_of(string)?;
        let (pos, endpos) = (0, text.len());
        let Some(caps) = regex.match_at(&text, pos, endpos) else {
            return Ok(py.None());
        };
        let m = ReMatch {
            string: string.clone().unbind(),
            text,
            caps,
            regex,
            pos,
            endpos,
        };
        Parser::wrap_match(py, owner, m, truthy(evaluate_result)?)
    }

    /// `Parser.search`, reached through `frames` upstream Python frames.
    fn search_from(
        py: Python<'_>,
        owner: Owner<'_, '_>,
        string: &Bound<'_, PyAny>,
        pos: Option<&Bound<'_, PyAny>>,
        endpos: Option<&Bound<'_, PyAny>>,
        evaluate_result: Option<&Bound<'_, PyAny>>,
        frames: usize,
    ) -> PyResult<Py<PyAny>> {
        let endpos = match endpos {
            Some(e) if !e.is_none() => e.clone(),
            _ => py_len(string)?.into_bound(py),
        };
        let pos = match pos {
            Some(p) => p.clone(),
            None => PyInt::new(py, 0).into_any(),
        };
        let text = || text_of(string);
        match owner
            .get()
            .run_search(py, string, &pos, &endpos, text, frames)?
        {
            None => Ok(py.None()),
            Some(m) => Parser::wrap_match(py, owner, m, truthy(evaluate_result)?),
        }
    }

    fn run_search<'py>(
        &self,
        py: Python<'py>,
        string: &Bound<'py, PyAny>,
        pos: &Bound<'py, PyAny>,
        endpos: &Bound<'py, PyAny>,
        text: impl FnOnce() -> PyResult<Arc<[char]>>,
        frames: usize,
    ) -> PyResult<Option<ReMatch>> {
        let regex = self
            .core
            .search_regex_nested(|| nesting_limit(py, frames))
            .pyr(py)?;
        let pos = ssize(pos)?;
        let endpos = ssize(endpos)?;
        let text = text()?;
        let (pos, endpos) = clamp_pos(pos, endpos, text.len());
        let Some(caps) = regex.search(&text, pos, endpos) else {
            return Ok(None);
        };
        Ok(Some(ReMatch {
            string: string.clone().unbind(),
            text,
            caps,
            regex,
            pos,
            endpos,
        }))
    }
}

/// Upstream `_expand_named_fields`, on Python objects.
fn expand_named_fields<'py>(
    py: Python<'py>,
    named: &Bound<'py, PyDict>,
) -> PyResult<Bound<'py, PyAny>> {
    let result = PyDict::new(py).into_any();
    for (field, value) in named.iter() {
        let field = str_text(field.cast::<PyString>()?)?;
        let (basename, subkeys) = match field.find('[') {
            Some(n) => field.split_at(n),
            None => (field.as_str(), ""),
        };
        let mut d = result.clone();
        let mut k = basename.to_owned();
        for sub in bracket_keys(subkeys) {
            d = d.call_method1("setdefault", (py_text(py, &k)?, PyDict::new(py)))?;
            k = sub;
        }
        d.set_item(py_text(py, &k)?, value)?;
    }
    Ok(result)
}

#[pymethods]
impl Parser {
    #[new]
    #[pyo3(signature = (format, extra_types = None, case_sensitive = None))]
    fn new(
        py: Python<'_>,
        format: &Bound<'_, PyAny>,
        extra_types: Option<&Bound<'_, PyAny>>,
        case_sensitive: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        Parser::build(py, format, extra_types, case_sensitive)
    }

    fn __reduce__<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyTuple>> {
        let py = slf.py();
        let p = slf.get();
        newobj_reduce(
            slf.as_any(),
            vec![
                p.format.bind(py).clone(),
                p.extra_types.bind(py).clone(),
                PyBool::new(py, p.case_sensitive).to_owned().into_any(),
            ],
        )
    }

    fn __repr__(slf: &Bound<'_, Self>) -> PyResult<String> {
        // Upstream works on the original object; doing the same keeps lone
        // surrogates (and str subclasses) exact.
        let py = slf.py();
        let name = slf.get_type().name()?.to_string();
        let f = slf.get().format.bind(py);
        let shown = if f.len()? > 20 {
            f.get_item(PySlice::new(py, 0, 17, 1))?.add("...")?
        } else {
            f.clone()
        };
        Ok(format!("<{name} {}>", shown.repr()?))
    }

    #[getter]
    fn named_fields<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        let names = self
            .core
            .named_fields()
            .iter()
            .map(|n| py_text(py, n))
            .collect::<PyResult<Vec<_>>>()?;
        PyList::new(py, names)
    }

    #[getter]
    fn fixed_fields(&self) -> Vec<i64> {
        self.core.fixed_fields().to_vec()
    }

    #[getter]
    fn format(&self, py: Python<'_>) -> Py<PyAny> {
        self.format.clone_ref(py)
    }

    #[getter]
    fn _expression<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyString>> {
        py_text(py, self.core.expression())
    }

    #[pyo3(signature = (string, evaluate_result = None))]
    fn parse(
        slf: &Bound<'_, Self>,
        string: &Bound<'_, PyAny>,
        evaluate_result: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        Parser::parse_from(
            slf.py(),
            Owner::Obj(slf),
            string,
            evaluate_result,
            METHOD_FRAMES,
        )
    }

    #[pyo3(signature = (string, pos = None, endpos = None, evaluate_result = None))]
    fn search(
        slf: &Bound<'_, Self>,
        string: &Bound<'_, PyAny>,
        pos: Option<&Bound<'_, PyAny>>,
        endpos: Option<&Bound<'_, PyAny>>,
        evaluate_result: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        Parser::search_from(
            slf.py(),
            Owner::Obj(slf),
            string,
            pos,
            endpos,
            evaluate_result,
            METHOD_FRAMES,
        )
    }

    #[pyo3(signature = (string, pos = None, endpos = None, extra_types = None, evaluate_result = None))]
    fn findall(
        slf: &Bound<'_, Self>,
        string: &Bound<'_, PyAny>,
        pos: Option<&Bound<'_, PyAny>>,
        endpos: Option<&Bound<'_, PyAny>>,
        extra_types: Option<&Bound<'_, PyAny>>,
        evaluate_result: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<ResultIterator> {
        let _ = extra_types;
        let py = slf.py();
        let endpos = match endpos {
            Some(e) if !e.is_none() => e.clone().unbind(),
            _ => py_len(string)?,
        };
        Ok(ResultIterator {
            parser: slf.clone().into_any().unbind(),
            string: string.clone().unbind(),
            pos: match pos {
                Some(p) => p.clone().unbind(),
                None => PyInt::new(py, 0).into_any().unbind(),
            },
            endpos,
            evaluate_result: match evaluate_result {
                Some(e) => e.clone().unbind(),
                None => PyBool::new(py, true).to_owned().into_any().unbind(),
            },
            text: None,
        })
    }

    fn evaluate_result(&self, py: Python<'_>, m: &Bound<'_, PyAny>) -> PyResult<ParseResult> {
        self.evaluate(py, m)
    }
}

/// Whether `idx` is strictly increasing within `0..len` (so no two fixed
/// fields share, or negatively alias, a slot of `list(m.groups())`).
fn distinct_in_range(idx: &[i64], len: usize) -> bool {
    let mut prev: Option<usize> = None;
    idx.iter().all(|&n| match usize::try_from(n) {
        Ok(n) if n < len && prev.is_none_or(|p| p < n) => {
            prev = Some(n);
            true
        }
        _ => false,
    })
}

fn truthy(v: Option<&Bound<'_, PyAny>>) -> PyResult<bool> {
    match v {
        Some(v) => v.is_truthy(),
        None => Ok(true),
    }
}

// ---------------------------------------------------------------- Result

/// `int_convert(base)(text)` as a Python int.
/// `float(t)` for text made only of digits, `.`, `e`/`E` and signs. Over
/// those characters Python's float syntax (no `_`, no spaces, no `inf`/`nan`)
/// and Rust's agree, and both round correctly. `None`: not such text, or not
/// a float (the caller's `float()` call then raises).
fn plain_float(t: &str) -> Option<f64> {
    let plain = !t.is_empty()
        && t.bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-'));
    if plain {
        t.parse().ok()
    } else {
        None
    }
}

fn int_to_py<'py>(py: Python<'py>, text: &str, base: Option<i64>) -> PyResult<Bound<'py, PyAny>> {
    let v = core_convert::int_convert(text, base).pyr(py)?;
    // Same value; small ints skip the big-integer byte round trip.
    match i64::try_from(&v) {
        Ok(small) => Ok(small.into_pyobject(py)?.into_any()),
        Err(_) => Ok(v.into_pyobject(py)?.into_any()),
    }
}

/// `parse.Result`.
#[pyclass(module = "parse", name = "Result", subclass, dict)]
pub struct ParseResult {
    #[pyo3(get, set)]
    fixed: Py<PyAny>,
    #[pyo3(get, set)]
    named: Py<PyAny>,
    #[pyo3(get, set)]
    spans: Py<PyAny>,
}

#[pymethods]
impl ParseResult {
    #[new]
    fn new(fixed: Py<PyAny>, named: Py<PyAny>, spans: Py<PyAny>) -> Self {
        ParseResult {
            fixed,
            named,
            spans,
        }
    }

    fn __reduce__<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyTuple>> {
        let py = slf.py();
        let args = {
            let r = slf.borrow();
            vec![
                r.fixed.bind(py).clone(),
                r.named.bind(py).clone(),
                r.spans.bind(py).clone(),
            ]
        };
        newobj_reduce(slf.as_any(), args)
    }

    fn __getitem__<'py>(
        &self,
        py: Python<'py>,
        item: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        if item.is_instance_of::<PyInt>() || item.is_instance_of::<PySlice>() {
            self.fixed.bind(py).get_item(item)
        } else {
            self.named.bind(py).get_item(item)
        }
    }

    fn __repr__(slf: &Bound<'_, Self>) -> PyResult<String> {
        let py = slf.py();
        let name = slf.get_type().name()?.to_string();
        let (fixed, named) = {
            let r = slf.borrow();
            (r.fixed.clone_ref(py), r.named.clone_ref(py))
        };
        let fixed = fixed.bind(py).repr()?.to_string();
        let named = named.bind(py).repr()?.to_string();
        Ok(format!("<{name} {fixed} {named}>"))
    }

    fn __contains__(&self, py: Python<'_>, name: &Bound<'_, PyAny>) -> PyResult<bool> {
        self.named.bind(py).contains(name)
    }
}

// ---------------------------------------------------------------- Match

/// `parse.Match`.
#[pyclass(module = "parse", name = "Match", subclass, dict)]
pub struct ParseMatch {
    #[pyo3(get, set)]
    parser: Py<PyAny>,
    #[pyo3(get, set, name = "match")]
    r#match: Py<PyAny>,
}

#[pymethods]
impl ParseMatch {
    #[new]
    fn new(parser: Py<PyAny>, r#match: Py<PyAny>) -> Self {
        ParseMatch { parser, r#match }
    }

    fn __reduce__<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyTuple>> {
        let py = slf.py();
        let args = {
            let r = slf.borrow();
            vec![r.parser.bind(py).clone(), r.r#match.bind(py).clone()]
        };
        newobj_reduce(slf.as_any(), args)
    }

    fn evaluate_result(slf: &Bound<'_, Self>) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        let (parser, m) = {
            let r = slf.borrow();
            (r.parser.clone_ref(py), r.r#match.clone_ref(py))
        };
        Ok(parser
            .bind(py)
            .call_method1("evaluate_result", (m,))?
            .unbind())
    }
}

// ---------------------------------------------------------------- ResultIterator

/// `parse.ResultIterator`.
#[pyclass(module = "parse", name = "ResultIterator", subclass, dict)]
pub struct ResultIterator {
    #[pyo3(get, set)]
    parser: Py<PyAny>,
    #[pyo3(get, set)]
    string: Py<PyAny>,
    #[pyo3(get, set)]
    pos: Py<PyAny>,
    #[pyo3(get, set)]
    endpos: Py<PyAny>,
    #[pyo3(get, set)]
    evaluate_result: Py<PyAny>,
    /// `string` decoded once, so each `__next__` doesn't re-copy the input.
    /// Keyed on the object, because `string` is settable.
    text: Option<(Py<PyAny>, Arc<[char]>)>,
}

#[pymethods]
impl ResultIterator {
    #[new]
    #[pyo3(signature = (parser, string, pos, endpos, evaluate_result = None))]
    fn new(
        py: Python<'_>,
        parser: Py<PyAny>,
        string: Py<PyAny>,
        pos: Py<PyAny>,
        endpos: Py<PyAny>,
        evaluate_result: Option<Py<PyAny>>,
    ) -> Self {
        ResultIterator {
            parser,
            string,
            pos,
            endpos,
            evaluate_result: evaluate_result
                .unwrap_or_else(|| PyBool::new(py, true).to_owned().into_any().unbind()),
            text: None,
        }
    }

    fn __reduce__<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyTuple>> {
        let py = slf.py();
        let args = {
            let r = slf.borrow();
            vec![
                r.parser.bind(py).clone(),
                r.string.bind(py).clone(),
                r.pos.bind(py).clone(),
                r.endpos.bind(py).clone(),
                r.evaluate_result.bind(py).clone(),
            ]
        };
        newobj_reduce(slf.as_any(), args)
    }

    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(slf: &Bound<'_, Self>) -> PyResult<Option<Py<PyAny>>> {
        let py = slf.py();
        let (parser, string, pos, endpos) = {
            let r = slf.borrow();
            (
                r.parser.clone_ref(py).into_bound(py),
                r.string.clone_ref(py).into_bound(py),
                r.pos.clone_ref(py).into_bound(py),
                r.endpos.clone_ref(py).into_bound(py),
            )
        };
        let Ok(p) = parser.cast::<Parser>() else {
            return Err(PyAttributeError::new_err(format!(
                "'{}' object has no attribute '_search_re'",
                type_name(&parser)?
            )));
        };
        let text = || {
            if let Some((obj, text)) = &slf.borrow().text {
                if obj.is(&string) {
                    return Ok(Arc::clone(text));
                }
            }
            let text: Arc<[char]> = text_of(&string)?;
            slf.borrow_mut().text = Some((string.clone().unbind(), Arc::clone(&text)));
            Ok(text)
        };
        let Some(m) = p
            .get()
            .run_search(py, &string, &pos, &endpos, text, METHOD_FRAMES)?
        else {
            return Ok(None);
        };
        let end = m.span_of(0).1;
        slf.borrow_mut().pos = end.into_pyobject(py)?.into_any().unbind();
        let ev = slf.borrow().evaluate_result.clone_ref(py);
        Parser::wrap_match(py, Owner::Obj(p), m, ev.bind(py).is_truthy()?).map(Some)
    }

    fn next(slf: &Bound<'_, Self>) -> PyResult<Py<PyAny>> {
        match ResultIterator::__next__(slf)? {
            Some(v) => Ok(v),
            None => Err(pyo3::exceptions::PyStopIteration::new_err(())),
        }
    }
}

// ---------------------------------------------------------------- functions

/// `parse.parse(format, string, extra_types=None, evaluate_result=True, case_sensitive=False)`.
#[pyfunction]
#[pyo3(signature = (format, string, extra_types = None, evaluate_result = None, case_sensitive = None))]
fn parse<'py>(
    py: Python<'py>,
    format: &Bound<'py, PyAny>,
    string: &Bound<'py, PyAny>,
    extra_types: Option<&Bound<'py, PyAny>>,
    evaluate_result: Option<&Bound<'py, PyAny>>,
    case_sensitive: Option<&Bound<'py, PyAny>>,
) -> PyResult<Py<PyAny>> {
    let p = Parser::build(py, format, extra_types, case_sensitive)?;
    Parser::parse_from(
        py,
        Owner::Value(p),
        string,
        evaluate_result,
        FUNCTION_FRAMES,
    )
}

/// `parse.search(format, string, pos=0, endpos=None, extra_types=None, evaluate_result=True, case_sensitive=False)`.
#[pyfunction]
#[pyo3(signature = (format, string, pos = None, endpos = None, extra_types = None, evaluate_result = None, case_sensitive = None))]
#[allow(clippy::too_many_arguments, reason = "mirrors upstream signature")]
fn search<'py>(
    py: Python<'py>,
    format: &Bound<'py, PyAny>,
    string: &Bound<'py, PyAny>,
    pos: Option<&Bound<'py, PyAny>>,
    endpos: Option<&Bound<'py, PyAny>>,
    extra_types: Option<&Bound<'py, PyAny>>,
    evaluate_result: Option<&Bound<'py, PyAny>>,
    case_sensitive: Option<&Bound<'py, PyAny>>,
) -> PyResult<Py<PyAny>> {
    let p = Parser::build(py, format, extra_types, case_sensitive)?;
    Parser::search_from(
        py,
        Owner::Value(p),
        string,
        pos,
        endpos,
        evaluate_result,
        FUNCTION_FRAMES,
    )
}

/// `parse.findall(format, string, pos=0, endpos=None, extra_types=None, evaluate_result=True, case_sensitive=False)`.
#[pyfunction]
#[pyo3(signature = (format, string, pos = None, endpos = None, extra_types = None, evaluate_result = None, case_sensitive = None))]
#[allow(clippy::too_many_arguments, reason = "mirrors upstream signature")]
fn findall<'py>(
    py: Python<'py>,
    format: &Bound<'py, PyAny>,
    string: &Bound<'py, PyAny>,
    pos: Option<&Bound<'py, PyAny>>,
    endpos: Option<&Bound<'py, PyAny>>,
    extra_types: Option<&Bound<'py, PyAny>>,
    evaluate_result: Option<&Bound<'py, PyAny>>,
    case_sensitive: Option<&Bound<'py, PyAny>>,
) -> PyResult<ResultIterator> {
    let p = Bound::new(py, Parser::build(py, format, extra_types, case_sensitive)?)?;
    Parser::findall(&p, string, pos, endpos, None, evaluate_result)
}

/// `parse.compile(format, extra_types=None, case_sensitive=False)`.
#[pyfunction]
#[pyo3(signature = (format, extra_types = None, case_sensitive = None))]
fn compile(
    py: Python<'_>,
    format: &Bound<'_, PyAny>,
    extra_types: Option<&Bound<'_, PyAny>>,
    case_sensitive: Option<&Bound<'_, PyAny>>,
) -> PyResult<Parser> {
    Parser::build(py, format, extra_types, case_sensitive)
}

/// `parse.extract_format(format, extra_types)`: returns upstream's `locals()`.
#[pyfunction]
#[pyo3(signature = (format, extra_types))]
fn extract_format<'py>(
    py: Python<'py>,
    format: &Bound<'py, PyAny>,
    extra_types: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyDict>> {
    let Ok(s) = format.cast::<PyString>() else {
        return Err(not_subscriptable(format)?);
    };
    let spec = core_format::extract_format(&str_text(s)?, |t| {
        extra_types
            .contains(py_text(py, t)?)
            .map_err(BuildError::Py)
    })
    .map_err(|e| e.into_py(py))?;
    let opt = |c: Option<char>| {
        c.map(|c| py_text(py, c.encode_utf8(&mut [0; 4])))
            .transpose()
    };
    let type_ = py_text(py, &spec.type_)?;
    let d = PyDict::new(py);
    d.set_item("format", &type_)?;
    d.set_item("extra_types", extra_types)?;
    d.set_item("fill", opt(spec.fill)?)?;
    d.set_item("align", opt(spec.align)?)?;
    d.set_item("zero", spec.zero)?;
    d.set_item("width", py_text(py, &spec.width)?)?;
    if let Some(g) = spec.grouping {
        d.set_item("grouping", g.to_string())?;
    }
    if let Some(p) = &spec.precision {
        d.set_item("precision", py_text(py, p)?)?;
    }
    d.set_item("type", type_)?;
    Ok(d)
}

/// Make the classes look like upstream's pure-Python ones. PyO3 gives every
/// class the `tp_name` `parse.X`, which `AttributeError` messages show
/// (upstream: `'Parser' object has no attribute 'x'`, `type object 'Parser'
/// ...`). Assigning `__name__` resets `tp_name` to the bare name; `__module__`
/// and `__qualname__` are unchanged. `Parser`'s read-only attributes become
/// real `property` objects, which give upstream's `property 'format' of
/// 'Parser' object has no setter` errors.
fn plain_class_names(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    let mut classes = vec![
        py.get_type::<Parser>(),
        py.get_type::<ParseResult>(),
        py.get_type::<ParseMatch>(),
        py.get_type::<ResultIterator>(),
    ];
    for name in ["FixedTzOffset", "TooManyFields", "RepeatedNameError"] {
        if let Ok(t) = m.getattr(name)?.cast_into::<PyType>() {
            classes.push(t);
        }
    }
    for t in classes {
        let name = t.name()?;
        t.setattr("__name__", name)?;
    }
    let parser = py.get_type::<Parser>();
    let property = py.import("builtins")?.getattr("property")?;
    for name in ["format", "named_fields", "fixed_fields"] {
        let getter = parser.getattr(name)?.getattr("__get__")?;
        let prop = property.call1((getter,))?;
        prop.call_method1("__set_name__", (&parser, name))?;
        parser.setattr(name, prop)?;
    }
    Ok(())
}

/// Adds this task's public objects to the `parse` module.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Parser>()?;
    m.add_class::<ParseResult>()?;
    m.add_class::<ParseMatch>()?;
    m.add_class::<ResultIterator>()?;
    plain_class_names(m)?;
    m.add_function(wrap_pyfunction!(parse, m)?)?;
    m.add_function(wrap_pyfunction!(search, m)?)?;
    m.add_function(wrap_pyfunction!(findall, m)?)?;
    m.add_function(wrap_pyfunction!(compile, m)?)?;
    m.add_function(wrap_pyfunction!(extract_format, m)?)?;
    let table = PyDict::new(m.py());
    for (k, v) in parse_port_core::pyre::dt_format::DT_FORMAT_TO_REGEX {
        table.set_item(k, v)?;
    }
    m.add("dt_format_to_regex", table)?;
    Ok(())
}
