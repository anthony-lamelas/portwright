//! Python's `repr()`, `str()` and `ascii()` of any [`Value`], used by
//! `format(value, "")`, `%r`/`%s`/`%a` and `!r`/`!s`/`!a`.

use crate::numbers::{bytearray_repr, bytes_repr, float_repr, int_str, str_repr_code_points};
use crate::value::{PcResult, PyStr, Value};

/// Python's `repr()` of a `set` whose items are given in iteration order.
///
/// # Errors
/// `ValueError` when an int inside has more than [`MAX_STR_DIGITS`] digits.
pub fn set_repr(items: &[Value]) -> PcResult<String> {
    if items.is_empty() {
        return Ok("set()".to_owned());
    }
    Ok(format!("{{{}}}", join_reprs(items)?))
}

/// Python's `repr()` of a `frozenset` whose items are given in iteration
/// order.
///
/// # Errors
/// `ValueError` when an int inside has more than [`MAX_STR_DIGITS`] digits.
pub fn frozenset_repr(items: &[Value]) -> PcResult<String> {
    if items.is_empty() {
        return Ok("frozenset()".to_owned());
    }
    Ok(format!("frozenset({{{}}})", join_reprs(items)?))
}

fn join_reprs<'a>(items: impl IntoIterator<Item = &'a Value>) -> PcResult<String> {
    let parts = items
        .into_iter()
        .map(value_repr)
        .collect::<PcResult<Vec<_>>>()?;
    Ok(parts.join(", "))
}

/// Python's `repr()` of any [`Value`]. Always valid text: `repr` escapes
/// lone surrogates.
///
/// # Errors
/// `ValueError` when an int inside has more than [`MAX_STR_DIGITS`] digits.
pub fn value_repr(v: &Value) -> PcResult<String> {
    Ok(match v {
        Value::None => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Int(n) => int_str(n)?,
        Value::Float(f) => float_repr(*f),
        Value::Str(s) => str_repr_code_points(s.code_points()),
        Value::Bytes(b) => bytes_repr(b),
        Value::ByteArray(b) => bytearray_repr(b),
        Value::Tuple(items) if items.len() == 1 => format!("({},)", join_reprs(items)?),
        Value::Tuple(items) => format!("({})", join_reprs(items)?),
        Value::List(items) => format!("[{}]", join_reprs(items)?),
        Value::Set(items) => set_repr(items)?,
        Value::FrozenSet(items) => frozenset_repr(items)?,
        Value::Dict(pairs) => {
            let parts = pairs
                .iter()
                .map(|(k, v)| Ok(format!("{}: {}", value_repr(k)?, value_repr(v)?)))
                .collect::<PcResult<Vec<_>>>()?;
            format!("{{{}}}", parts.join(", "))
        }
        // Unreachable after the guard; shaped like object.__repr__.
        Value::Foreign(name) => format!("<{name} object>"),
    })
}

/// [`value_repr`] as code points, built directly for the common types.
///
/// # Errors
/// As [`value_repr`].
pub fn value_repr_py_str(v: &Value) -> PcResult<PyStr> {
    match v {
        Value::Str(s) => Ok(str_repr_py_str(s.code_points())),
        Value::Int(n) => crate::numbers::int_str_py_str(n),
        Value::Float(f) => Ok(crate::numbers::float_repr_py_str(*f)),
        other => value_repr(other).map(PyStr::from),
    }
}

/// [`str_repr_code_points`] as code points. Printable ASCII text (only
/// `\\` and the quote need escaping) skips the general path.
fn str_repr_py_str(text: &[u32]) -> PyStr {
    if !text.iter().all(|&cp| (0x20..0x7f).contains(&cp)) {
        return PyStr::from(str_repr_code_points(text));
    }
    let has = |c: u8| text.contains(&u32::from(c));
    let quote = u32::from(if has(b'\'') && !has(b'"') {
        b'"'
    } else {
        b'\''
    });
    let backslash = u32::from(b'\\');
    let mut out = Vec::with_capacity(text.len() + 2);
    out.push(quote);
    for &cp in text {
        if cp == backslash || cp == quote {
            out.push(backslash);
        }
        out.push(cp);
    }
    out.push(quote);
    PyStr::from_code_points(out)
}

/// Python's `str()` of any [`Value`].
///
/// # Errors
/// `ValueError` when an int has more than [`MAX_STR_DIGITS`] digits.
pub fn value_str(v: &Value) -> PcResult<PyStr> {
    match v {
        Value::Str(s) => Ok(s.clone()),
        Value::Float(f) => Ok(crate::numbers::float_repr_py_str(*f)),
        Value::Int(n) => crate::numbers::int_str_py_str(n),
        other => value_repr(other).map(PyStr::from),
    }
}

/// Python's `ascii(v)`: `repr(v)` with every non-ASCII code point escaped as
/// `\xhh`, `\uhhhh` or `\Uhhhhhhhh`.
///
/// # Errors
/// As [`value_repr`].
pub fn ascii_repr(v: &Value) -> PcResult<String> {
    Ok(ascii_escape(&PyStr::from(value_repr(v)?)))
}

/// `backslashreplace` ASCII encoding of text.
pub fn ascii_escape(text: &PyStr) -> String {
    let mut out = String::with_capacity(text.len());
    for &cp in text.code_points() {
        match char::from_u32(cp) {
            Some(c) if c.is_ascii() => out.push(c),
            _ if cp < 0x100 => out.push_str(&format!("\\x{cp:02x}")),
            _ if cp < 0x1_0000 => out.push_str(&format!("\\u{cp:04x}")),
            _ => out.push_str(&format!("\\U{cp:08x}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_by_width() {
        let s = PyStr::from_code_points(vec![0x61, 0xe9, 0x65e5, 0x1f600, 0xd800]);
        assert_eq!(ascii_escape(&s), "a\\xe9\\u65e5\\U0001f600\\ud800");
    }

    #[test]
    fn ascii() {
        assert_eq!(ascii_repr(&Value::str("é")), Ok("'\\xe9'".to_owned()));
    }
}

#[cfg(test)]
mod repr_fast_path_tests {
    use super::*;

    #[test]
    fn ascii_repr_fast_path_matches_general_one() {
        let alphabet: Vec<u32> = "a'\"\\ ~\t\u{7f}\u{e9}".chars().map(u32::from).collect();
        // Every string of up to 4 characters from the alphabet.
        let mut stack = vec![Vec::new()];
        while let Some(text) = stack.pop() {
            assert_eq!(
                str_repr_py_str(&text),
                PyStr::from(str_repr_code_points(&text)),
                "{text:?}"
            );
            if text.len() < 4 {
                for &c in &alphabet {
                    let mut next = text.clone();
                    next.push(c);
                    stack.push(next);
                }
            }
        }
    }
}
