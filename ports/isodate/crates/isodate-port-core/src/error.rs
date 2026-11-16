//! The exceptions isodate raises, as plain data. Shared and frozen.
//!
//! The PyO3 layer maps each variant to the exact Python exception (see
//! `isodate-port-py/src/errors.rs`). Messages are [`PyStr`] because several embed
//! user text that may hold lone surrogates (`"%s not a valid time zone info"`).

use std::fmt;

pub use pc_datetime_port_core::{PyError, PyStr};

/// A `decimal` signal raised under the default context (traps: `InvalidOperation`,
/// `DivisionByZero`, `Overflow`).
///
/// CPython's `_decimal` raises `decimal.<signal>([<class 'decimal.<condition>'>])`.
/// Each variant names the condition; [`DecimalSignal::signal`] gives the class raised.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DecimalSignal {
    /// `Decimal("x")`: `InvalidOperation([ConversionSyntax])`.
    ConversionSyntax,
    /// `NaN < 1`, quantize result too long, `int` of sNaN...: `InvalidOperation([InvalidOperation])`.
    InvalidOperation,
    /// `0 / 0`: `InvalidOperation([DivisionUndefined])`.
    DivisionUndefined,
    /// `1 / 0`: `DivisionByZero([DivisionByZero])`.
    DivisionByZero,
    /// Exponent above `Emax`: `Overflow([Overflow])`.
    Overflow,
}

impl DecimalSignal {
    /// Name of the `decimal` exception class that is raised.
    pub fn signal(self) -> &'static str {
        match self {
            Self::ConversionSyntax | Self::InvalidOperation | Self::DivisionUndefined => {
                "InvalidOperation"
            }
            Self::DivisionByZero => "DivisionByZero",
            Self::Overflow => "Overflow",
        }
    }

    /// Name of the `decimal` class listed in the exception's single argument.
    pub fn condition(self) -> &'static str {
        match self {
            Self::ConversionSyntax => "ConversionSyntax",
            Self::InvalidOperation => "InvalidOperation",
            Self::DivisionUndefined => "DivisionUndefined",
            Self::DivisionByZero => "DivisionByZero",
            Self::Overflow => "Overflow",
        }
    }
}

/// An exception raised by isodate.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Error {
    /// `isodate.ISO8601Error(message)` (a `ValueError` subclass).
    Iso8601(PyStr),
    /// A built-in exception, with CPython's exact message. `pc_datetime` returns these
    /// for `date(...)`, `timedelta(...)` and friends, so `?` converts them directly.
    Builtin(PyError),
    /// A `decimal` signal.
    Decimal(DecimalSignal),
}

/// Result of a core function.
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// `ISO8601Error(message)`.
    pub fn iso8601(message: impl Into<PyStr>) -> Self {
        Self::Iso8601(message.into())
    }

    /// `ValueError(message)`.
    pub fn value(message: impl Into<PyStr>) -> Self {
        Self::Builtin(PyError::value_error(message))
    }

    /// `TypeError(message)`.
    pub fn type_error(message: impl Into<PyStr>) -> Self {
        Self::Builtin(PyError::type_error(message))
    }

    /// `OverflowError(message)`.
    pub fn overflow(message: impl Into<PyStr>) -> Self {
        Self::Builtin(PyError::overflow_error(message))
    }

    /// `AttributeError(message)`.
    pub fn attribute(message: impl Into<PyStr>) -> Self {
        Self::Builtin(PyError::attribute_error(message))
    }
}

impl From<PyError> for Error {
    fn from(error: PyError) -> Self {
        Self::Builtin(error)
    }
}

impl From<DecimalSignal> for Error {
    fn from(signal: DecimalSignal) -> Self {
        Self::Decimal(signal)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Iso8601(message) => write!(f, "ISO8601Error: {}", message.to_string_lossy()),
            Self::Builtin(error) => write!(f, "{error}"),
            Self::Decimal(signal) => write!(
                f,
                "decimal.{}: [<class 'decimal.{}'>]",
                signal.signal(),
                signal.condition()
            ),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_signal_names() {
        assert_eq!(
            DecimalSignal::DivisionUndefined.signal(),
            "InvalidOperation"
        );
        assert_eq!(
            DecimalSignal::DivisionUndefined.condition(),
            "DivisionUndefined"
        );
        assert_eq!(DecimalSignal::Overflow.signal(), "Overflow");
    }

    #[test]
    fn builtin_from_pc_datetime() {
        let e: Error = PyError::value_error("year 0 is out of range").into();
        assert_eq!(e, Error::value("year 0 is out of range"));
    }
}
