//! `isodate.ISO8601Error` and the mapping from core errors to Python exceptions.
//! Shared and frozen.

use isodate_port_core::{DecimalSignal, Error, PyError};
use pyo3::exceptions::{
    PyAttributeError, PyIndexError, PyKeyError, PyOSError, PyOverflowError, PyRecursionError,
    PyTypeError, PyValueError, PyZeroDivisionError,
};
use pyo3::prelude::*;
use pyo3::types::PyList;

use crate::text::cps_to_py;
use crate::Modules;

pyo3::create_exception!(
    isodate.isoerror,
    ISO8601Error,
    PyValueError,
    "Raised when the given ISO string can not be parsed."
);

pub(crate) fn register(modules: &Modules<'_>) -> PyResult<()> {
    let py = modules.root.py();
    modules
        .isoerror
        .add("ISO8601Error", py.get_type::<ISO8601Error>())
}

/// Raises `decimal.<signal>([<class 'decimal.<condition>'>])`, as CPython's `_decimal` does.
pub fn decimal_error(py: Python<'_>, signal: DecimalSignal) -> PyErr {
    let build = || -> PyResult<PyErr> {
        let decimal = py.import("decimal")?;
        let class = decimal.getattr(signal.signal())?;
        let condition = decimal.getattr(signal.condition())?;
        let args = PyList::new(py, [condition])?;
        Ok(PyErr::from_value(class.call1((args,))?))
    };
    build().unwrap_or_else(|error| error)
}

/// Converts a `pc_datetime` built-in exception.
pub fn builtin_error(py: Python<'_>, error: PyError) -> PyErr {
    let message = |text: &isodate_port_core::PyStr| -> PyResult<Py<PyAny>> {
        Ok(cps_to_py(py, text.code_points())?.into_any().unbind())
    };
    let build = || -> PyResult<PyErr> {
        Ok(match &error {
            PyError::TypeError(m) => PyTypeError::new_err(message(m)?),
            PyError::ValueError(m) => PyValueError::new_err(message(m)?),
            PyError::OverflowError(m) => PyOverflowError::new_err(message(m)?),
            PyError::ZeroDivisionError(m) => PyZeroDivisionError::new_err(message(m)?),
            PyError::IndexError(m) => PyIndexError::new_err(message(m)?),
            PyError::RecursionError(m) => PyRecursionError::new_err(message(m)?),
            PyError::AttributeError(m) => PyAttributeError::new_err(message(m)?),
            PyError::OsError { errno, strerror } => {
                PyOSError::new_err((*errno, message(strerror)?))
            }
            // isodate never looks up keys or compiles user patterns; keep the text.
            PyError::KeyError(_) | PyError::ReError(_) => PyKeyError::new_err(error.to_string()),
        })
    };
    build().unwrap_or_else(|e| e)
}

/// Converts any core error to the Python exception isodate raises.
pub fn to_py_err(py: Python<'_>, error: Error) -> PyErr {
    match error {
        Error::Iso8601(message) => match cps_to_py(py, message.code_points()) {
            Ok(text) => ISO8601Error::new_err(text.unbind()),
            Err(e) => e,
        },
        Error::Builtin(error) => builtin_error(py, error),
        Error::Decimal(signal) => decimal_error(py, signal),
        _ => PyValueError::new_err(error.to_string()),
    }
}

/// `result.map_err(|e| to_py_err(py, e))`, shortened.
pub trait IntoPyResult<T> {
    /// Converts the core error, if any.
    fn py_err(self, py: Python<'_>) -> PyResult<T>;
}

impl<T> IntoPyResult<T> for isodate_port_core::Result<T> {
    fn py_err(self, py: Python<'_>) -> PyResult<T> {
        self.map_err(|e| to_py_err(py, e))
    }
}
