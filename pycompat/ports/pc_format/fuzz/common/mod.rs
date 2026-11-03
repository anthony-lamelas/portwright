//! Shared input builders: turn fuzz bytes into `Value`s shaped like what the
//! Python binding can hand the core (only exact built-ins, plus `Foreign`).
#![allow(dead_code)]

use arbitrary::{Result, Unstructured};
use num_bigint::{BigInt, Sign};
use pc_format_port_core::{PyStr, Value};

const MAX_DEPTH: u32 = 4;
const MAX_ITEMS: usize = 6;

/// Characters that drive the parsers (format specs, templates, number text).
const SYNTAX: &[u32] = &[
    b'{' as u32, b'}' as u32, b'%' as u32, b':' as u32, b'!' as u32, b'.' as u32,
    b'[' as u32, b']' as u32, b'(' as u32, b')' as u32, b'*' as u32, b'#' as u32,
    b'0' as u32, b'1' as u32, b'9' as u32, b'_' as u32, b',' as u32, b'+' as u32,
    b'-' as u32, b' ' as u32, b'<' as u32, b'>' as u32, b'=' as u32, b'^' as u32,
    b'z' as u32, b'e' as u32, b'E' as u32, b'f' as u32, b'F' as u32, b'g' as u32,
    b'G' as u32, b'n' as u32, b'%' as u32, b'd' as u32, b'i' as u32, b'u' as u32,
    b'x' as u32, b'X' as u32, b'o' as u32, b'b' as u32, b'c' as u32, b's' as u32,
    b'r' as u32, b'a' as u32, b'j' as u32, b'\t' as u32, b'\n' as u32, b'\x0b' as u32,
    0x00, 0x7f, 0xa0, 0xe9, 0x0663, 0x0966, 0xff10, 0xff19, 0x1d7ce, 0x2007, 0x3000,
    0x2028, 0x1680, 0xd800, 0xdc00, 0xdfff, 0xfffd, 0x10ffff, 0x1f600, 0x0130,
];

/// Words that steer number parsing into special cases.
const WORDS: &[&str] = &[
    "inf", "Infinity", "nan", "NaN", "-inf", "+nan", "0x", "0o", "0b", "0X", "1e308",
    "1e-324", "2.5", "1_000", "9007199254740993", "4.35", "real", "imag", "numerator",
    "denominator", "conjugate", "0.5", "%(a)s", "{0}", "{a}", "{}", "{0!r:>10}", "{0[0]}",
    "2147483648", "9223372036854775808", "%.*f", "%*d",
];

pub fn code_point(u: &mut Unstructured) -> Result<u32> {
    Ok(match u.int_in_range(0u8..=9)? {
        0..=5 => SYNTAX[u.choose_index(SYNTAX.len())?],
        6 | 7 => u.int_in_range(0x20u32..=0x7e)?,
        8 => u.int_in_range(0u32..=0xffff)?,
        _ => u.int_in_range(0u32..=0x10ffff)?,
    })
}

/// Any Python str, including lone surrogates, biased toward parser syntax.
pub fn py_str(u: &mut Unstructured, max_len: usize) -> Result<PyStr> {
    let mut s = PyStr::new();
    let n = u.int_in_range(0..=max_len)?;
    for _ in 0..n {
        if u.ratio(1u8, 8)? {
            s.push_str(WORDS[u.choose_index(WORDS.len())?]);
        } else {
            s.push_code_point(code_point(u)?);
        }
    }
    Ok(s)
}

/// Raw bytes for bytes/bytearray, mostly ASCII so the parsers see syntax.
pub fn byte_string(u: &mut Unstructured, max_len: usize) -> Result<Vec<u8>> {
    let n = u.int_in_range(0..=max_len)?;
    (0..n)
        .map(|_| {
            Ok(if u.ratio(3u8, 4)? {
                let cp = code_point(u)?;
                if cp < 0x80 { cp as u8 } else { u.arbitrary()? }
            } else {
                u.arbitrary()?
            })
        })
        .collect()
}

/// Ints: small values, i64/u64 edges, powers of two and occasionally huge
/// magnitudes (past f64 range and the 4300-digit str limit).
pub fn big_int(u: &mut Unstructured) -> Result<BigInt> {
    Ok(match u.int_in_range(0u8..=9)? {
        0..=3 => BigInt::from(u.int_in_range(-300i64..=300)?),
        4 => BigInt::from(u.arbitrary::<i64>()?),
        5 => {
            let e = u.int_in_range(0u32..=1100)?;
            let b = (BigInt::from(1) << e) + BigInt::from(u.int_in_range(-2i64..=2)?);
            if u.arbitrary()? { -b } else { b }
        }
        6 => {
            let e = u.int_in_range(0u32..=40)?;
            let b = BigInt::from(10).pow(e) + BigInt::from(u.int_in_range(-1i64..=1)?);
            if u.arbitrary()? { -b } else { b }
        }
        7 | 8 => {
            let n = u.int_in_range(0usize..=48)?;
            let bytes = u.bytes(n.min(u.len()))?;
            let sign = if u.arbitrary()? { Sign::Minus } else { Sign::Plus };
            BigInt::from_bytes_le(sign, bytes)
        }
        _ => {
            // Around and beyond 4300 decimal digits (~14285 bits).
            let bits = u.int_in_range(14000u32..=15000)?;
            let b = (BigInt::from(1) << bits) - BigInt::from(u.int_in_range(0i64..=3)?);
            if u.arbitrary()? { -b } else { b }
        }
    })
}

pub fn float(u: &mut Unstructured) -> Result<f64> {
    const SPECIAL: &[f64] = &[
        0.0, -0.0, 0.5, -0.5, 1.5, 2.5, -2.5, 1e16, 1e22, 1e300, 1e-300, 5e-324,
        f64::MAX, f64::MIN_POSITIVE, f64::INFINITY, f64::NEG_INFINITY, f64::NAN,
        9007199254740992.0, 0.1, 1.0 / 3.0, 123456.789, 2.675, 0.125, 1e-5, 1e17,
    ];
    Ok(match u.int_in_range(0u8..=3)? {
        0 | 1 => SPECIAL[u.choose_index(SPECIAL.len())?],
        2 => f64::from_bits(u.arbitrary()?),
        _ => {
            let m = u.int_in_range(-100000i64..=100000)? as f64;
            m / 10f64.powi(u.int_in_range(0i32..=6)?)
        }
    })
}

/// Numbers only: int, bool or float.
pub fn number(u: &mut Unstructured) -> Result<Value> {
    Ok(match u.int_in_range(0u8..=4)? {
        0 | 1 => Value::Int(big_int(u)?),
        2 => Value::Bool(u.arbitrary()?),
        _ => Value::Float(float(u)?),
    })
}

/// Any value the binding can produce, nested up to `MAX_DEPTH`.
pub fn value(u: &mut Unstructured) -> Result<Value> {
    value_at(u, 0)
}

fn items(u: &mut Unstructured, depth: u32) -> Result<Vec<Value>> {
    let n = u.int_in_range(0..=MAX_ITEMS)?;
    (0..n).map(|_| value_at(u, depth + 1)).collect()
}

/// Values that are hashable in Python (usable as dict keys / set items).
pub fn hashable(u: &mut Unstructured, depth: u32) -> Result<Value> {
    Ok(match u.int_in_range(0u8..=7)? {
        0 => Value::None,
        1 => Value::Bool(u.arbitrary()?),
        2 => Value::Int(big_int(u)?),
        3 => Value::Float(float(u)?),
        4 | 5 => Value::Str(py_str(u, 8)?),
        6 => Value::Bytes(byte_string(u, 8)?),
        _ if depth < MAX_DEPTH => {
            let n = u.int_in_range(0..=3)?;
            Value::Tuple((0..n).map(|_| hashable(u, depth + 1)).collect::<Result<_>>()?)
        }
        _ => Value::None,
    })
}

fn value_at(u: &mut Unstructured, depth: u32) -> Result<Value> {
    let leaf = depth >= MAX_DEPTH;
    Ok(match u.int_in_range(0u8..=15)? {
        0 => Value::None,
        1 => Value::Bool(u.arbitrary()?),
        2 | 3 => Value::Int(big_int(u)?),
        4 | 5 => Value::Float(float(u)?),
        6 | 7 => Value::Str(py_str(u, 16)?),
        8 => Value::Bytes(byte_string(u, 16)?),
        9 => Value::ByteArray(byte_string(u, 16)?),
        10 if !leaf => Value::Tuple(items(u, depth)?),
        11 if !leaf => Value::List(items(u, depth)?),
        12 if !leaf => {
            let n = u.int_in_range(0..=MAX_ITEMS)?;
            let v = (0..n).map(|_| hashable(u, depth + 1)).collect::<Result<_>>()?;
            if u.arbitrary()? { Value::Set(v) } else { Value::FrozenSet(v) }
        }
        13 if !leaf => {
            let n = u.int_in_range(0..=MAX_ITEMS)?;
            let pairs = (0..n)
                .map(|_| Ok((hashable(u, depth + 1)?, value_at(u, depth + 1)?)))
                .collect::<Result<_>>()?;
            Value::Dict(pairs)
        }
        14 if u.ratio(1u8, 4)? => Value::Foreign(
            ["object", "complex", "Decimal", "MyStr"][u.choose_index(4)?].to_string(),
        ),
        _ => Value::Str(py_str(u, 4)?),
    })
}

/// Text input for the parsers: usually a str, sometimes bytes/bytearray,
/// sometimes any value (exercising type errors).
pub fn text_like(u: &mut Unstructured, max_len: usize) -> Result<Value> {
    Ok(match u.int_in_range(0u8..=9)? {
        0..=5 => Value::Str(py_str(u, max_len)?),
        6 => Value::Bytes(byte_string(u, max_len)?),
        7 => Value::ByteArray(byte_string(u, max_len)?),
        _ => value(u)?,
    })
}

/// Mostly small ints (including bools), sometimes anything.
pub fn small_int_like(u: &mut Unstructured, lo: i64, hi: i64) -> Result<Value> {
    Ok(match u.int_in_range(0u8..=9)? {
        0..=6 => Value::int(u.int_in_range(lo..=hi)?),
        7 => Value::Bool(u.arbitrary()?),
        8 => Value::Int(big_int(u)?),
        _ => value(u)?,
    })
}

/// Swallows expected Python exceptions; only panics count as failures.
pub fn run<T, E>(r: std::result::Result<T, E>) {
    let _ = r;
}
