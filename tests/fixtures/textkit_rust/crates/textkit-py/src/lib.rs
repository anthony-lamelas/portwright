//! Thin Python layer over textkit-core. Python-specific behavior lives here.
#![forbid(unsafe_code)]

use num_bigint::BigInt;
use num_traits::{Signed, ToPrimitive};
use pyo3::create_exception;
use pyo3::exceptions::{
    PyException, PyOverflowError, PyTypeError, PyValueError, PyZeroDivisionError,
};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyList, PySlice, PyString};
use textkit_core as core_lib;

create_exception!(textkit, ParseError, PyValueError);
create_exception!(textkit, EmptyError, PyException);

const ENCODING: &str = "utf-32-le";
const SURROGATES: &str = "surrogatepass";

/// Read a Python string as code points, keeping any lone surrogates.
fn to_points(text: &Bound<'_, PyAny>) -> PyResult<Vec<u32>> {
    let encoded = text.call_method1("encode", (ENCODING, SURROGATES))?;
    let bytes: &[u8] = encoded.cast::<PyBytes>()?.as_bytes();
    let (chunks, _) = bytes.as_chunks::<4>();
    Ok(chunks.iter().copied().map(u32::from_le_bytes).collect())
}

/// Build a Python string from code points.
fn from_points<'py>(py: Python<'py>, points: &[u32]) -> PyResult<Bound<'py, PyAny>> {
    let bytes: Vec<u8> = points
        .iter()
        .flat_map(|point| point.to_le_bytes())
        .collect();
    PyBytes::new(py, &bytes).call_method1("decode", (ENCODING, SURROGATES))
}

#[pyfunction]
#[pyo3(signature = (text, max_len=None, sep=None))]
fn slug<'py>(
    py: Python<'py>,
    text: &Bound<'py, PyAny>,
    max_len: Option<BigInt>,
    sep: Option<&Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    if !text.is_instance_of::<PyString>() {
        let name = text.get_type().name()?;
        return Err(PyTypeError::new_err(format!(
            "text must be str, got {name}"
        )));
    }
    let sep_points = match sep {
        Some(value) => to_points(value)?,
        None => vec![u32::from(b'-')],
    };
    let result = core_lib::slug(&to_points(text)?, &sep_points);
    match max_len {
        None => from_points(py, &result),
        Some(limit) if limit.is_negative() => {
            Err(PyValueError::new_err("max_len must be non-negative"))
        }
        Some(limit) => from_points(
            py,
            core_lib::rstrip(core_lib::prefix(&result, &limit), &sep_points),
        ),
    }
}

#[pyfunction]
fn truncate<'py>(
    py: Python<'py>,
    text: &Bound<'py, PyAny>,
    n: BigInt,
) -> PyResult<Bound<'py, PyAny>> {
    from_points(py, core_lib::prefix(&to_points(text)?, &n))
}

#[pyfunction]
fn floor_bucket(n: BigInt, size: BigInt) -> PyResult<(BigInt, BigInt)> {
    core_lib::floor_bucket(&n, &size)
        .ok_or_else(|| PyZeroDivisionError::new_err("integer division or modulo by zero"))
}

#[pyfunction]
fn sum_digits(n: BigInt) -> BigInt {
    core_lib::sum_digits(&n)
}

#[pyfunction]
fn word_count<'py>(py: Python<'py>, text: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyDict>> {
    let counts = PyDict::new(py);
    for (word, count) in core_lib::word_count(&to_points(text)?) {
        counts.set_item(from_points(py, &word)?, count)?;
    }
    Ok(counts)
}

#[pyfunction]
fn parse_kv<'py>(
    py: Python<'py>,
    line: &Bound<'py, PyAny>,
) -> PyResult<(Bound<'py, PyAny>, Bound<'py, PyAny>)> {
    let points = to_points(line)?;
    match core_lib::parse_kv(&points) {
        Ok((key, value)) => Ok((from_points(py, key)?, from_points(py, value)?)),
        Err(error) => {
            let message = match error {
                core_lib::ParseKvError::MissingEquals => "missing '='",
                core_lib::ParseKvError::EmptyKey => "empty key",
                _ => "invalid line",
            };
            Err(ParseError::new_err((message, line.clone().unbind())))
        }
    }
}

#[pyfunction]
fn normalize_in_place(py: Python<'_>, values: &Bound<'_, PyList>) -> PyResult<usize> {
    let mut numbers: Vec<BigInt> = values.extract()?;
    let removed = core_lib::normalize(&mut numbers);
    values.call_method1("__setitem__", (PySlice::full(py), numbers))?;
    Ok(removed)
}

/// Reads chunks lazily, so a bad `n` fails on the first read like a generator does.
#[pyclass]
struct FirstChunks {
    values: Py<PyList>,
    n: BigInt,
    start: usize,
    end: Option<usize>,
}

#[pymethods]
impl FirstChunks {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__<'py>(&mut self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyList>>> {
        if !self.n.is_positive() {
            return Err(PyValueError::new_err("n must be positive"));
        }
        let values = self.values.bind(py);
        let end = *self.end.get_or_insert_with(|| values.len());
        if self.start >= end {
            return Ok(None);
        }
        let size = self.n.to_usize().unwrap_or(usize::MAX);
        let stop = self.start.saturating_add(size);
        let chunk = values.get_slice(self.start, stop);
        self.start = stop;
        Ok(Some(chunk))
    }
}

#[pyfunction]
fn first_chunks(values: Bound<'_, PyList>, n: BigInt) -> FirstChunks {
    FirstChunks {
        values: values.unbind(),
        n,
        start: 0,
        end: None,
    }
}

#[pyclass(module = "textkit", name = "Stack")]
struct PyStack {
    inner: core_lib::Stack<Py<PyAny>>,
}

#[pymethods]
impl PyStack {
    #[new]
    #[pyo3(signature = (capacity=None))]
    fn new(capacity: Option<i64>) -> Self {
        Self {
            inner: core_lib::Stack::new(capacity),
        }
    }

    #[getter]
    fn capacity(&self) -> Option<i64> {
        self.inner.capacity()
    }

    fn push(&mut self, item: Py<PyAny>) -> PyResult<()> {
        self.inner
            .push(item)
            .map_err(|_| PyOverflowError::new_err("stack full"))
    }

    fn pop(&mut self) -> PyResult<Py<PyAny>> {
        self.inner
            .pop()
            .map_err(|_| EmptyError::new_err("pop from empty stack"))
    }

    fn peek(&self, py: Python<'_>) -> Option<Py<PyAny>> {
        self.inner.peek().map(|item| item.clone_ref(py))
    }

    fn __len__(&self) -> usize {
        self.inner.len()
    }

    fn __repr__(&self) -> String {
        format!("Stack(size={})", self.inner.len())
    }
}

#[pymodule(name = "textkit")]
fn textkit_module(module: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = module.py();
    module.add("ParseError", py.get_type::<ParseError>())?;
    module.add("EmptyError", py.get_type::<EmptyError>())?;
    module.add_class::<PyStack>()?;
    module.add_function(wrap_pyfunction!(slug, module)?)?;
    module.add_function(wrap_pyfunction!(truncate, module)?)?;
    module.add_function(wrap_pyfunction!(floor_bucket, module)?)?;
    module.add_function(wrap_pyfunction!(sum_digits, module)?)?;
    module.add_function(wrap_pyfunction!(word_count, module)?)?;
    module.add_function(wrap_pyfunction!(parse_kv, module)?)?;
    module.add_function(wrap_pyfunction!(normalize_in_place, module)?)?;
    module.add_function(wrap_pyfunction!(first_chunks, module)?)?;
    Ok(())
}
