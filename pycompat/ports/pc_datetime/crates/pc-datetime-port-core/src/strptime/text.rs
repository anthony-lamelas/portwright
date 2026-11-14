//! Python's Unicode `\d`, `\s` and `int()` for the strings `_strptime` handles.

use std::fmt::Write as _;

use crate::error::{PyError, PyResult};
use crate::pystr::PyStr;

/// The code point of digit zero for every run of ten Unicode `Nd` digits (Unicode 15.0,
/// as in CPython 3.12). Python's `\d` and `int()` accept exactly these.
const DIGIT_ZEROS: [u32; 68] = [
    0x30, 0x660, 0x6f0, 0x7c0, 0x966, 0x9e6, 0xa66, 0xae6, 0xb66, 0xbe6, 0xc66, 0xce6, 0xd66,
    0xde6, 0xe50, 0xed0, 0xf20, 0x1040, 0x1090, 0x17e0, 0x1810, 0x1946, 0x19d0, 0x1a80, 0x1a90,
    0x1b50, 0x1bb0, 0x1c40, 0x1c50, 0xa620, 0xa8d0, 0xa900, 0xa9d0, 0xa9f0, 0xaa50, 0xabf0, 0xff10,
    0x104a0, 0x10d30, 0x11066, 0x110f0, 0x11136, 0x111d0, 0x112f0, 0x11450, 0x114d0, 0x11650,
    0x116c0, 0x11730, 0x118e0, 0x11950, 0x11c50, 0x11d50, 0x11da0, 0x11f50, 0x16a60, 0x16ac0,
    0x16b50, 0x1d7ce, 0x1d7d8, 0x1d7e2, 0x1d7ec, 0x1d7f6, 0x1e140, 0x1e2f0, 0x1e4f0, 0x1e950,
    0x1fbf0,
];

/// Ranges of code points for which `str.isspace()` (and `re`'s `\s`) is true.
const SPACE_RANGES: [(u32, u32); 10] = [
    (0x09, 0x0d),
    (0x1c, 0x20),
    (0x85, 0x85),
    (0xa0, 0xa0),
    (0x1680, 0x1680),
    (0x2000, 0x200a),
    (0x2028, 0x2029),
    (0x202f, 0x202f),
    (0x205f, 0x205f),
    (0x3000, 0x3000),
];

/// The decimal value of a Unicode digit, or None.
pub fn digit_value(c: u32) -> Option<u32> {
    DIGIT_ZEROS
        .iter()
        .find(|&&zero| (zero..zero + 10).contains(&c))
        .map(|zero| c - zero)
}

/// `c.isspace()`.
pub fn is_space(c: u32) -> bool {
    SPACE_RANGES.iter().any(|&(lo, hi)| (lo..=hi).contains(&c))
}

/// A regex-crate class equal to Python's `\d` in a `str` pattern.
pub fn digit_class() -> String {
    let mut class = String::from("[");
    for zero in DIGIT_ZEROS {
        let _ = write!(class, "\\x{{{zero:x}}}-\\x{{{:x}}}", zero + 9);
    }
    class.push(']');
    class
}

/// A regex-crate class equal to Python's `\s` in a `str` pattern.
pub fn space_class() -> String {
    let mut class = String::from("[");
    for (lo, hi) in SPACE_RANGES {
        let _ = write!(class, "\\x{{{lo:x}}}-\\x{{{hi:x}}}");
    }
    class.push(']');
    class
}

/// `int(s)` for a base-10 str: surrounding whitespace, one sign, Unicode digits and
/// single underscores between digits.
///
/// # Errors
/// `ValueError("invalid literal for int() with base 10: 's'")`, or an OverflowError
/// for values beyond i64 (never reached by `_strptime`'s short groups).
pub fn py_int(s: &[u32]) -> PyResult<i64> {
    let invalid = || {
        PyError::value_error(format!(
            "invalid literal for int() with base 10: {}",
            PyStr::from_code_points(s.to_vec()).repr()
        ))
    };
    let start = s.iter().position(|&c| !is_space(c)).unwrap_or(s.len());
    let end = s
        .iter()
        .rposition(|&c| !is_space(c))
        .map_or(start, |i| i + 1);
    let mut body = &s[start..end];
    let negative = match body.first() {
        Some(&c) if c == u32::from('-') || c == u32::from('+') => {
            body = &body[1..];
            c == u32::from('-')
        }
        _ => false,
    };
    if body.is_empty() {
        return Err(invalid());
    }
    let mut value: i64 = 0;
    let mut previous_underscore = true;
    for &c in body {
        if c == u32::from('_') {
            if previous_underscore {
                return Err(invalid());
            }
            previous_underscore = true;
            continue;
        }
        let digit = digit_value(c).ok_or_else(invalid)?;
        previous_underscore = false;
        value = value
            .checked_mul(10)
            .and_then(|v| v.checked_add(i64::from(digit)))
            .ok_or_else(|| PyError::overflow_error("int too large"))?;
    }
    if previous_underscore {
        return Err(invalid());
    }
    Ok(if negative { -value } else { value })
}

/// Encodes code points as generalized UTF-8: lone surrogates become their 3-byte form,
/// which no Unicode-aware regex item can match.
pub fn to_wtf8(points: &[u32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(points.len());
    for &c in points {
        match u8::try_from(c) {
            Ok(b) if b.is_ascii() => out.push(b),
            _ => push_wtf8(&mut out, c),
        }
    }
    out
}

/// Appends one code point in generalized UTF-8.
pub fn push_wtf8(out: &mut Vec<u8>, c: u32) {
    if let Some(ch) = char::from_u32(c) {
        let mut buf = [0u8; 4];
        out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
    } else {
        // Only surrogates (0xD800..=0xDFFF) reach here.
        out.extend_from_slice(&[
            0xe0 | ((c >> 12) & 0x0f) as u8,
            0x80 | ((c >> 6) & 0x3f) as u8,
            0x80 | (c & 0x3f) as u8,
        ]);
    }
}

/// The number of code points in a generalized UTF-8 byte string.
pub fn wtf8_len(bytes: &[u8]) -> usize {
    bytes.iter().filter(|&&b| b & 0xc0 != 0x80).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn points(s: &str) -> Vec<u32> {
        s.chars().map(u32::from).collect()
    }

    #[test]
    fn int_parsing() {
        assert_eq!(py_int(&points(" 5")), Ok(5));
        assert_eq!(py_int(&points("١٢")), Ok(12));
        assert_eq!(py_int(&points("1_0")), Ok(10));
        assert!(py_int(&points(":1")).is_err());
        assert!(py_int(&points("")).is_err());
    }
}
