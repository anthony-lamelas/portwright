//! Python `str`: a sequence of code points that may include lone surrogates.

use std::fmt::Write as _;

use crate::printable::NON_PRINTABLE;

/// A Python string. Unlike [`String`], it can hold lone surrogates (`'\ud800'`),
/// which Python strings allow and which this library echoes back in results and errors.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub struct PyStr(Vec<u32>);

impl PyStr {
    /// Builds a string from code points. Each must be at most `0x10FFFF`.
    ///
    /// # Panics
    /// Panics if a code point is above `0x10FFFF`.
    pub fn from_code_points(points: Vec<u32>) -> Self {
        assert!(
            points.iter().all(|&c| c <= 0x10_FFFF),
            "code point out of range"
        );
        Self(points)
    }

    /// The code points, as Python indexes them.
    pub fn code_points(&self) -> &[u32] {
        &self.0
    }

    /// `len(s)`: the number of code points.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// `not s`.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The text as a Rust string, or `None` if it holds a lone surrogate.
    pub fn to_str(&self) -> Option<String> {
        self.0.iter().map(|&c| char::from_u32(c)).collect()
    }

    /// The text with each lone surrogate replaced by U+FFFD. Lossy: use for diagnostics.
    pub fn to_string_lossy(&self) -> String {
        self.0
            .iter()
            .map(|&c| char::from_u32(c).unwrap_or('\u{FFFD}'))
            .collect()
    }

    /// Appends another string.
    pub fn push_str(&mut self, other: &PyStr) {
        self.0.extend_from_slice(&other.0);
    }

    /// Appends one code point (which may be a surrogate).
    ///
    /// # Panics
    /// Panics if `point` is above `0x10FFFF`.
    pub fn push_code_point(&mut self, point: u32) {
        assert!(point <= 0x10_FFFF, "code point out of range");
        self.0.push(point);
    }

    /// `repr(s)`, exactly as CPython 3.12 writes it.
    pub fn repr(&self) -> String {
        let quote = if self.0.contains(&u32::from('\'')) && !self.0.contains(&u32::from('"')) {
            '"'
        } else {
            '\''
        };
        let mut out = String::with_capacity(self.0.len() + 2);
        out.push(quote);
        for &c in &self.0 {
            match c {
                0x5C => out.push_str("\\\\"),
                0x09 => out.push_str("\\t"),
                0x0A => out.push_str("\\n"),
                0x0D => out.push_str("\\r"),
                _ if c == u32::from(quote) => {
                    out.push('\\');
                    out.push(quote);
                }
                _ if c < 0x20 || c == 0x7F => {
                    let _ = write!(out, "\\x{c:02x}");
                }
                _ if c < 0x7F => out.push(char::from_u32(c).unwrap_or('?')),
                _ if is_printable(c) => out.push(char::from_u32(c).unwrap_or('?')),
                _ if c <= 0xFF => {
                    let _ = write!(out, "\\x{c:02x}");
                }
                _ if c <= 0xFFFF => {
                    let _ = write!(out, "\\u{c:04x}");
                }
                _ => {
                    let _ = write!(out, "\\U{c:08x}");
                }
            }
        }
        out.push(quote);
        out
    }
}

/// `chr(c).isprintable()` in CPython 3.12. Surrogates aren't printable.
pub fn is_printable(c: u32) -> bool {
    let idx = NON_PRINTABLE.partition_point(|&(_, hi)| hi < c);
    !NON_PRINTABLE.get(idx).is_some_and(|&(lo, _)| lo <= c)
}

impl PartialEq<str> for PyStr {
    fn eq(&self, other: &str) -> bool {
        self.0.iter().copied().eq(other.chars().map(u32::from))
    }
}

impl PartialEq<&str> for PyStr {
    fn eq(&self, other: &&str) -> bool {
        *self == **other
    }
}

impl From<&str> for PyStr {
    fn from(s: &str) -> Self {
        Self(s.chars().map(u32::from).collect())
    }
}

impl From<String> for PyStr {
    fn from(s: String) -> Self {
        Self::from(s.as_str())
    }
}

impl From<&String> for PyStr {
    fn from(s: &String) -> Self {
        Self::from(s.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repr_matches_cpython() {
        assert_eq!(PyStr::from("abc").repr(), "'abc'");
        assert_eq!(PyStr::from("a'b").repr(), "\"a'b\"");
        assert_eq!(PyStr::from("a'\"b").repr(), "'a\\'\"b'");
        assert_eq!(
            PyStr::from("\u{0}\t\u{7f}\u{a0}é日").repr(),
            "'\\x00\\t\\x7f\\xa0é日'"
        );
        assert_eq!(PyStr::from_code_points(vec![0xD800]).repr(), "'\\ud800'");
        assert_eq!(PyStr::from("\u{E0000}").repr(), "'\\U000e0000'");
        assert_eq!(PyStr::from("\u{3000}").repr(), "'\\u3000'");
    }
}
