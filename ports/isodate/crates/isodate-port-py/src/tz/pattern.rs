//! Compiled patterns returned by `build_date_regexps`, `build_time_regexps` and
//! `isotzinfo.TZ_RE`, with `re.Pattern`'s API, backed by `pc_re`.
//!
//! The types are the port's own (`isodate.isotzinfo.Pattern` / `Match` / `Scanner`;
//! an accepted difference in type identity). Their behavior follows CPython's `_sre`:
//! argument clinic's `TypeError` messages, conversion order (integer arguments
//! before the subject string), match objects, scanners and substitution.

use std::sync::{Arc, Mutex, PoisonError};

use isodate_port_core::pc_re::matching::{clamp, Match, Mode};
use isodate_port_core::pc_re::template::{self, Template};
use isodate_port_core::pc_re::{scan, Error as ReErr, Pattern, Value};
use num_bigint::BigInt;
use pyo3::exceptions::{
    PyAttributeError, PyIndexError, PyMemoryError, PyOverflowError, PyRecursionError, PyTypeError,
    PyValueError,
};
use pyo3::prelude::*;
use pyo3::types::{
    PyByteArray, PyBytes, PyDict, PyInt, PyList, PyMappingProxy, PyMemoryView, PyString, PyTuple,
};

use super::{match_text, str_cps, tp_name};
use crate::text::cps_to_py;

/// Converts a `pc_re` error to the exception `re` raises. `re.error` itself can't be
/// reached without importing `re`, so its cases raise `ValueError` with its message.
fn re_err(py: Python<'_>, error: ReErr) -> PyErr {
    match error {
        ReErr::Re(e) => match cps_to_py(py, &e.message()) {
            Ok(text) => PyValueError::new_err(text.unbind()),
            Err(err) => err,
        },
        ReErr::Type(m) => PyTypeError::new_err(m),
        ReErr::Overflow(m) => PyOverflowError::new_err(m),
        ReErr::Index(m) => PyIndexError::new_err(m),
        ReErr::Attribute(m) => PyAttributeError::new_err(m),
        ReErr::Recursion(m) => PyRecursionError::new_err(m),
        ReErr::Memory(m) => PyMemoryError::new_err(m),
        other => PyValueError::new_err(other.to_string()),
    }
}

/// Argument clinic's `_PyArg_UnpackKeywords` for positional-or-keyword parameters,
/// with its messages and checking order.
fn clinic<'py>(
    fname: &str,
    params: &[&str],
    required: usize,
    args: &Bound<'py, PyTuple>,
    kwargs: Option<&Bound<'py, PyDict>>,
) -> PyResult<Vec<Option<Bound<'py, PyAny>>>> {
    let nargs = args.len();
    let nkw = kwargs.map_or(0, |k| k.len());
    let max = params.len();
    if nargs + nkw > max {
        return Err(PyTypeError::new_err(format!(
            "{fname}() takes at most {max} {}argument{} ({} given)",
            if nargs == 0 { "keyword " } else { "" },
            if max == 1 { "" } else { "s" },
            nargs + nkw
        )));
    }
    let mut out: Vec<Option<Bound<'py, PyAny>>> = args.iter().map(Some).collect();
    let mut remaining = nkw;
    for (i, name) in params.iter().enumerate().skip(nargs) {
        let current = match kwargs {
            Some(k) if remaining > 0 => k.get_item(name)?,
            _ if i >= required => break,
            _ => None,
        };
        if current.is_some() {
            remaining -= 1;
        } else if i < required {
            return Err(PyTypeError::new_err(format!(
                "{fname}() missing required argument '{name}' (pos {})",
                i + 1
            )));
        }
        out.push(current);
    }
    if let Some(k) = kwargs.filter(|_| remaining > 0) {
        for (i, name) in params.iter().enumerate().take(nargs) {
            if k.contains(name)? {
                return Err(PyTypeError::new_err(format!(
                    "argument for {fname}() given by name ('{name}') and position ({})",
                    i + 1
                )));
            }
        }
        for key in k.keys() {
            let known = match key.cast::<PyString>() {
                Ok(s) => params.contains(&s.to_str()?),
                Err(_) => false,
            };
            if !known {
                return Err(PyTypeError::new_err(format!(
                    "{} is an invalid keyword argument for {fname}()",
                    key.repr()?
                )));
            }
        }
    }
    out.resize_with(max, || None);
    Ok(out)
}

/// `TypeError` for a method that takes no keyword arguments (`Match.span(x=1)`).
fn no_keywords(fname: &str, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<()> {
    match kwargs {
        Some(k) if !k.is_empty() => Err(PyTypeError::new_err(format!(
            "{fname}() takes no keyword arguments"
        ))),
        _ => Ok(()),
    }
}

/// A `Py_ssize_t` argument (`pos`, `endpos`, `count`, `maxsplit`).
fn ssize(value: Option<&Bound<'_, PyAny>>, default: isize) -> PyResult<isize> {
    match value.map_or(Ok(default), |v| v.extract::<isize>()) {
        Err(e) if value.is_some_and(|v| e.is_instance_of::<PyOverflowError>(v.py())) => Err(
            PyOverflowError::new_err("Python int too large to convert to C ssize_t"),
        ),
        other => other,
    }
}

/// A `pc_re` result value (`str`, `None` or a tuple of them) as a Python object.
fn value_to_py<'py>(py: Python<'py>, value: &Value) -> PyResult<Bound<'py, PyAny>> {
    Ok(match value {
        Value::Str(s) => cps_to_py(py, s)?.into_any(),
        Value::Tuple(items) => {
            let items = items
                .iter()
                .map(|v| value_to_py(py, v))
                .collect::<PyResult<Vec<_>>>()?;
            PyTuple::new(py, items)?.into_any()
        }
        _ => py.None().into_bound(py),
    })
}

/// Whether `obj` supports the buffer protocol (`bytes`, `bytearray`, `memoryview`...).
fn bytes_like(obj: &Bound<'_, PyAny>) -> bool {
    obj.is_instance_of::<PyBytes>()
        || obj.is_instance_of::<PyByteArray>()
        || obj.is_instance_of::<PyMemoryView>()
        || PyMemoryView::from(obj).is_ok()
}

/// `re._compile_template`'s `TypeError` for a template that is neither `str` nor
/// bytes-like.
fn template_type_error(obj: &Bound<'_, PyAny>) -> PyResult<PyErr> {
    Ok(PyTypeError::new_err(format!(
        "decoding to str: need a bytes-like object, {} found",
        tp_name(obj)?
    )))
}

/// A compiled pattern (`TZ_RE`, `build_date_regexps()`...), backed by `pc_re`.
#[pyclass(name = "Pattern", module = "isodate.isotzinfo", frozen)]
pub struct RePattern {
    inner: Arc<Pattern>,
}

impl RePattern {
    /// Wraps a compiled pattern.
    pub(crate) fn new(inner: Arc<Pattern>) -> Self {
        Self { inner }
    }

    /// Binds `(string, pos=0, endpos=sys.maxsize)`, converting `pos` / `endpos`
    /// before checking the subject, as `_sre` does.
    fn window<'py>(
        fname: &str,
        args: &Bound<'py, PyTuple>,
        kwargs: Option<&Bound<'py, PyDict>>,
    ) -> PyResult<Window<'py>> {
        let bound = clinic(fname, &["string", "pos", "endpos"], 1, args, kwargs)?;
        let pos = ssize(bound.get(1).and_then(Option::as_ref), 0)?;
        let endpos = ssize(bound.get(2).and_then(Option::as_ref), isize::MAX)?;
        let Some(Some(string)) = bound.into_iter().next() else {
            return Err(PyTypeError::new_err(format!(
                "{fname}() missing required argument 'string' (pos 1)"
            )));
        };
        let text: Arc<[u32]> = match_text(&string)?.into();
        let len = text.len();
        Ok(Window {
            string,
            text,
            pos: clamp(pos, len),
            endpos: clamp(endpos, len),
        })
    }

    fn run(
        slf: &Bound<'_, Self>,
        fname: &str,
        mode: Mode,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Option<Py<ReMatch>>> {
        let py = slf.py();
        let w = Self::window(fname, args, kwargs)?;
        let found = Match::run(&slf.get().inner, &w.text, w.pos, w.endpos, mode)
            .map_err(|e| re_err(py, e))?;
        found
            .map(|m| ReMatch::create(slf, &w.string, m, w.pos, w.endpos))
            .transpose()
    }

    /// `pattern.sub` / `pattern.subn`.
    fn substitute<'py>(
        slf: &Bound<'py, Self>,
        fname: &str,
        args: &Bound<'py, PyTuple>,
        kwargs: Option<&Bound<'py, PyDict>>,
    ) -> PyResult<(Bound<'py, PyAny>, usize)> {
        let py = slf.py();
        let bound = clinic(fname, &["repl", "string", "count"], 2, args, kwargs)?;
        let count = ssize(bound.get(2).and_then(Option::as_ref), 0)?;
        let mut bound = bound.into_iter();
        let (Some(Some(repl)), Some(Some(string))) = (bound.next(), bound.next()) else {
            return Err(PyTypeError::new_err(format!(
                "{fname}() missing required argument"
            )));
        };
        let pattern = &slf.get().inner;
        // The replacement is examined before the subject string.
        let template = if repl.is_callable() {
            None
        } else if let Ok(s) = repl.cast::<PyString>() {
            Some(Template::parse(&str_cps(s)?, pattern).map_err(|e| re_err(py, e))?)
        } else if bytes_like(&repl) {
            None
        } else {
            return Err(template_type_error(&repl)?);
        };
        let text: Arc<[u32]> = match_text(&string)?.into();
        if let Some(template) = template {
            let (out, n) = template::substitute(pattern, &template, &text, count)
                .map_err(|e| re_err(py, e))?;
            return Ok((cps_to_py(py, &out)?.into_any(), n));
        }
        // A callable (or a bytes-like literal, which `str.join` then rejects as
        // CPython's does): build the pieces and join them.
        let len = text.len();
        let matches = scan::finditer(pattern, &text, 0, len).map_err(|e| re_err(py, e))?;
        let limit = match usize::try_from(count) {
            Ok(0) => usize::MAX,
            Ok(c) => c,
            Err(_) => 0,
        };
        let pieces = PyList::empty(py);
        let mut i = 0;
        let mut n = 0;
        for m in matches.into_iter().take(limit) {
            let (b, e) = m.span_of(0);
            let (b, e) = (
                usize::try_from(b).unwrap_or(0),
                usize::try_from(e).unwrap_or(0),
            );
            if i < b {
                pieces.append(cps_to_py(py, text.get(i..b).unwrap_or_default())?)?;
            }
            let item = if repl.is_callable() {
                let m = ReMatch::create(slf, &string, m, 0, len)?;
                repl.call1((m,))?
            } else {
                repl.clone()
            };
            if !item.is_none() {
                pieces.append(item)?;
            }
            i = e;
            n += 1;
        }
        if i < len {
            pieces.append(cps_to_py(py, text.get(i..).unwrap_or_default())?)?;
        }
        let joined = PyString::new(py, "").call_method1("join", (pieces,))?;
        Ok((joined, n))
    }

    fn groupindex_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        for (name, &index) in self.inner.groupindex() {
            dict.set_item(cps_to_py(py, name)?, index)?;
        }
        Ok(dict)
    }
}

/// A bound subject: the original object, its code points and the clamped window.
struct Window<'py> {
    string: Bound<'py, PyAny>,
    text: Arc<[u32]>,
    pos: usize,
    endpos: usize,
}

#[pymethods]
impl RePattern {
    fn __repr__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyString>> {
        cps_to_py(py, &self.inner.repr())
    }

    /// `pattern.pattern`.
    #[getter]
    fn pattern<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyString>> {
        cps_to_py(py, self.inner.pattern())
    }

    /// `pattern.flags`.
    #[getter]
    fn flags(&self) -> i32 {
        self.inner.flags()
    }

    /// `pattern.groups`.
    #[getter]
    fn groups(&self) -> usize {
        self.inner.groups()
    }

    /// `pattern.groupindex`: a read-only mapping of group names to numbers.
    #[getter]
    fn groupindex<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyMappingProxy>> {
        let dict = self.groupindex_dict(py)?;
        Ok(PyMappingProxy::new(py, dict.as_mapping()))
    }

    #[pyo3(signature = (*args, **kwargs))]
    fn r#match(
        slf: &Bound<'_, Self>,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Option<Py<ReMatch>>> {
        Self::run(slf, "match", Mode::Match, args, kwargs)
    }

    #[pyo3(signature = (*args, **kwargs))]
    fn fullmatch(
        slf: &Bound<'_, Self>,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Option<Py<ReMatch>>> {
        Self::run(slf, "fullmatch", Mode::FullMatch, args, kwargs)
    }

    #[pyo3(signature = (*args, **kwargs))]
    fn search(
        slf: &Bound<'_, Self>,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Option<Py<ReMatch>>> {
        Self::run(slf, "search", Mode::Search, args, kwargs)
    }

    #[pyo3(signature = (*args, **kwargs))]
    fn findall<'py>(
        slf: &Bound<'py, Self>,
        args: &Bound<'py, PyTuple>,
        kwargs: Option<&Bound<'py, PyDict>>,
    ) -> PyResult<Bound<'py, PyList>> {
        let py = slf.py();
        let w = Self::window("findall", args, kwargs)?;
        let found =
            scan::findall(&slf.get().inner, &w.text, w.pos, w.endpos).map_err(|e| re_err(py, e))?;
        let items = found
            .iter()
            .map(|v| value_to_py(py, v))
            .collect::<PyResult<Vec<_>>>()?;
        PyList::new(py, items)
    }

    #[pyo3(signature = (*args, **kwargs))]
    fn finditer<'py>(
        slf: &Bound<'py, Self>,
        args: &Bound<'py, PyTuple>,
        kwargs: Option<&Bound<'py, PyDict>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let py = slf.py();
        let w = Self::window("finditer", args, kwargs)?;
        let found = scan::finditer(&slf.get().inner, &w.text, w.pos, w.endpos)
            .map_err(|e| re_err(py, e))?;
        let items = found
            .into_iter()
            .map(|m| ReMatch::create(slf, &w.string, m, w.pos, w.endpos))
            .collect::<PyResult<Vec<_>>>()?;
        Ok(PyList::new(py, items)?.try_iter()?.into_any())
    }

    #[pyo3(signature = (*args, **kwargs))]
    fn scanner(
        slf: &Bound<'_, Self>,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<ReScanner> {
        let w = Self::window("scanner", args, kwargs)?;
        Ok(ReScanner {
            re: slf.clone().unbind(),
            string: w.string.unbind(),
            text: w.text,
            pos: w.pos,
            endpos: w.endpos,
            state: Mutex::new(ScanState {
                start: Some(w.pos),
                must_advance: false,
            }),
        })
    }

    #[pyo3(signature = (*args, **kwargs))]
    fn split<'py>(
        slf: &Bound<'py, Self>,
        args: &Bound<'py, PyTuple>,
        kwargs: Option<&Bound<'py, PyDict>>,
    ) -> PyResult<Bound<'py, PyList>> {
        let py = slf.py();
        let bound = clinic("split", &["string", "maxsplit"], 1, args, kwargs)?;
        let maxsplit = ssize(bound.get(1).and_then(Option::as_ref), 0)?;
        let Some(Some(string)) = bound.into_iter().next() else {
            return Err(PyTypeError::new_err(
                "split() missing required argument 'string' (pos 1)",
            ));
        };
        let text = match_text(&string)?;
        let parts = scan::split(&slf.get().inner, &text, maxsplit).map_err(|e| re_err(py, e))?;
        let items = parts
            .iter()
            .map(|v| value_to_py(py, v))
            .collect::<PyResult<Vec<_>>>()?;
        PyList::new(py, items)
    }

    #[pyo3(signature = (*args, **kwargs))]
    fn sub<'py>(
        slf: &Bound<'py, Self>,
        args: &Bound<'py, PyTuple>,
        kwargs: Option<&Bound<'py, PyDict>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        Ok(Self::substitute(slf, "sub", args, kwargs)?.0)
    }

    #[pyo3(signature = (*args, **kwargs))]
    fn subn<'py>(
        slf: &Bound<'py, Self>,
        args: &Bound<'py, PyTuple>,
        kwargs: Option<&Bound<'py, PyDict>>,
    ) -> PyResult<Bound<'py, PyTuple>> {
        let (text, n) = Self::substitute(slf, "subn", args, kwargs)?;
        PyTuple::new(slf.py(), [text, n.into_pyobject(slf.py())?.into_any()])
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let py = other.py();
        let Ok(other) = other.cast::<RePattern>() else {
            return Ok(py.NotImplemented());
        };
        let other = &other.get().inner;
        let same = Arc::ptr_eq(&self.inner, other)
            || (self.inner.flags() == other.flags() && self.inner.pattern() == other.pattern());
        Ok(same.into_pyobject(py)?.to_owned().into_any().unbind())
    }

    fn __hash__(&self, py: Python<'_>) -> PyResult<isize> {
        let text = cps_to_py(py, self.inner.pattern())?;
        Ok(text.hash()? ^ isize::try_from(self.inner.flags()).unwrap_or(0))
    }

    fn __copy__(slf: Bound<'_, Self>) -> Bound<'_, Self> {
        slf
    }

    fn __deepcopy__<'py>(slf: Bound<'py, Self>, _memo: &Bound<'py, PyAny>) -> Bound<'py, Self> {
        slf
    }
}

/// A successful match of a [`RePattern`].
#[pyclass(name = "Match", module = "isodate.isotzinfo", frozen)]
pub struct ReMatch {
    re: Py<RePattern>,
    string: Py<PyAny>,
    inner: Match,
    pos: usize,
    endpos: usize,
}

impl ReMatch {
    fn create(
        re: &Bound<'_, RePattern>,
        string: &Bound<'_, PyAny>,
        inner: Match,
        pos: usize,
        endpos: usize,
    ) -> PyResult<Py<Self>> {
        Py::new(
            re.py(),
            Self {
                re: re.clone().unbind(),
                string: string.clone().unbind(),
                inner,
                pos,
                endpos,
            },
        )
    }

    /// `match_getindex`: a group number from an index or a group name.
    fn index(&self, py: Python<'_>, index: &Bound<'_, PyAny>) -> PyResult<usize> {
        let pattern = self.inner.re();
        let number: Option<BigInt> = if index.is_instance_of::<PyInt>() {
            Some(index.extract()?)
        } else if index.get_type().hasattr("__index__")? {
            Some(index.call_method0("__index__")?.extract()?)
        } else if pattern.groupindex().is_empty() {
            None
        } else {
            let names = self.re.get().groupindex_dict(py)?;
            match names.get_item(index)? {
                Some(v) if v.is_instance_of::<PyInt>() => Some(v.extract()?),
                _ => None,
            }
        };
        number
            .and_then(|n| usize::try_from(&n).ok())
            .filter(|&g| g <= pattern.groups())
            .ok_or_else(|| PyIndexError::new_err("no such group"))
    }

    fn group_obj<'py>(
        &self,
        py: Python<'py>,
        g: usize,
        default: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        match self.inner.group_text(g) {
            Some(text) => Ok(cps_to_py(py, &text)?.into_any()),
            None => Ok(default.clone()),
        }
    }

    fn one_group<'py>(
        &self,
        py: Python<'py>,
        index: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let g = self.index(py, index)?;
        self.group_obj(py, g, &py.None().into_bound(py))
    }

    /// `start` / `end` / `span`'s optional positional group argument.
    fn span_arg(
        &self,
        py: Python<'_>,
        name: &str,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<(isize, isize)> {
        no_keywords(&format!("Match.{name}"), kwargs)?;
        let g = match args.len() {
            0 => 0,
            1 => self.index(py, &args.get_item(0)?)?,
            n => {
                return Err(PyTypeError::new_err(format!(
                    "{name} expected at most 1 argument, got {n}"
                )))
            }
        };
        Ok(self.inner.span_of(g))
    }

    fn default_arg<'py>(
        fname: &str,
        args: &Bound<'py, PyTuple>,
        kwargs: Option<&Bound<'py, PyDict>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let bound = clinic(fname, &["default"], 0, args, kwargs)?;
        Ok(bound
            .into_iter()
            .next()
            .flatten()
            .unwrap_or_else(|| args.py().None().into_bound(args.py())))
    }
}

#[pymethods]
impl ReMatch {
    fn __repr__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyString>> {
        cps_to_py(py, &self.inner.repr())
    }

    #[pyo3(signature = (*args, **kwargs))]
    fn group<'py>(
        &self,
        py: Python<'py>,
        args: &Bound<'py, PyTuple>,
        kwargs: Option<&Bound<'py, PyDict>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        no_keywords("Match.group", kwargs)?;
        match args.len() {
            0 => self.group_obj(py, 0, &py.None().into_bound(py)),
            1 => self.one_group(py, &args.get_item(0)?),
            _ => {
                let items = args
                    .iter()
                    .map(|a| self.one_group(py, &a))
                    .collect::<PyResult<Vec<_>>>()?;
                Ok(PyTuple::new(py, items)?.into_any())
            }
        }
    }

    fn __getitem__<'py>(
        &self,
        py: Python<'py>,
        index: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        self.one_group(py, index)
    }

    #[pyo3(signature = (*args, **kwargs))]
    fn groups<'py>(
        &self,
        py: Python<'py>,
        args: &Bound<'py, PyTuple>,
        kwargs: Option<&Bound<'py, PyDict>>,
    ) -> PyResult<Bound<'py, PyTuple>> {
        let default = Self::default_arg("groups", args, kwargs)?;
        let items = (1..=self.inner.re().groups())
            .map(|g| self.group_obj(py, g, &default))
            .collect::<PyResult<Vec<_>>>()?;
        PyTuple::new(py, items)
    }

    #[pyo3(signature = (*args, **kwargs))]
    fn groupdict<'py>(
        &self,
        py: Python<'py>,
        args: &Bound<'py, PyTuple>,
        kwargs: Option<&Bound<'py, PyDict>>,
    ) -> PyResult<Bound<'py, PyDict>> {
        let default = Self::default_arg("groupdict", args, kwargs)?;
        let out = PyDict::new(py);
        for (name, &g) in self.inner.re().groupindex() {
            out.set_item(cps_to_py(py, name)?, self.group_obj(py, g, &default)?)?;
        }
        Ok(out)
    }

    #[pyo3(signature = (*args, **kwargs))]
    fn start(
        &self,
        py: Python<'_>,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<isize> {
        Ok(self.span_arg(py, "start", args, kwargs)?.0)
    }

    #[pyo3(signature = (*args, **kwargs))]
    fn end(
        &self,
        py: Python<'_>,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<isize> {
        Ok(self.span_arg(py, "end", args, kwargs)?.1)
    }

    #[pyo3(signature = (*args, **kwargs))]
    fn span(
        &self,
        py: Python<'_>,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<(isize, isize)> {
        self.span_arg(py, "span", args, kwargs)
    }

    #[pyo3(signature = (*args, **kwargs))]
    fn expand<'py>(
        &self,
        py: Python<'py>,
        args: &Bound<'py, PyTuple>,
        kwargs: Option<&Bound<'py, PyDict>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let bound = clinic("expand", &["template"], 1, args, kwargs)?;
        let Some(Some(source)) = bound.into_iter().next() else {
            return Err(PyTypeError::new_err(
                "expand() missing required argument 'template' (pos 1)",
            ));
        };
        let Ok(source) = source.cast::<PyString>() else {
            return Err(template_type_error(&source)?);
        };
        let template =
            Template::parse(&str_cps(source)?, self.inner.re()).map_err(|e| re_err(py, e))?;
        let string = self.inner.string();
        let mut out = Vec::new();
        template.expand_into(&mut out, |g| {
            let (a, b) = self.inner.span_of(g);
            let a = usize::try_from(a).ok()?;
            let b = usize::try_from(b).ok()?;
            string.get(a..b)
        });
        Ok(cps_to_py(py, &out)?.into_any())
    }

    /// `match.regs`: the span of every group.
    #[getter]
    fn regs<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        let spans: Vec<(isize, isize)> = (0..=self.inner.re().groups())
            .map(|g| self.inner.span_of(g))
            .collect();
        PyTuple::new(py, spans)
    }

    #[getter]
    fn lastindex(&self) -> Option<usize> {
        self.inner.lastindex()
    }

    #[getter]
    fn lastgroup<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyString>>> {
        self.inner
            .lastgroup()
            .map(|name| cps_to_py(py, name))
            .transpose()
    }

    #[getter]
    fn re(&self, py: Python<'_>) -> Py<RePattern> {
        self.re.clone_ref(py)
    }

    #[getter]
    fn string(&self, py: Python<'_>) -> Py<PyAny> {
        self.string.clone_ref(py)
    }

    #[getter]
    fn pos(&self) -> usize {
        self.pos
    }

    #[getter]
    fn endpos(&self) -> usize {
        self.endpos
    }

    fn __copy__(slf: Bound<'_, Self>) -> Bound<'_, Self> {
        slf
    }

    fn __deepcopy__<'py>(slf: Bound<'py, Self>, _memo: &Bound<'py, PyAny>) -> Bound<'py, Self> {
        slf
    }
}

/// Where a scanner continues (`state->start`, `None` once exhausted) and whether its
/// next match must not be empty at that position (`state->must_advance`).
struct ScanState {
    start: Option<usize>,
    must_advance: bool,
}

/// `pattern.scanner(string, pos, endpos)`.
#[pyclass(name = "Scanner", module = "isodate.isotzinfo", frozen)]
pub struct ReScanner {
    re: Py<RePattern>,
    string: Py<PyAny>,
    text: Arc<[u32]>,
    pos: usize,
    endpos: usize,
    state: Mutex<ScanState>,
}

impl ReScanner {
    /// One `scanner.match()` / `scanner.search()` step.
    ///
    /// A step after an empty match must not return an empty match at the same
    /// position (`must_advance`). `finditer` from that position applies exactly that
    /// rule to its second match, so the step takes `finditer`'s first match unless it
    /// is the forbidden empty one.
    fn step(
        &self,
        py: Python<'_>,
        name: &str,
        mode: Mode,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Option<Py<ReMatch>>> {
        if !args.is_empty() || kwargs.is_some_and(|k| !k.is_empty()) {
            return Err(PyTypeError::new_err(format!("{name}() takes no arguments")));
        }
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(start) = state.start else {
            return Ok(None);
        };
        let pattern = &self.re.get().inner;
        let found = if state.must_advance {
            let mut all = scan::finditer(pattern, &self.text, start, self.endpos)
                .map_err(|e| re_err(py, e))?
                .into_iter();
            let first = all.next();
            let first = match first {
                Some(m) if m.span_of(0) == (to_isize(start), to_isize(start)) => all.next(),
                other => other,
            };
            first.filter(|m| mode == Mode::Search || m.span_of(0).0 == to_isize(start))
        } else {
            Match::run(pattern, &self.text, start, self.endpos, mode).map_err(|e| re_err(py, e))?
        };
        let Some(m) = found else {
            state.start = None;
            return Ok(None);
        };
        // `state->start` moved to the match's start, so this is "the match was empty".
        let (b, e) = m.span_of(0);
        let end = usize::try_from(e).unwrap_or(start);
        state.must_advance = b == e;
        state.start = Some(end);
        drop(state);
        ReMatch::create(
            self.re.bind(py),
            self.string.bind(py),
            m,
            self.pos,
            self.endpos,
        )
        .map(Some)
    }
}

fn to_isize(x: usize) -> isize {
    isize::try_from(x).unwrap_or(isize::MAX)
}

#[pymethods]
impl ReScanner {
    #[getter]
    fn pattern(&self, py: Python<'_>) -> Py<RePattern> {
        self.re.clone_ref(py)
    }

    #[pyo3(signature = (*args, **kwargs))]
    fn r#match(
        &self,
        py: Python<'_>,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Option<Py<ReMatch>>> {
        self.step(py, "match", Mode::Match, args, kwargs)
    }

    #[pyo3(signature = (*args, **kwargs))]
    fn search(
        &self,
        py: Python<'_>,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Option<Py<ReMatch>>> {
        self.step(py, "search", Mode::Search, args, kwargs)
    }
}
