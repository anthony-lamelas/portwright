//! Errors raised by the port. Each variant maps to one Python exception type;
//! the PyO3 layer only translates, it never builds messages.

use crate::text::{push_str, PyStr};

/// `re.error` raised by the engine (parser, compiler, template parser).
///
/// `args[0]` of the Python exception is [`ReError::message`]; the attributes
/// `msg`, `pattern`, `pos`, `lineno` and `colno` come from the fields and
/// methods here. (`error(...)` called directly from Python with arbitrary
/// argument values is handled separately, see `decisions.md`.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReError {
    /// The bare message (`error.msg`), e.g. `missing ), unterminated subpattern`.
    pub msg: PyStr,
    /// The pattern or template being parsed (`error.pattern`).
    pub pattern: Option<PyStr>,
    /// Code point index into `pattern` (`error.pos`).
    pub pos: Option<usize>,
}

impl ReError {
    /// An error at `pos` in `pattern`.
    pub fn new(msg: PyStr, pattern: &[u32], pos: usize) -> Self {
        Self {
            msg,
            pattern: Some(pattern.to_vec()),
            pos: Some(pos),
        }
    }

    /// An error without location (`error(msg)`).
    pub fn bare(msg: PyStr) -> Self {
        Self {
            msg,
            pattern: None,
            pos: None,
        }
    }

    fn located(&self) -> Option<(&[u32], usize)> {
        match (&self.pattern, self.pos) {
            (Some(p), Some(pos)) => Some((p.as_slice(), pos)),
            _ => None,
        }
    }

    /// `error.lineno`: `pattern.count('\n', 0, pos) + 1`, if located.
    pub fn lineno(&self) -> Option<usize> {
        let (p, pos) = self.located()?;
        let end = pos.min(p.len());
        Some(p[..end].iter().filter(|&&c| c == 0x0A).count() + 1)
    }

    /// `error.colno`: `pos - pattern.rfind('\n', 0, pos)`, if located.
    pub fn colno(&self) -> Option<usize> {
        let (p, pos) = self.located()?;
        let end = pos.min(p.len());
        Some(match p[..end].iter().rposition(|&c| c == 0x0A) {
            Some(i) => pos - i,
            None => pos + 1,
        })
    }

    /// `str(error)` / `error.args[0]`: `msg`, plus ` at position N` and, when
    /// the pattern contains a newline, ` (line L, column C)`.
    pub fn message(&self) -> PyStr {
        let mut out = self.msg.clone();
        if let Some((p, pos)) = self.located() {
            push_str(&mut out, &format!(" at position {pos}"));
            if p.contains(&0x0A) {
                let (l, c) = (self.lineno().unwrap_or(1), self.colno().unwrap_or(1));
                push_str(&mut out, &format!(" (line {l}, column {c})"));
            }
        }
        out
    }
}

/// Every exception the port can raise. Messages are complete and final.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// `re.error` (the `error` class exported by the module).
    Re(ReError),
    /// `TypeError`
    Type(String),
    /// `ValueError`
    Value(String),
    /// `OverflowError`
    Overflow(String),
    /// `IndexError`
    Index(String),
    /// `AttributeError`
    Attribute(String),
    /// `RecursionError`
    Recursion(String),
    /// `MemoryError`
    Memory(String),
}

impl From<ReError> for Error {
    fn from(e: ReError) -> Self {
        Error::Re(e)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Re(e) => f.write_str(&crate::text::to_string_lossy(&e.message())),
            Error::Type(m)
            | Error::Value(m)
            | Error::Overflow(m)
            | Error::Index(m)
            | Error::Attribute(m)
            | Error::Recursion(m)
            | Error::Memory(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for Error {}

/// Result alias for the core.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::{cps, to_string_lossy};

    #[test]
    fn single_line() {
        let e = ReError::new(cps("missing )"), &cps("(a"), 0);
        assert_eq!(to_string_lossy(&e.message()), "missing ) at position 0");
        assert_eq!((e.lineno(), e.colno()), (Some(1), Some(1)));
    }

    #[test]
    fn multi_line() {
        // re.error('m\n', 'ab\ncd', 4) -> 'm\n at position 4 (line 2, column 2)'
        let e = ReError::new(cps("m\n"), &cps("ab\ncd"), 4);
        assert_eq!(
            to_string_lossy(&e.message()),
            "m\n at position 4 (line 2, column 2)"
        );
        // newline after pos still triggers the suffix
        let e = ReError::new(cps("x"), &cps("a\n"), 0);
        assert_eq!(
            to_string_lossy(&e.message()),
            "x at position 0 (line 1, column 1)"
        );
    }

    #[test]
    fn bare() {
        let e = ReError::bare(cps("boom"));
        assert_eq!(to_string_lossy(&e.message()), "boom");
        assert_eq!(e.lineno(), None);
        assert_eq!(Error::from(e).to_string(), "boom");
    }
}
