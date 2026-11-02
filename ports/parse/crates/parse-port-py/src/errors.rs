//! Exception classes and the single mapping from core errors to Python.
//! Frozen shared file.
#![allow(non_camel_case_types, reason = "Python class re.error is lowercase")]

use parse_port_core::{Error, RegexError};
use pyo3::exceptions::{
    PyAttributeError, PyException, PyIndexError, PyKeyError, PyNotImplementedError,
    PyOverflowError, PyTypeError, PyValueError, PyZeroDivisionError,
};
use pyo3::prelude::*;

pyo3::create_exception!(parse, TooManyFields, PyValueError);
pyo3::create_exception!(parse, RepeatedNameError, PyValueError);

/// Stand-in for Python's `re.error` (qualname `error`, module `re`, base
/// `Exception`). The PyO3 layer may not import `re`, so the class is
/// recreated; instances carry `msg`, `pattern`, `pos`, `lineno`, `colno`.
pub mod re {
    pyo3::create_exception!(re, error, pyo3::exceptions::PyException);
}

/// Build a Python `re.error` instance from a core [`RegexError`].
pub fn regex_error(py: Python<'_>, e: &RegexError) -> PyErr {
    let err = re::error::new_err(e.message());
    let value = err.value(py);
    let _ = value.setattr("msg", &e.msg);
    let _ = value.setattr("pattern", &e.pattern);
    let _ = value.setattr("pos", e.pos);
    let _ = value.setattr("lineno", e.lineno());
    let _ = value.setattr("colno", e.colno());
    err
}

/// Convert any core error into the matching Python exception.
pub fn to_pyerr(py: Python<'_>, e: Error) -> PyErr {
    match e {
        Error::Value(m) => PyValueError::new_err(m),
        Error::Type(m) => PyTypeError::new_err(m),
        Error::Key(m) => PyKeyError::new_err(m),
        Error::Index(m) => PyIndexError::new_err(m),
        Error::Overflow(m) => PyOverflowError::new_err(m),
        Error::Attribute(m) => PyAttributeError::new_err(m),
        Error::ZeroDivision(m) => PyZeroDivisionError::new_err(m),
        Error::NotImplemented(m) => PyNotImplementedError::new_err(m),
        Error::RepeatedName(m) => RepeatedNameError::new_err(m),
        Error::TooManyFields(m) => TooManyFields::new_err(m),
        Error::Regex(r) => regex_error(py, &r),
        other => PyException::new_err(other.to_string()),
    }
}

/// Extension for `Result<T, core::Error>` → `PyResult<T>`.
pub trait IntoPyResult<T> {
    /// Map the core error with [`to_pyerr`].
    fn py(self, py: Python<'_>) -> PyResult<T>;
}

impl<T> IntoPyResult<T> for Result<T, Error> {
    fn py(self, py: Python<'_>) -> PyResult<T> {
        self.map_err(|e| to_pyerr(py, e))
    }
}

/// Registers `TooManyFields` and `RepeatedNameError` on the module.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    m.add("TooManyFields", py.get_type::<TooManyFields>())?;
    m.add("RepeatedNameError", py.get_type::<RepeatedNameError>())?;
    Ok(())
}
