//! The `re.Match`-like object our compiled patterns produce.

use std::sync::Arc;

use parse_port_core::pyre::unicode15 as pystr;
use parse_port_core::pyre::{Captures, Regex};
use pyo3::exceptions::PyIndexError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyInt, PyList, PySlice, PyString, PyTuple};

use super::{py_text, str_text};

/// The `re.Match`-like object produced by our compiled patterns.
#[pyclass(module = "re", name = "Match", frozen)]
pub struct ReMatch {
    pub(super) string: Py<PyAny>,
    pub(super) text: Arc<[char]>,
    pub(super) caps: Captures,
    pub(super) regex: Arc<Regex>,
    pub(super) pos: usize,
    pub(super) endpos: usize,
}

impl ReMatch {
    /// Group `i`'s text, or `default` when it didn't take part.
    fn group_or(&self, py: Python<'_>, i: usize, default: &Py<PyAny>) -> PyResult<Py<PyAny>> {
        let (Some(s), Some((start, end))) = (self.caps.text(&self.text, i), self.caps.span(i))
        else {
            return Ok(default.clone_ref(py));
        };
        if !s.chars().any(|c| pystr::standin_surrogate(c).is_some()) {
            return Ok(PyString::new(py, &s).into_any().unbind());
        }
        // Lone surrogates: slice the original `str`, which keeps them (and
        // any real stand-in code points) exactly.
        let slice = PySlice::new(py, isize::try_from(start)?, isize::try_from(end)?, 1);
        Ok(py
            .get_type::<PyString>()
            .call_method1("__getitem__", (self.string.bind(py), slice))?
            .unbind())
    }

    /// The matched `str` as UTF-8 when it is ASCII, so that code-point
    /// spans are also byte spans into it.
    pub(super) fn ascii_text<'a>(&'a self, py: Python<'a>) -> Option<&'a str> {
        let s = self
            .string
            .bind(py)
            .cast::<PyString>()
            .ok()?
            .to_str()
            .ok()?;
        (s.is_ascii() && s.len() == self.text.len()).then_some(s)
    }

    pub(super) fn group_text(&self, py: Python<'_>, i: usize) -> PyResult<Py<PyAny>> {
        self.group_or(py, i, &py.None())
    }

    fn resolve(&self, g: &Bound<'_, PyAny>) -> PyResult<usize> {
        let no_such = || PyIndexError::new_err("no such group");
        if let Ok(s) = g.cast::<PyString>() {
            return self
                .regex
                .groupindex()
                .get(str_text(s)?.as_str())
                .copied()
                .ok_or_else(no_such);
        }
        if g.is_instance_of::<PyInt>() || g.hasattr("__index__")? {
            let i: i64 = g.extract().map_err(|_| no_such())?;
            return usize::try_from(i)
                .ok()
                .filter(|&i| i <= self.regex.groups())
                .ok_or_else(no_such);
        }
        Err(no_such())
    }

    pub(super) fn span_index(&self, i: i64) -> PyResult<(isize, isize)> {
        let idx = usize::try_from(i)
            .ok()
            .filter(|&i| i <= self.regex.groups())
            .ok_or_else(|| PyIndexError::new_err("no such group"))?;
        Ok(self.span_of(idx))
    }

    pub(super) fn span_of(&self, i: usize) -> (isize, isize) {
        match self.caps.span(i) {
            Some((s, e)) => (
                isize::try_from(s).unwrap_or(isize::MAX),
                isize::try_from(e).unwrap_or(isize::MAX),
            ),
            None => (-1, -1),
        }
    }

    pub(super) fn group_list<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        let items: Vec<Py<PyAny>> = (1..=self.regex.groups())
            .map(|i| self.group_text(py, i))
            .collect::<PyResult<_>>()?;
        PyList::new(py, items)
    }
}

#[pymethods]
impl ReMatch {
    #[pyo3(signature = (*args))]
    fn group(&self, py: Python<'_>, args: &Bound<'_, PyTuple>) -> PyResult<Py<PyAny>> {
        match args.len() {
            0 => self.group_text(py, 0),
            1 => self.group_text(py, self.resolve(&args.get_item(0)?)?),
            _ => {
                let mut items = Vec::with_capacity(args.len());
                for a in args.iter() {
                    items.push(self.group_text(py, self.resolve(&a)?)?);
                }
                Ok(PyTuple::new(py, items)?.into_any().unbind())
            }
        }
    }

    fn __getitem__(&self, py: Python<'_>, g: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        self.group_text(py, self.resolve(g)?)
    }

    #[pyo3(signature = (default = None))]
    fn groups<'py>(
        &self,
        py: Python<'py>,
        default: Option<Py<PyAny>>,
    ) -> PyResult<Bound<'py, PyTuple>> {
        let default = default.unwrap_or_else(|| py.None());
        let items = (1..=self.regex.groups())
            .map(|i| self.group_or(py, i, &default))
            .collect::<PyResult<Vec<_>>>()?;
        PyTuple::new(py, items)
    }

    #[pyo3(signature = (default = None))]
    fn groupdict<'py>(
        &self,
        py: Python<'py>,
        default: Option<Py<PyAny>>,
    ) -> PyResult<Bound<'py, PyDict>> {
        let default = default.unwrap_or_else(|| py.None());
        let d = PyDict::new(py);
        for (name, &i) in self.regex.groupindex() {
            d.set_item(py_text(py, name)?, self.group_or(py, i, &default)?)?;
        }
        Ok(d)
    }

    #[pyo3(signature = (group = None))]
    fn span(&self, group: Option<&Bound<'_, PyAny>>) -> PyResult<(isize, isize)> {
        let i = match group {
            Some(g) => self.resolve(g)?,
            None => 0,
        };
        Ok(self.span_of(i))
    }

    #[pyo3(signature = (group = None))]
    fn start(&self, group: Option<&Bound<'_, PyAny>>) -> PyResult<isize> {
        Ok(self.span(group)?.0)
    }

    #[pyo3(signature = (group = None))]
    fn end(&self, group: Option<&Bound<'_, PyAny>>) -> PyResult<isize> {
        Ok(self.span(group)?.1)
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

    #[getter]
    fn lastindex(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for i in 1..=self.regex.groups() {
            if let Some((_, e)) = self.caps.span(i) {
                if best.is_none_or(|(_, be)| e >= be) {
                    best = Some((i, e));
                }
            }
        }
        best.map(|(i, _)| i)
    }

    #[getter]
    fn lastgroup<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyString>>> {
        let Some(li) = self.lastindex() else {
            return Ok(None);
        };
        self.regex
            .groupindex()
            .iter()
            .find(|(_, &i)| i == li)
            .map(|(n, _)| py_text(py, n))
            .transpose()
    }

    fn __repr__(&self) -> String {
        let (s, e) = self.span_of(0);
        let m = self.caps.text(&self.text, 0).unwrap_or_default();
        format!(
            "<re.Match object; span=({s}, {e}), match={}>",
            pystr::repr(&m)
        )
    }
}
