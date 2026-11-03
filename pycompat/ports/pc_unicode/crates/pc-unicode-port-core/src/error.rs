//! Errors raised by the port, mirroring the Python exception that CPython raises.

use std::fmt;

/// Which Python exception an [`Error`] becomes at the PyO3 boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    /// `TypeError`.
    Type,
    /// `ValueError`.
    Value,
    /// `KeyError` (the message is the single exception argument).
    Key,
    /// `OverflowError`.
    Overflow,
    /// `MemoryError` (message is usually empty).
    Memory,
    /// `RecursionError`.
    Recursion,
}

/// A Python exception: its kind and the exact `str(exc)` CPython would produce
/// (for `KeyError`, the argument whose repr CPython shows).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    /// Exception class.
    pub kind: ErrorKind,
    /// Exception argument, byte-for-byte as CPython builds it.
    pub message: String,
}

impl Error {
    /// Builds an error of `kind` with `message`.
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// `TypeError(message)`.
    pub fn type_error(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Type, message)
    }

    /// `ValueError(message)`.
    pub fn value_error(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Value, message)
    }

    /// `KeyError(message)`.
    pub fn key_error(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Key, message)
    }

    /// `OverflowError(message)`.
    pub fn overflow_error(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Overflow, message)
    }

    /// `MemoryError()`.
    pub fn memory_error() -> Self {
        Self::new(ErrorKind::Memory, "")
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

impl std::error::Error for Error {}

/// Result alias used across the core.
pub type Result<T> = std::result::Result<T, Error>;
