//! `format(value, spec)`: the format-spec mini-language for every built-in
//! type, ported from CPython 3.12's `Python/formatter_unicode.c`.

mod number;
mod parse;

use num_bigint::BigInt;

pub(crate) use number::wrap_e_precision;
pub(crate) use number::{check_size, memory_error, try_vec};
pub use number::{double_to_string, int_magnitude_digits, try_double_to_string, DtoaFlags};
pub use parse::{parse_spec, parse_spec_code_points, FormatSpec, Thousands};

use crate::repr::value_str;
use crate::value::{check_builtins, PcResult, PyError, PyStr, Value};

/// `pc_format.format_value(value, spec)`: `format(value, spec)`.
///
/// # Errors
/// As the Python built-in it wraps.
pub fn format_value_api(value: &Value, spec: &Value) -> PcResult<Value> {
    check_builtins(&[value, spec])?;
    let Value::Str(spec) = spec else {
        return Err(PyError::type_error(format!(
            "format() argument 2 must be str, not {}",
            arg_type_name(spec)
        )));
    };
    format_with_spec(value, spec).map(Value::Str)
}

/// The type name CPython's argument-clinic messages use: `None` is named
/// `None`, not `NoneType`.
fn arg_type_name(v: &Value) -> &str {
    match v {
        Value::None => "None",
        other => other.type_name(),
    }
}

fn unknown_type(kind: u32, type_name: &str) -> PyError {
    PyError::value_error(format!(
        "Unknown format code '{}' for object of type '{type_name}'",
        parse::type_char_text(kind)
    ))
}

/// `value.__format__(spec)` for any built-in value (no builtin guard).
///
/// # Errors
/// The `ValueError`/`TypeError`/`OverflowError` CPython raises.
pub fn format_with_spec(value: &Value, spec: &PyStr) -> PcResult<PyStr> {
    format_with_code_points(value, spec.code_points())
}

/// [`format_with_spec`] for a spec given as code points.
///
/// # Errors
/// As [`format_with_spec`].
pub fn format_with_code_points(value: &Value, spec: &[u32]) -> PcResult<PyStr> {
    let mut out = Vec::new();
    format_into(value, spec, &mut out)?;
    Ok(PyStr::from_code_points(out))
}

/// [`format_with_code_points`], appending the text to `out`. On error,
/// `out` may hold part of it.
///
/// # Errors
/// As [`format_with_spec`].
pub(crate) fn format_into(value: &Value, spec: &[u32], out: &mut Vec<u32>) -> PcResult<()> {
    match value {
        Value::Str(s) => format_str(s, spec, out),
        Value::Int(n) => {
            if spec.is_empty() {
                return crate::numbers::push_int_str(n, out);
            }
            format_int_value(n, spec, "int", out)
        }
        Value::Bool(b) => {
            if spec.is_empty() {
                let text: &[u8] = if *b { b"True" } else { b"False" };
                out.extend(text.iter().map(|&c| u32::from(c)));
                return Ok(());
            }
            format_int_value(&BigInt::from(u8::from(*b)), spec, "bool", out)
        }
        Value::Float(x) => {
            if spec.is_empty() {
                let text = crate::numbers::float_repr_ascii(*x);
                out.extend(text.as_bytes().iter().map(|&c| u32::from(c)));
                return Ok(());
            }
            let f = parse_spec_code_points(spec, "float", 0, b'>')?;
            match char::from_u32(f.kind) {
                Some('\0' | 'e' | 'E' | 'f' | 'F' | 'g' | 'G' | 'n' | '%') => {
                    number::format_float(*x, &f, out)
                }
                _ => Err(unknown_type(f.kind, "float")),
            }
        }
        other => {
            if spec.is_empty() {
                out.extend_from_slice(value_str(other)?.code_points());
                Ok(())
            } else {
                Err(PyError::type_error(format!(
                    "unsupported format string passed to {}.__format__",
                    other.type_name()
                )))
            }
        }
    }
}

fn format_int_value(n: &BigInt, spec: &[u32], type_name: &str, out: &mut Vec<u32>) -> PcResult<()> {
    let f = parse_spec_code_points(spec, type_name, u32::from(b'd'), b'>')?;
    match char::from_u32(f.kind) {
        Some('b' | 'c' | 'd' | 'o' | 'x' | 'X' | 'n') => number::format_int(n, &f, out),
        Some('e' | 'E' | 'f' | 'F' | 'g' | 'G' | '%') => {
            number::format_float(number::int_as_float(n)?, &f, out)
        }
        _ => Err(unknown_type(f.kind, type_name)),
    }
}

/// `str.__format__` (`format_string_internal`).
fn format_str(s: &PyStr, spec: &[u32], out: &mut Vec<u32>) -> PcResult<()> {
    if spec.is_empty() {
        out.extend_from_slice(s.code_points());
        return Ok(());
    }
    let f = parse_spec_code_points(spec, "str", u32::from(b's'), b'<')?;
    if f.kind != u32::from(b's') {
        return Err(unknown_type(f.kind, "str"));
    }
    match f.sign {
        b' ' => {
            return Err(PyError::value_error(
                "Space not allowed in string format specifier",
            ))
        }
        0 => {}
        _ => {
            return Err(PyError::value_error(
                "Sign not allowed in string format specifier",
            ))
        }
    }
    if f.no_neg_0 {
        return Err(PyError::value_error(
            "Negative zero coercion (z) not allowed in string format specifier",
        ));
    }
    if f.alternate {
        return Err(PyError::value_error(
            "Alternate form (#) not allowed in string format specifier",
        ));
    }
    if f.align == b'=' {
        return Err(PyError::value_error(
            "'=' alignment not allowed in string format specifier",
        ));
    }
    let cps = s.code_points();
    let len = f.precision.map_or(cps.len(), |p| p.min(cps.len()));
    let width = f.width.unwrap_or(0);
    let total = width.max(len);
    let body = &cps[..len];
    let mut maxchar = body.iter().copied().max().unwrap_or(0);
    if total > len {
        maxchar = maxchar.max(f.fill);
    }
    number::check_size(total, maxchar)?;
    let lpad = match f.align {
        b'>' => total - len,
        b'^' => (total - len) / 2,
        _ => 0,
    };
    let rpad = total - len - lpad;
    out.try_reserve(total).map_err(|_| memory_error())?;
    out.extend(std::iter::repeat_n(f.fill, lpad));
    out.extend_from_slice(body);
    out.extend(std::iter::repeat_n(f.fill, rpad));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::ExcKind;

    fn fmt(v: Value, spec: &str) -> String {
        match format_with_spec(&v, &PyStr::from(spec)) {
            Ok(s) => s.to_string_lossy(),
            Err(e) => e.to_string(),
        }
    }

    #[test]
    fn basics() {
        assert_eq!(fmt(Value::int(1234), "010,"), "00,001,234");
        assert_eq!(fmt(Value::int(255), "#X"), "0XFF");
        assert_eq!(fmt(Value::Float(1e16), "#"), "1.e+16");
        assert_eq!(fmt(Value::Float(f64::INFINITY), "010"), "0000000inf");
        assert_eq!(fmt(Value::Float(-0.0001), "z.2f"), "0.00");
        assert_eq!(fmt(Value::Float(0.125), ".2f"), "0.12");
        assert_eq!(fmt(Value::str("ab"), "05"), "ab000");
        assert_eq!(fmt(Value::Bool(true), ""), "True");
        assert_eq!(fmt(Value::Bool(true), "5"), "    1");
        assert_eq!(fmt(Value::int(0x1234_5678), "_x"), "1234_5678");
        assert_eq!(fmt(Value::Float(-f64::NAN), "+"), "+nan");
        assert_eq!(fmt(Value::Float(0.5), ".0f"), "0");
        assert_eq!(fmt(Value::Float(1.5), "\u{0}"), "1.5");
        assert_eq!(fmt(Value::Float(100.0), "#.0g"), "1.e+02");
        assert_eq!(fmt(Value::Float(1234.5), ",.2%"), "123,450.00%");
        assert_eq!(fmt(Value::int(5), "\u{663}"), "  5");
        assert!(fmt(Value::int(5), "99999999999999999999").contains("Too many decimal digits"));
        assert!(fmt(Value::int(5), ",c").contains("Cannot specify ',' with 'c'."));
        assert_eq!(
            fmt(Value::int(12_345_678), "0=30,"),
            "00,000,000,000,000,012,345,678"
        );
        assert_eq!(
            fmt(Value::Float(-1234.5), "0=20_.3f"),
            "-000_000_001_234.500"
        );
        assert_eq!(fmt(Value::Float(0.0001), "z.1e"), "1.0e-04");
        assert_eq!(fmt(Value::Float(-1e-9), "z.2%"), "0.00%");
        assert_eq!(fmt(Value::Float(1e300), ".400f").len(), 702);
    }

    fn kind(v: Value, spec: &str) -> Option<ExcKind> {
        format_with_spec(&v, &PyStr::from(spec))
            .err()
            .map(|e| e.kind)
    }

    #[test]
    fn huge_sizes() {
        use ExcKind::{MemoryError, ValueError};
        assert_eq!(kind(Value::str("None"), "2147483648"), Some(MemoryError));
        assert_eq!(
            kind(Value::str("ab"), "\u{1F600}<300000000"),
            Some(MemoryError)
        );
        assert_eq!(kind(Value::int(1), "2000000000"), Some(MemoryError));
        assert_eq!(kind(Value::Float(1.5), ".2000000000f"), Some(MemoryError));
        assert_eq!(kind(Value::Float(1.5), ".2000000000g"), Some(MemoryError));
        assert_eq!(kind(Value::Float(1.5), ".2147483648f"), Some(ValueError));
        assert_eq!(
            kind(Value::str("x"), "9223372036854775807"),
            Some(MemoryError)
        );
        assert_eq!(
            kind(Value::str("x"), "9223372036854775808"),
            Some(ValueError)
        );
        assert_eq!(fmt(Value::Float(1.5), "#.2000g").len(), 2001);
    }
}
