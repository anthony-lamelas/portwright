//! Shared error type. Every variant maps 1:1 onto a Python exception class in
//! the PyO3 layer (`parse-port-py/src/errors.rs`). The `String` payload is the
//! exception's single argument, byte-identical to what CPython would produce.

use thiserror::Error as ThisError;

/// An error that surfaces in Python as the exception named by the variant.
#[derive(Debug, Clone, PartialEq, Eq, ThisError)]
#[non_exhaustive]
pub enum Error {
    /// `ValueError(msg)`
    #[error("{0}")]
    Value(String),
    /// `TypeError(msg)`
    #[error("{0}")]
    Type(String),
    /// `KeyError(arg)`: the payload is the *argument* (e.g. the missing key
    /// string), not its repr. Python's `str(KeyError)` adds the repr itself.
    #[error("{0}")]
    Key(String),
    /// `IndexError(msg)`
    #[error("{0}")]
    Index(String),
    /// `OverflowError(msg)`
    #[error("{0}")]
    Overflow(String),
    /// `AttributeError(msg)`
    #[error("{0}")]
    Attribute(String),
    /// `ZeroDivisionError(msg)`
    #[error("{0}")]
    ZeroDivision(String),
    /// `NotImplementedError(msg)`
    #[error("{0}")]
    NotImplemented(String),
    /// `parse.RepeatedNameError(msg)`
    #[error("{0}")]
    RepeatedName(String),
    /// `parse.TooManyFields(msg)`
    #[error("{0}")]
    TooManyFields(String),
    /// `re.error`, raised by the sre-compatible engine.
    #[error(transparent)]
    Regex(#[from] RegexError),
}

/// Mirror of Python's `re.error`: `msg`, `pattern`, `pos` are the raw
/// constructor arguments; [`RegexError::message`] is `str(exc)` / `args[0]`.
#[derive(Debug, Clone, PartialEq, Eq, ThisError)]
#[error("{}", self.message())]
pub struct RegexError {
    /// Bare message, e.g. `missing ), unterminated subpattern`.
    pub msg: String,
    /// The full pattern being compiled.
    pub pattern: String,
    /// Position in **code points** into `pattern`, if known.
    pub pos: Option<usize>,
}

impl RegexError {
    /// Build an error at a code-point position.
    pub fn new(msg: impl Into<String>, pattern: impl Into<String>, pos: Option<usize>) -> Self {
        Self {
            msg: msg.into(),
            pattern: pattern.into(),
            pos,
        }
    }

    /// 1-based line of `pos` (`re.error.lineno`), if `pos` is known.
    pub fn lineno(&self) -> Option<usize> {
        let pos = self.pos?;
        Some(
            self.pattern
                .chars()
                .take(pos)
                .filter(|&c| c == '\n')
                .count()
                + 1,
        )
    }

    /// 1-based column of `pos` (`re.error.colno`), if `pos` is known.
    pub fn colno(&self) -> Option<usize> {
        let pos = self.pos?;
        let prefix: Vec<char> = self.pattern.chars().take(pos).collect();
        let last_nl = prefix.iter().rposition(|&c| c == '\n');
        Some(match last_nl {
            Some(i) => pos - i,
            None => pos + 1,
        })
    }

    /// The formatted message exactly as CPython's `re.error.__init__` builds it.
    pub fn message(&self) -> String {
        match self.pos {
            None => self.msg.clone(),
            Some(pos) => {
                let mut out = format!("{} at position {}", self.msg, pos);
                if self.pattern.contains('\n') {
                    out = format!(
                        "{} (line {}, column {})",
                        out,
                        self.lineno().unwrap_or(1),
                        self.colno().unwrap_or(1)
                    );
                }
                out
            }
        }
    }
}

/// Convenience alias used throughout the core crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regex_error_message_matches_python() {
        let e = RegexError::new("missing ), unterminated subpattern", "(", Some(0));
        assert_eq!(
            e.message(),
            "missing ), unterminated subpattern at position 0"
        );
        let e = RegexError::new("x", "ab\ncd", Some(4));
        assert_eq!(e.message(), "x at position 4 (line 2, column 2)");
    }
}
