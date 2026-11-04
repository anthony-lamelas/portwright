//! Python `str()` and `repr()` of [`Value`]s, as CPython 3.12 prints them.
//!
//! Needed wherever the original formats an argument into text, such as
//! `'%s at position %d' % (msg, pos)` in `re.error.__init__`.

use num_bigint::BigInt;

use crate::error::{Error, Result};
use crate::text::{push_str, PyStr};
use crate::unicode;
use crate::value::Value;

/// CPython's default `sys.get_int_max_str_digits()`.
const INT_MAX_STR_DIGITS: usize = 4300;

/// Decimal text of an int, with CPython's digit limit for int to str conversion.
///
/// # Errors
/// `ValueError` when the result would have more than 4300 digits.
pub fn int_text(n: &BigInt) -> Result<String> {
    // Cheap bound first: bits * log10(2) digits, so huge values fail fast.
    if n.bits() > (INT_MAX_STR_DIGITS as u64) * 4 {
        return Err(int_limit_error());
    }
    let text = n.to_string();
    let digits = text.trim_start_matches('-').len();
    if digits > INT_MAX_STR_DIGITS {
        return Err(int_limit_error());
    }
    Ok(text)
}

fn int_limit_error() -> Error {
    Error::Value(format!(
        "Exceeds the limit ({INT_MAX_STR_DIGITS} digits) for integer string conversion; \
         use sys.set_int_max_str_digits() to increase the limit"
    ))
}

/// `repr(float)`: the shortest round-tripping digits, in fixed notation when
/// the decimal exponent is in `-4..16`, else `d.ddde+XX`.
pub fn float_repr(f: f64) -> String {
    if f.is_nan() {
        return "nan".to_owned();
    }
    if f.is_infinite() {
        return if f < 0.0 { "-inf" } else { "inf" }.to_owned();
    }
    // `{:e}` gives the shortest round-trip digits: "-1.5e-7", "0e0".
    let sci = format!("{f:e}");
    let (mantissa, exp) = sci.split_once('e').unwrap_or((sci.as_str(), "0"));
    let exp: i64 = exp.parse().unwrap_or(0);
    let (sign, mantissa) = match mantissa.strip_prefix('-') {
        Some(m) => ("-", m),
        None => ("", mantissa),
    };
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let ndigits = i64::try_from(digits.len()).unwrap_or(i64::MAX);
    // Position of the decimal point relative to the start of `digits`.
    let decpt = exp + 1;
    let mut out = String::from(sign);
    if -4 < decpt && decpt <= 16 {
        if decpt <= 0 {
            out.push_str("0.");
            for _ in 0..(-decpt) {
                out.push('0');
            }
            out.push_str(&digits);
        } else if decpt >= ndigits {
            out.push_str(&digits);
            for _ in 0..(decpt - ndigits) {
                out.push('0');
            }
            out.push_str(".0");
        } else {
            let split = usize::try_from(decpt).unwrap_or(0);
            out.push_str(&digits[..split]);
            out.push('.');
            out.push_str(&digits[split..]);
        }
    } else {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        let e = decpt - 1;
        let esign = if e < 0 { '-' } else { '+' };
        out.push_str(&format!("e{esign}{:02}", e.abs()));
    }
    out
}

fn push_hex(out: &mut PyStr, prefix: &str, c: u32, width: usize) {
    push_str(out, &format!("{prefix}{c:0width$x}"));
}

/// `repr(str)`: CPython's quote choice and escapes (`unicode_repr`).
pub fn str_repr(s: &[u32]) -> PyStr {
    let quote = if s.contains(&u32::from('\'')) && !s.contains(&u32::from('"')) {
        u32::from('"')
    } else {
        u32::from('\'')
    };
    let mut out = Vec::with_capacity(s.len() + 2);
    out.push(quote);
    for &c in s {
        match c {
            _ if c == quote || c == 0x5C => {
                out.push(0x5C);
                out.push(c);
            }
            0x09 => push_str(&mut out, "\\t"),
            0x0A => push_str(&mut out, "\\n"),
            0x0D => push_str(&mut out, "\\r"),
            _ if c < 0x20 || c == 0x7F => push_hex(&mut out, "\\x", c, 2),
            _ if c < 0x7F => out.push(c),
            _ if unicode::is_printable(c) => out.push(c),
            _ if c <= 0xFF => push_hex(&mut out, "\\x", c, 2),
            _ if c <= 0xFFFF => push_hex(&mut out, "\\u", c, 4),
            _ => push_hex(&mut out, "\\U", c, 8),
        }
    }
    out.push(quote);
    out
}

/// `repr(bytes)`.
pub fn bytes_repr(b: &[u8]) -> String {
    let quote = if b.contains(&b'\'') && !b.contains(&b'"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(b.len() + 3);
    out.push('b');
    out.push(quote);
    for &c in b {
        match c {
            _ if char::from(c) == quote || c == b'\\' => {
                out.push('\\');
                out.push(char::from(c));
            }
            b'\t' => out.push_str("\\t"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            _ if !(0x20..0x7F).contains(&c) => out.push_str(&format!("\\x{c:02x}")),
            _ => out.push(char::from(c)),
        }
    }
    out.push(quote);
    out
}

fn push_items(out: &mut PyStr, items: &[Value]) -> Result<()> {
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            push_str(out, ", ");
        }
        out.extend(repr(item)?);
    }
    Ok(())
}

/// `repr(value)`.
///
/// # Errors
/// `ValueError` for ints over CPython's 4300-digit conversion limit.
pub fn repr(value: &Value) -> Result<PyStr> {
    let mut out = PyStr::new();
    match value {
        Value::None => push_str(&mut out, "None"),
        Value::Bool(true) => push_str(&mut out, "True"),
        Value::Bool(false) => push_str(&mut out, "False"),
        Value::Int(n) => push_str(&mut out, &int_text(n)?),
        Value::Float(f) => push_str(&mut out, &float_repr(*f)),
        Value::Str(s) => out = str_repr(s),
        Value::Bytes(b) => push_str(&mut out, &bytes_repr(b)),
        Value::ByteArray(b) => push_str(&mut out, &format!("bytearray({})", bytes_repr(b))),
        Value::List(items) => {
            push_str(&mut out, "[");
            push_items(&mut out, items)?;
            push_str(&mut out, "]");
        }
        Value::Tuple(items) => {
            push_str(&mut out, "(");
            push_items(&mut out, items)?;
            if items.len() == 1 {
                push_str(&mut out, ",");
            }
            push_str(&mut out, ")");
        }
        Value::Set(items) if items.is_empty() => push_str(&mut out, "set()"),
        Value::Set(items) => {
            push_str(&mut out, "{");
            push_items(&mut out, items)?;
            push_str(&mut out, "}");
        }
        Value::FrozenSet(items) if items.is_empty() => push_str(&mut out, "frozenset()"),
        Value::FrozenSet(items) => {
            push_str(&mut out, "frozenset({");
            push_items(&mut out, items)?;
            push_str(&mut out, "})");
        }
        Value::Dict(pairs) => {
            push_str(&mut out, "{");
            for (i, (k, v)) in pairs.iter().enumerate() {
                if i > 0 {
                    push_str(&mut out, ", ");
                }
                out.extend(repr(k)?);
                push_str(&mut out, ": ");
                out.extend(repr(v)?);
            }
            push_str(&mut out, "}");
        }
        Value::Decimal(text) => push_str(&mut out, &format!("Decimal('{text}')")),
        Value::Pattern(p) => out = p.repr(),
        Value::Match(m) => out = m.repr(),
        // The object's own __repr__ is out of reach; this is CPython's default
        // without the address.
        Value::Other(name) => push_str(&mut out, &format!("<{name} object>")),
    }
    Ok(out)
}

/// `str(value)`: the text itself for `str`, `Decimal`'s own text, else `repr`.
///
/// # Errors
/// `ValueError` for ints over CPython's 4300-digit conversion limit.
pub fn str(value: &Value) -> Result<PyStr> {
    match value {
        Value::Str(s) => Ok(s.clone()),
        Value::Decimal(text) => Ok(text.chars().map(u32::from).collect()),
        other => repr(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::{cps, to_string_lossy};

    fn r(v: &Value) -> String {
        repr(v).map(|s| to_string_lossy(&s)).unwrap_or_default()
    }

    #[test]
    fn floats() {
        for (f, want) in [
            (1e16, "1e+16"),
            (1e15, "1000000000000000.0"),
            (-0.0, "-0.0"),
            (1.5e-7, "1.5e-07"),
            (1e-5, "1e-05"),
            (1e-4, "0.0001"),
            (1.2345678901234568e17, "1.2345678901234568e+17"),
            (1.5, "1.5"),
            (0.0, "0.0"),
            (123.0, "123.0"),
            (1e300 * 1e10, "inf"),
            (1e22, "1e+22"),
            (5e-324, "5e-324"),
        ] {
            assert_eq!(float_repr(f), want, "{f}");
        }
    }

    #[test]
    fn strings() {
        assert_eq!(to_string_lossy(&str_repr(&cps("a'b"))), "\"a'b\"");
        assert_eq!(to_string_lossy(&str_repr(&cps("a'\""))), "'a\\'\"'");
        assert_eq!(
            to_string_lossy(&str_repr(&[0, 9, 0x7F, 0x9F, 0xAD, 0xD800, 0xE0001])),
            "'\\x00\\t\\x7f\\x9f\\xad\\ud800\\U000e0001'"
        );
        assert_eq!(bytes_repr(b"\x00'\"\x80"), "b'\\x00\\'\"\\x80'");
        assert_eq!(bytes_repr(b"'"), "b\"'\"");
    }

    #[test]
    fn containers() {
        let one = Value::Int(BigInt::from(1));
        assert_eq!(r(&Value::Tuple(vec![one.clone()])), "(1,)");
        assert_eq!(r(&Value::Tuple(vec![])), "()");
        assert_eq!(r(&Value::Set(vec![])), "set()");
        assert_eq!(r(&Value::FrozenSet(vec![one.clone()])), "frozenset({1})");
        assert_eq!(
            r(&Value::Dict(vec![(one, Value::Str(cps("a")))])),
            "{1: 'a'}"
        );
        assert_eq!(r(&Value::ByteArray(vec![0])), "bytearray(b'\\x00')");
        assert_eq!(r(&Value::Decimal("1.5".into())), "Decimal('1.5')");
    }

    #[test]
    fn int_limit() {
        let big = num_traits::pow(BigInt::from(10), 4300);
        assert!(int_text(&big).is_err());
        let ok = num_traits::pow(BigInt::from(10), 4299);
        assert!(int_text(&ok).is_ok());
        assert!(int_text(&-ok).is_ok());
    }
}
