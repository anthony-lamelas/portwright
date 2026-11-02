//! `template % arg` for a `str` template and a single `str` argument.
//!
//! Upstream builds a field as `wrap % s`, where `wrap` embeds the group name.
//! Bracketed names (`{a[%d]}`) may contain `%`, so Python's printf-style
//! formatting (and its errors) must be reproduced exactly.

use crate::error::{Error, Result};
use crate::pyre::unicode15 as pystr;

/// `ascii(s)` for a `str`.
fn ascii_repr(s: &str) -> String {
    let mut out = String::new();
    for c in pystr::repr(s).chars() {
        let u = u32::from(c);
        if u < 0x80 {
            out.push(c);
        } else if u <= 0xff {
            out.push_str(&format!("\\x{u:02x}"));
        } else if u <= 0xffff {
            out.push_str(&format!("\\u{u:04x}"));
        } else {
            out.push_str(&format!("\\U{u:08x}"));
        }
    }
    out
}

/// CPython's `PY_SSIZE_T_MAX`: a larger width raises `width too big`.
const SSIZE_MAX: usize = isize::MAX.unsigned_abs();
/// CPython's `INT_MAX`: a larger precision raises `precision too big`.
const INT_MAX: usize = i32::MAX as usize;
/// Output size (code points) beyond which CPython would need gigabytes and
/// in practice raises `MemoryError`. The core has no `MemoryError` variant, so
/// this reports `width too big` instead of allocating (see differences/).
const MAX_OUTPUT: usize = 1 << 28;

fn type_error(msg: &str) -> Error {
    Error::Type(msg.to_owned())
}

/// Python `template % arg` where `arg` is a (non-tuple) `str`.
///
/// # Errors
/// The `TypeError`/`ValueError` Python raises for the same operands.
pub fn percent_format(template: &str, arg: &str) -> Result<String> {
    let chars: Vec<char> = template.chars().collect();
    let mut out = String::with_capacity(template.len() + arg.len());
    let mut used = false;
    let mut i = 0;
    let incomplete = || Error::Value("incomplete format".into());
    while let Some(&c) = chars.get(i) {
        if c != '%' {
            out.push(c);
            i += 1;
            continue;
        }
        i += 1;
        let Some(&next) = chars.get(i) else {
            return Err(incomplete());
        };
        if next == '%' {
            out.push('%');
            i += 1;
            continue;
        }
        if next == '(' {
            return Err(type_error("format requires a mapping"));
        }
        let mut left = false;
        while let Some(&f) = chars.get(i) {
            match f {
                '-' => left = true,
                '+' | ' ' | '#' | '0' => {}
                _ => break,
            }
            i += 1;
        }
        let mut width: usize = 0;
        if chars.get(i) == Some(&'*') {
            if used {
                return Err(type_error("not enough arguments for format string"));
            }
            return Err(type_error("* wants int"));
        }
        while let Some(d) = chars.get(i).and_then(|c| c.to_digit(10)) {
            let d = d as usize;
            if width > (SSIZE_MAX - d) / 10 {
                return Err(Error::Value("width too big".into()));
            }
            width = width * 10 + d;
            i += 1;
        }
        let mut precision: Option<usize> = None;
        if chars.get(i) == Some(&'.') {
            i += 1;
            if chars.get(i) == Some(&'*') {
                if used {
                    return Err(type_error("not enough arguments for format string"));
                }
                return Err(type_error("* wants int"));
            }
            let mut p: usize = 0;
            while let Some(d) = chars.get(i).and_then(|c| c.to_digit(10)) {
                let d = d as usize;
                if p > (INT_MAX - d) / 10 {
                    return Err(Error::Value("precision too big".into()));
                }
                p = p * 10 + d;
                i += 1;
            }
            precision = Some(p);
        }
        if matches!(chars.get(i), Some('h' | 'l' | 'L')) {
            i += 1;
        }
        let Some(&conv) = chars.get(i) else {
            return Err(incomplete());
        };
        if conv != '%' {
            if used {
                return Err(type_error("not enough arguments for format string"));
            }
            used = true;
        }
        let text = match conv {
            's' => arg.to_owned(),
            'r' => pystr::repr(arg),
            'a' => ascii_repr(arg),
            'd' | 'i' | 'u' => {
                return Err(Error::Type(format!(
                    "%{conv} format: a real number is required, not str"
                )))
            }
            'o' | 'x' | 'X' => {
                return Err(Error::Type(format!(
                    "%{conv} format: an integer is required, not str"
                )))
            }
            'e' | 'E' | 'f' | 'F' | 'g' | 'G' => {
                return Err(type_error("must be real number, not str"))
            }
            'c' => {
                if arg.chars().count() == 1 {
                    arg.to_owned()
                } else {
                    return Err(type_error("%c requires int or char"));
                }
            }
            other => {
                return Err(Error::Value(format!(
                    "unsupported format character '{other}' (0x{:x}) at index {i}",
                    u32::from(other)
                )))
            }
        };
        let text: String = match (conv, precision) {
            ('s' | 'r' | 'a', Some(p)) => text.chars().take(p).collect(),
            _ => text,
        };
        let len = text.chars().count();
        let pad = width.saturating_sub(len);
        if out.len().saturating_add(pad) > MAX_OUTPUT {
            return Err(Error::Value("width too big".into()));
        }
        let spaces = std::iter::repeat_n(' ', pad);
        if left {
            out.push_str(&text);
            out.extend(spaces);
        } else {
            out.extend(spaces);
            out.push_str(&text);
        }
        i += 1;
    }
    if !used {
        return Err(type_error(
            "not all arguments converted during string formatting",
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_python() {
        assert_eq!(
            percent_format("(?P<a>%s)", "x").ok(),
            Some("(?P<a>x)".into())
        );
        assert_eq!(percent_format("(%5s)", "x").ok(), Some("(    x)".into()));
        assert_eq!(percent_format("%%%s", "x").ok(), Some("%x".into()));
        assert_eq!(
            percent_format("(?P<a_%_>%s)", "x"),
            Err(Error::Value(
                "unsupported format character '_' (0x5f) at index 7".into()
            ))
        );
        assert_eq!(
            percent_format("%s%q", "x"),
            Err(Error::Type("not enough arguments for format string".into()))
        );
        let too_big = |m: &str| Err(Error::Value(m.into()));
        assert_eq!(
            percent_format("%-9223372036854775808s", "x"),
            too_big("width too big")
        );
        assert_eq!(
            percent_format("%.2147483648q", "x"),
            too_big("precision too big")
        );
        assert_eq!(percent_format("%.2147483647s", "x").ok(), Some("x".into()));
        assert_eq!(
            percent_format("%-3333999999r999r+99", "x"),
            too_big("width too big")
        );
        assert_eq!(
            percent_format("ab", "x"),
            Err(Error::Type(
                "not all arguments converted during string formatting".into()
            ))
        );
    }
}
