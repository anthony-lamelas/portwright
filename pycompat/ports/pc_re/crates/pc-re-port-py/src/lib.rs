//! Thin Python layer over pc-re-port-core: converts arguments to
//! `pc_re_port_core::Value`, results back to Python objects, and core errors
//! to exceptions. No behavior lives here (see `/work/out/decisions.md`).
#![forbid(unsafe_code)]

use pc_re_port_core::args::ArgSource;
use pc_re_port_core::{api, flags, pyrepr, Error, PyStr, ReError, Value};
use pyo3::exceptions::{
    PyAttributeError, PyException, PyIndexError, PyMemoryError, PyOverflowError, PyRecursionError,
    PyTypeError, PyValueError,
};
use pyo3::prelude::*;
use pyo3::types::{
    PyBool, PyByteArray, PyBytes, PyDict, PyFloat, PyFrozenSet, PyInt, PyList, PySet, PyString,
    PyTuple, PyWeakrefMethods, PyWeakrefReference,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// Containers nested deeper than this raise `RecursionError`, as CPython's
/// recursive `_check_builtin` would, instead of overflowing the Rust stack.
const MAX_DEPTH: usize = 900;

// ---------------------------------------------------------------------------
// Python -> Value

/// Code points of a Python str, lone surrogates included.
fn str_to_pystr(s: &Bound<'_, PyString>) -> PyResult<PyStr> {
    if let Ok(text) = s.to_str() {
        return Ok(if text.is_ascii() {
            text.bytes().map(u32::from).collect()
        } else {
            let mut out = Vec::with_capacity(text.len());
            out.extend(text.chars().map(u32::from));
            out
        });
    }
    let encoded = s.call_method1("encode", ("utf-32-le", "surrogatepass"))?;
    let bytes = encoded.cast::<PyBytes>()?.as_bytes();
    Ok(bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect())
}

fn type_name(obj: &Bound<'_, PyAny>) -> String {
    obj.get_type()
        .name()
        .and_then(|n| n.extract::<String>())
        .unwrap_or_else(|_| "object".to_owned())
}

fn items<'py>(iter: impl Iterator<Item = Bound<'py, PyAny>>, depth: usize) -> PyResult<Vec<Value>> {
    iter.map(|item| to_value(&item, depth + 1)).collect()
}

/// Converts one argument without interpreting it (exact types only).
fn to_value(obj: &Bound<'_, PyAny>, depth: usize) -> PyResult<Value> {
    if depth > MAX_DEPTH {
        return Err(PyRecursionError::new_err(
            "maximum recursion depth exceeded",
        ));
    }
    if obj.is_none() {
        return Ok(Value::None);
    }
    if obj.is_exact_instance_of::<PyBool>() {
        return Ok(Value::Bool(obj.extract()?));
    }
    if obj.is_exact_instance_of::<PyInt>() {
        return Ok(Value::Int(int_to_bigint(obj)?));
    }
    if obj.is_exact_instance_of::<PyFloat>() {
        return Ok(Value::Float(obj.extract()?));
    }
    if obj.is_exact_instance_of::<PyString>() {
        return Ok(Value::Str(str_to_pystr(obj.cast::<PyString>()?)?));
    }
    if obj.is_exact_instance_of::<PyBytes>() {
        return Ok(Value::Bytes(obj.cast::<PyBytes>()?.as_bytes().to_vec()));
    }
    if obj.is_exact_instance_of::<PyByteArray>() {
        return Ok(Value::ByteArray(obj.cast::<PyByteArray>()?.to_vec()));
    }
    if obj.is_exact_instance_of::<PyList>() {
        return Ok(Value::List(items(obj.cast::<PyList>()?.iter(), depth)?));
    }
    if obj.is_exact_instance_of::<PyTuple>() {
        return Ok(Value::Tuple(items(obj.cast::<PyTuple>()?.iter(), depth)?));
    }
    if obj.is_exact_instance_of::<PySet>() {
        return Ok(Value::Set(items(obj.cast::<PySet>()?.iter(), depth)?));
    }
    if obj.is_exact_instance_of::<PyFrozenSet>() {
        return Ok(Value::FrozenSet(items(
            obj.cast::<PyFrozenSet>()?.iter(),
            depth,
        )?));
    }
    if obj.is_exact_instance_of::<PyDict>() {
        let mut pairs = Vec::new();
        for (k, v) in obj.cast::<PyDict>()?.iter() {
            pairs.push((to_value(&k, depth + 1)?, to_value(&v, depth + 1)?));
        }
        return Ok(Value::Dict(pairs));
    }
    if let Ok(p) = obj.cast_exact::<PyPattern>() {
        return Ok(Value::Pattern(Arc::clone(&p.get().inner)));
    }
    if let Ok(m) = obj.cast_exact::<PyMatch>() {
        return Ok(Value::Match(Arc::clone(&m.get().inner)));
    }
    let qualified = obj.get_type().fully_qualified_name();
    let type_is = |name: &str| {
        qualified
            .as_ref()
            .is_ok_and(|n| n.to_str().is_ok_and(|n| n == name))
    };
    // The shim admits `re.RegexFlag` (an `int` subclass); it behaves as its int value.
    if type_is("re.RegexFlag") && obj.is_instance_of::<PyInt>() {
        return Ok(Value::Int(int_to_bigint(obj)?));
    }
    let is_decimal = type_is("decimal.Decimal");
    if is_decimal {
        return Ok(match obj.extract::<rust_decimal::Decimal>() {
            Ok(d) => Value::Decimal(d.to_string()),
            Err(_) => Value::Other("Decimal".to_owned()),
        });
    }
    Ok(Value::Other(type_name(obj)))
}

/// A Python int as a `BigInt`; most fit an `i64`, which converts far faster.
fn int_to_bigint(obj: &Bound<'_, PyAny>) -> PyResult<num_bigint::BigInt> {
    match obj.extract::<i64>() {
        Ok(n) => Ok(num_bigint::BigInt::from(n)),
        Err(_) => obj.extract(),
    }
}

/// A `BigInt` as a Python int, through `i64` when it fits.
fn bigint_to_py<'py>(py: Python<'py>, n: &num_bigint::BigInt) -> PyResult<Bound<'py, PyAny>> {
    match i64::try_from(n) {
        Ok(small) => Ok(small.into_pyobject(py)?.into_any()),
        Err(_) => Ok(n.into_pyobject(py)?.into_any()),
    }
}

type Call = (Vec<Value>, Vec<(String, Value)>);

fn convert_call(args: &Bound<'_, PyTuple>, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Call> {
    let positional = items(args.iter(), 0)?;
    let mut keywords = Vec::new();
    if let Some(kwargs) = kwargs {
        for (k, v) in kwargs.iter() {
            let key = str_to_pystr(k.cast::<PyString>()?)?;
            keywords.push((
                pc_re_port_core::text::to_string_lossy(&key),
                to_value(&v, 0)?,
            ));
        }
    }
    Ok((positional, keywords))
}

// ---------------------------------------------------------------------------
// Value -> Python

/// `s` as a Rust string, or `None` if it holds a lone surrogate.
fn to_rust_string(s: &[u32]) -> Option<String> {
    let mut text = String::with_capacity(s.len());
    for &c in s {
        text.push(char::from_u32(c)?);
    }
    Some(text)
}

fn pystr_to_py<'py>(py: Python<'py>, s: &[u32]) -> PyResult<Bound<'py, PyAny>> {
    if let Some(text) = to_rust_string(s) {
        return Ok(PyString::new(py, &text).into_any());
    }
    let bytes: Vec<u8> = s.iter().flat_map(|c| c.to_le_bytes()).collect();
    PyBytes::new(py, &bytes).call_method1("decode", ("utf-32-le", "surrogatepass"))
}

fn to_py_list<'py>(py: Python<'py>, values: &[Value]) -> PyResult<Vec<Bound<'py, PyAny>>> {
    values.iter().map(|v| to_py(py, v)).collect()
}

fn to_py<'py>(py: Python<'py>, value: &Value) -> PyResult<Bound<'py, PyAny>> {
    Ok(match value {
        Value::None => py.None().into_bound(py),
        Value::Bool(b) => PyBool::new(py, *b).to_owned().into_any(),
        Value::Int(n) => bigint_to_py(py, n)?,
        Value::Float(f) => PyFloat::new(py, *f).into_any(),
        Value::Str(s) => pystr_to_py(py, s)?,
        Value::Bytes(b) => PyBytes::new(py, b).into_any(),
        Value::ByteArray(b) => PyByteArray::new(py, b).into_any(),
        Value::List(items) => PyList::new(py, to_py_list(py, items)?)?.into_any(),
        Value::Tuple(items) => PyTuple::new(py, to_py_list(py, items)?)?.into_any(),
        Value::Set(items) => PySet::new(py, to_py_list(py, items)?)?.into_any(),
        Value::FrozenSet(items) => PyFrozenSet::new(py, to_py_list(py, items)?)?.into_any(),
        Value::Dict(pairs) => {
            let dict = PyDict::new(py);
            for (k, v) in pairs {
                dict.set_item(to_py(py, k)?, to_py(py, v)?)?;
            }
            dict.into_any()
        }
        Value::Decimal(text) => text
            .parse::<rust_decimal::Decimal>()
            .map_err(|e| PyValueError::new_err(e.to_string()))?
            .into_pyobject(py)?,
        Value::Pattern(p) => pattern_object(py, p)?.into_any(),
        Value::Match(m) => Bound::new(
            py,
            PyMatch {
                inner: Arc::clone(m),
            },
        )?
        .into_any(),
        Value::Other(_) => {
            return Err(PyTypeError::new_err(format!(
                "cannot convert {} back to Python",
                value.type_name()
            )))
        }
    })
}

fn opt_int<'py>(py: Python<'py>, n: Option<&num_bigint::BigInt>) -> PyResult<Py<PyAny>> {
    Ok(match n {
        Some(n) => bigint_to_py(py, n)?.unbind(),
        None => py.None(),
    })
}

// ---------------------------------------------------------------------------
// Pattern objects

/// `re.Pattern`: a compiled pattern. Behavior lives in the core.
#[pyclass(name = "Pattern", module = "re", frozen, weakref)]
struct PyPattern {
    inner: Arc<pc_re_port_core::Pattern>,
}

#[pymethods]
impl PyPattern {
    fn __repr__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let text =
            pyrepr::repr(&Value::Pattern(Arc::clone(&self.inner))).map_err(|e| to_pyerr(py, e))?;
        pystr_to_py(py, &text)
    }
}

/// `re.Match`: a successful match. Behavior lives in the core.
#[pyclass(name = "Match", module = "re", frozen)]
struct PyMatch {
    inner: Arc<pc_re_port_core::Match>,
}

#[pymethods]
impl PyMatch {
    fn __repr__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let text =
            pyrepr::repr(&Value::Match(Arc::clone(&self.inner))).map_err(|e| to_pyerr(py, e))?;
        pystr_to_py(py, &text)
    }
}

type Identity = Mutex<HashMap<usize, Py<PyWeakrefReference>>>;

/// Core pattern address to its live Python object, so one core pattern is
/// always the same Python object (as `re`'s cache returns the same object).
fn identities() -> &'static Identity {
    static MAP: OnceLock<Identity> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

fn pattern_object<'py>(
    py: Python<'py>,
    p: &Arc<pc_re_port_core::Pattern>,
) -> PyResult<Bound<'py, PyPattern>> {
    let key = Arc::as_ptr(p) as usize;
    let mut map = identities()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(existing) = map.get(&key).and_then(|w| w.bind(py).upgrade()) {
        if let Ok(obj) = existing.cast_into::<PyPattern>() {
            return Ok(obj);
        }
    }
    let obj = Bound::new(
        py,
        PyPattern {
            inner: Arc::clone(p),
        },
    )?;
    if map.len() >= 1024 {
        map.retain(|_, w| w.bind(py).upgrade().is_some());
    }
    map.insert(key, PyWeakrefReference::new(&obj)?.unbind());
    Ok(obj)
}

// ---------------------------------------------------------------------------
// Errors

fn to_pyerr(py: Python<'_>, err: Error) -> PyErr {
    match err {
        Error::Re(e) => match re_error(py, &e) {
            Ok(err) | Err(err) => err,
        },
        Error::Type(m) => PyTypeError::new_err(m),
        Error::Value(m) => PyValueError::new_err(m),
        Error::Overflow(m) => PyOverflowError::new_err(m),
        Error::Index(m) => PyIndexError::new_err(m),
        Error::Attribute(m) => PyAttributeError::new_err(m),
        Error::Recursion(m) => PyRecursionError::new_err(m),
        // `PyErr_NoMemory()` raises `MemoryError()` with empty `args`.
        Error::Memory(m) if m.is_empty() => PyMemoryError::new_err(()),
        Error::Memory(m) => PyMemoryError::new_err(m),
        other => PyException::new_err(other.to_string()),
    }
}

/// An `re.error` instance for an engine error.
fn re_error(py: Python<'_>, e: &ReError) -> PyResult<PyErr> {
    let message = e.message();
    let to_int = |n: Option<usize>| opt_int(py, n.map(num_bigint::BigInt::from).as_ref());
    let state = ErrorState {
        args: PyTuple::new(py, [pystr_to_py(py, &message)?])?.unbind(),
        msg: pystr_to_py(py, &e.msg)?.unbind(),
        pattern: match &e.pattern {
            Some(p) => pystr_to_py(py, p)?.unbind(),
            None => py.None(),
        },
        pos: to_int(e.pos)?,
        lineno: to_int(e.lineno())?,
        colno: to_int(e.colno())?,
        text: Ok(message.clone()),
        repr_arg: Value::Str(message),
    };
    let obj = Bound::new(py, PyReError { state })?;
    Ok(PyErr::from_value(obj.into_any()))
}

#[derive(Debug)]
struct ErrorState {
    args: Py<PyTuple>,
    msg: Py<PyAny>,
    pattern: Py<PyAny>,
    pos: Py<PyAny>,
    lineno: Py<PyAny>,
    colno: Py<PyAny>,
    text: Result<PyStr, Error>,
    /// `args[0]` as a core value, for `repr`.
    repr_arg: Value,
}

impl ErrorState {
    fn init(args: &Bound<'_, PyTuple>, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        let py = args.py();
        let (values, keywords) = convert_call(args, kwargs)?;
        let init = api::error_init(&values, &keywords).map_err(|e| to_pyerr(py, e))?;
        let original = |source: Option<ArgSource>| -> PyResult<Py<PyAny>> {
            Ok(match source {
                Some(ArgSource::Positional(i)) => args.get_item(i)?.unbind(),
                Some(ArgSource::Keyword(i)) => match kwargs {
                    Some(kw) => kw
                        .values()
                        .get_item(i)
                        .map_or_else(|_| py.None(), Bound::unbind),
                    None => py.None(),
                },
                None => py.None(),
            })
        };
        let msg = original(Some(init.msg))?;
        let (args0, repr_arg) = match &init.formatted {
            Some(text) => (pystr_to_py(py, text)?.unbind(), Value::Str(text.clone())),
            None => (
                msg.clone_ref(py),
                init.msg
                    .get(&values, &keywords)
                    .cloned()
                    .unwrap_or(Value::None),
            ),
        };
        let (lineno, colno) = match &init.location {
            Some((l, c)) => (opt_int(py, Some(l))?, opt_int(py, Some(c))?),
            None => (py.None(), py.None()),
        };
        Ok(Self {
            args: PyTuple::new(py, [args0])?.unbind(),
            msg,
            pattern: original(init.pattern)?,
            pos: original(init.pos)?,
            lineno,
            colno,
            text: init.text,
            repr_arg,
        })
    }
}

/// `re.error`: Exception raised for invalid regular expressions.
#[pyclass(extends = PyException, name = "error", module = "re", subclass)]
struct PyReError {
    state: ErrorState,
}

#[pymethods]
impl PyReError {
    #[new]
    #[pyo3(signature = (*args, **kwargs))]
    fn new(args: &Bound<'_, PyTuple>, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        Ok(Self {
            state: ErrorState::init(args, kwargs)?,
        })
    }

    #[pyo3(signature = (*args, **kwargs))]
    fn __init__(
        mut slf: PyRefMut<'_, Self>,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<()> {
        slf.state = ErrorState::init(args, kwargs)?;
        Ok(())
    }

    #[getter]
    fn args(&self, py: Python<'_>) -> Py<PyTuple> {
        self.state.args.clone_ref(py)
    }

    #[getter]
    fn msg(&self, py: Python<'_>) -> Py<PyAny> {
        self.state.msg.clone_ref(py)
    }

    #[getter]
    fn pattern(&self, py: Python<'_>) -> Py<PyAny> {
        self.state.pattern.clone_ref(py)
    }

    #[getter]
    fn pos(&self, py: Python<'_>) -> Py<PyAny> {
        self.state.pos.clone_ref(py)
    }

    #[getter]
    fn lineno(&self, py: Python<'_>) -> Py<PyAny> {
        self.state.lineno.clone_ref(py)
    }

    #[getter]
    fn colno(&self, py: Python<'_>) -> Py<PyAny> {
        self.state.colno.clone_ref(py)
    }

    fn __str__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        match &self.state.text {
            Ok(text) => pystr_to_py(py, text),
            Err(e) => Err(to_pyerr(py, e.clone())),
        }
    }

    fn __repr__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let arg = pyrepr::repr(&self.state.repr_arg).map_err(|e| to_pyerr(py, e))?;
        let mut text: PyStr = "error(".chars().map(u32::from).collect();
        text.extend(arg);
        text.push(u32::from(')'));
        pystr_to_py(py, &text)
    }
}

// ---------------------------------------------------------------------------
// Functions

macro_rules! api_functions {
    ($($name:ident),* $(,)?) => {
        $(
            #[pyfunction]
            #[pyo3(signature = (*args, **kwargs))]
            fn $name<'py>(
                args: &Bound<'py, PyTuple>,
                kwargs: Option<&Bound<'py, PyDict>>,
            ) -> PyResult<Bound<'py, PyAny>> {
                let py = args.py();
                let (values, keywords) = convert_call(args, kwargs)?;
                let result = api::$name(&values, &keywords).map_err(|e| to_pyerr(py, e))?;
                to_py(py, &result)
            }
        )*
    };
}

api_functions!(
    compile,
    pattern_text,
    pattern_flags,
    pattern_groups,
    pattern_groupindex,
    search,
    r#match,
    fullmatch,
    pattern_search,
    pattern_match,
    pattern_fullmatch,
    match_group,
    match_groups,
    match_groupdict,
    match_start,
    match_end,
    match_span,
    match_lastindex,
    match_lastgroup,
    match_pos,
    match_endpos,
    match_string,
    findall,
    finditer,
    pattern_findall,
    pattern_finditer,
    split,
    pattern_split,
    sub,
    subn,
    pattern_sub,
    pattern_subn,
    match_expand,
    escape,
);

#[pymodule(name = "pc_re")]
fn module_init(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyReError>()?;
    m.add_function(wrap_pyfunction!(escape, m)?)?;
    m.add_function(wrap_pyfunction!(compile, m)?)?;
    m.add_function(wrap_pyfunction!(pattern_text, m)?)?;
    m.add_function(wrap_pyfunction!(pattern_flags, m)?)?;
    m.add_function(wrap_pyfunction!(pattern_groups, m)?)?;
    m.add_function(wrap_pyfunction!(pattern_groupindex, m)?)?;
    m.add_class::<PyMatch>()?;
    m.add_function(wrap_pyfunction!(search, m)?)?;
    m.add_function(wrap_pyfunction!(r#match, m)?)?;
    m.add_function(wrap_pyfunction!(fullmatch, m)?)?;
    m.add_function(wrap_pyfunction!(pattern_search, m)?)?;
    m.add_function(wrap_pyfunction!(pattern_match, m)?)?;
    m.add_function(wrap_pyfunction!(pattern_fullmatch, m)?)?;
    m.add_function(wrap_pyfunction!(match_group, m)?)?;
    m.add_function(wrap_pyfunction!(match_groups, m)?)?;
    m.add_function(wrap_pyfunction!(match_groupdict, m)?)?;
    m.add_function(wrap_pyfunction!(match_start, m)?)?;
    m.add_function(wrap_pyfunction!(match_end, m)?)?;
    m.add_function(wrap_pyfunction!(match_span, m)?)?;
    m.add_function(wrap_pyfunction!(match_lastindex, m)?)?;
    m.add_function(wrap_pyfunction!(match_lastgroup, m)?)?;
    m.add_function(wrap_pyfunction!(match_pos, m)?)?;
    m.add_function(wrap_pyfunction!(match_endpos, m)?)?;
    m.add_function(wrap_pyfunction!(match_string, m)?)?;
    m.add_function(wrap_pyfunction!(findall, m)?)?;
    m.add_function(wrap_pyfunction!(finditer, m)?)?;
    m.add_function(wrap_pyfunction!(pattern_findall, m)?)?;
    m.add_function(wrap_pyfunction!(pattern_finditer, m)?)?;
    m.add_function(wrap_pyfunction!(split, m)?)?;
    m.add_function(wrap_pyfunction!(pattern_split, m)?)?;
    m.add_function(wrap_pyfunction!(sub, m)?)?;
    m.add_function(wrap_pyfunction!(subn, m)?)?;
    m.add_function(wrap_pyfunction!(pattern_sub, m)?)?;
    m.add_function(wrap_pyfunction!(pattern_subn, m)?)?;
    m.add_function(wrap_pyfunction!(match_expand, m)?)?;
    for (name, value) in [
        ("NOFLAG", flags::NOFLAG),
        ("ASCII", flags::ASCII),
        ("IGNORECASE", flags::IGNORECASE),
        ("LOCALE", flags::LOCALE),
        ("MULTILINE", flags::MULTILINE),
        ("DOTALL", flags::DOTALL),
        ("VERBOSE", flags::VERBOSE),
        ("UNICODE", flags::UNICODE),
    ] {
        m.add(name, value)?;
    }
    Ok(())
}
