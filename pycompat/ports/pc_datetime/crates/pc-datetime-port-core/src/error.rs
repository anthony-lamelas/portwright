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
    /// `re.error(msg, pattern, pos)`, whose class name is `error`. `str()` of it, and
    /// its `lineno` and `colno`, are derived as `re.error.__init__` derives them.
    ReError {
        /// `error.msg`, without the position.
        msg: PyStr,
        /// `error.pattern`, the regular expression that failed.
        pattern: PyStr,
        /// `error.pos`, an index into `pattern`.
        pos: usize,
    },
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
    /// `re.error(msg, pattern, pos)`.
    pub fn re_error(msg: impl Into<PyStr>, pattern: impl Into<PyStr>, pos: usize) -> Self {
        Self::ReError {
            msg: msg.into(),
            pattern: pattern.into(),
            pos,
        }
    }

    /// `re.error`'s `(lineno, colno)`: the line of `pos` in the pattern, from 1, and
    /// `pos` minus the index of the newline before it (-1 when there is none).
    #[must_use]
    pub fn re_location(pattern: &PyStr, pos: usize) -> (usize, usize) {
        let newline = u32::from('\n');
        let before = &pattern.code_points()[..pos.min(pattern.len())];
        let lineno = before.iter().filter(|&&c| c == newline).count() + 1;
        let colno = before
            .iter()
            .rposition(|&c| c == newline)
            .map_or(pos + 1, |i| pos - i);
        (lineno, colno)
    }

    /// `str(re.error(msg, pattern, pos))`: the message, its position, and the line and
    /// column when the pattern has more than one line.
    #[must_use]
    pub fn re_text(msg: &PyStr, pattern: &PyStr, pos: usize) -> PyStr {
        let mut text = msg.clone();
        text.push_str(&PyStr::from(format!(" at position {pos}").as_str()));
        if pattern.code_points().contains(&u32::from('\n')) {
            let (lineno, colno) = Self::re_location(pattern, pos);
            text.push_str(&PyStr::from(
                format!(" (line {lineno}, column {colno})").as_str(),
            ));
        }
        text
    }

    /// `repr(re.error(...))`: `error('<str() of it>')`.
    #[must_use]
    pub fn re_repr(text: &PyStr) -> PyStr {
        PyStr::from(format!("error({})", text.repr()))
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
            Self::ReError { .. } => "error",
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
            | Self::AttributeError(m) => m.to_string_lossy(),
            Self::ReError { msg, pattern, pos } => {
                Self::re_text(msg, pattern, *pos).to_string_lossy()
            }
            Self::KeyError(key) => key.repr(),
            Self::OsError { errno, strerror } => {
                format!("[Errno {errno}] {}", strerror.to_string_lossy())
            }
        };
        write!(f, "{}: {}", self.type_name(), message)
    }
}

impl std::error::Error for PyError {}
