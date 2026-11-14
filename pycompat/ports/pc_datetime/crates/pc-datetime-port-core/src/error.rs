//! The exception a ported function raises, as plain data.

use std::fmt;

use crate::pystr::PyStr;
use crate::value::Value;

/// A Python exception: its type and the single argument `str(exc)` shows.
///
/// Messages are [`PyStr`] because some embed user text that may hold lone surrogates.
/// `KeyError` carries the missing key itself, because `str(KeyError(k))` is `repr(k)`.
#[derive(Debug, Clone, PartialEq)]
pub enum PyError {
    /// `TypeError(message)`.
    TypeError(PyStr),
    /// `ValueError(message)`.
    ValueError(PyStr),
    /// `OverflowError(message)`.
    OverflowError(PyStr),
    /// `ZeroDivisionError(message)`.
    ZeroDivisionError(PyStr),
    /// `IndexError(message)`.
    IndexError(PyStr),
    /// `KeyError(key)`.
    KeyError(Value),
    /// `RecursionError(message)`.
    RecursionError(PyStr),
    /// `AttributeError(message)`.
    AttributeError(PyStr),
    /// `OSError(errno, strerror)`, as CPython raises from a failing C call.
    OsError {
        /// The C `errno` value.
        errno: i32,
        /// `strerror(errno)`.
        strerror: PyStr,
    },
    /// `re.error(message)`, whose class name is `error`.
    ReError(PyStr),
}

/// Result of a ported function.
pub type PyResult<T> = Result<T, PyError>;

impl PyError {
    /// `TypeError(message)`.
    pub fn type_error(message: impl Into<PyStr>) -> Self {
        Self::TypeError(message.into())
    }
    /// `ValueError(message)`.
    pub fn value_error(message: impl Into<PyStr>) -> Self {
        Self::ValueError(message.into())
    }
    /// `OverflowError(message)`.
    pub fn overflow_error(message: impl Into<PyStr>) -> Self {
        Self::OverflowError(message.into())
    }
    /// `ZeroDivisionError(message)`.
    pub fn zero_division_error(message: impl Into<PyStr>) -> Self {
        Self::ZeroDivisionError(message.into())
    }
    /// `IndexError(message)`.
    pub fn index_error(message: impl Into<PyStr>) -> Self {
        Self::IndexError(message.into())
    }
    /// `AttributeError(message)`.
    pub fn attribute_error(message: impl Into<PyStr>) -> Self {
        Self::AttributeError(message.into())
    }
    /// `OSError(errno, strerror)`.
    pub fn os_error(errno: i32, strerror: impl Into<PyStr>) -> Self {
        Self::OsError {
            errno,
            strerror: strerror.into(),
        }
    }
    /// The Python exception class name, such as `"TypeError"`.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::TypeError(_) => "TypeError",
            Self::ValueError(_) => "ValueError",
            Self::OverflowError(_) => "OverflowError",
            Self::ZeroDivisionError(_) => "ZeroDivisionError",
            Self::IndexError(_) => "IndexError",
            Self::KeyError(_) => "KeyError",
            Self::RecursionError(_) => "RecursionError",
            Self::AttributeError(_) => "AttributeError",
            Self::OsError { .. } => "OSError",
            Self::ReError(_) => "error",
        }
    }
}

impl fmt::Display for PyError {
    /// `type: message`, with lone surrogates replaced. For diagnostics only.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::TypeError(m)
            | Self::ValueError(m)
            | Self::OverflowError(m)
            | Self::ZeroDivisionError(m)
            | Self::IndexError(m)
            | Self::RecursionError(m)
            | Self::AttributeError(m)
            | Self::ReError(m) => m.to_string_lossy(),
            Self::KeyError(key) => key.repr(),
            Self::OsError { errno, strerror } => {
                format!("[Errno {errno}] {}", strerror.to_string_lossy())
            }
        };
        write!(f, "{}: {}", self.type_name(), message)
    }
}

impl std::error::Error for PyError {}
