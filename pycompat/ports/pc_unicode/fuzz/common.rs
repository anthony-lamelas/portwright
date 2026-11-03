//! Shared builders that turn fuzz bytes into structured `Value` arguments.
//! Shared by every target via `#[path = "../common.rs"] mod common;`.
#![allow(dead_code)]

use arbitrary::{Result, Unstructured};
use num_bigint::BigInt;
use pc_unicode_port_core::Value;

/// Interesting code points: ASCII edges, whitespace/line breaks, casing specials,
/// combining marks, Hangul, surrogates, CJK, unassigned, max.
const SPECIAL: &[u32] = &[
    0x00, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x1C, 0x1D, 0x1E, 0x1F, 0x20, 0x2B, 0x2D, 0x30, 0x41,
    0x5F, 0x61, 0x7F, 0x85, 0xA0, 0xAD, 0xB2, 0xB5, 0xBD, 0xDF, 0x130, 0x131, 0x149, 0x1C5,
    0x300, 0x301, 0x345, 0x3A3, 0x3C2, 0x3C3, 0x390, 0x1E9E, 0x2028, 0x2029, 0x2160, 0x2163,
    0x3000, 0x0660, 0x0F73, 0x1100, 0x1161, 0x11A8, 0xAC00, 0xAC01, 0xD7A3, 0xD800, 0xDBFF,
    0xDC00, 0xDFFF, 0x4E00, 0x9FFF, 0xF900, 0xFB00, 0xFB03, 0xFEFF, 0xFFFD, 0xFFFF, 0x10400,
    0x1D400, 0x1F600, 0x20000, 0x2FA1D, 0xE0001, 0x10FFFF,
];

/// One code point in `0..=0x10FFFF`, biased toward interesting ones.
pub fn code_point(u: &mut Unstructured) -> Result<u32> {
    Ok(match u.int_in_range(0u8..=9)? {
        0..=3 => u32::from(u.int_in_range(0x20u8..=0x7E)?),
        4..=6 => *u.choose(SPECIAL)?,
        7 => u.int_in_range(0x80u32..=0x3000)?,
        8 => u.int_in_range(0xD800u32..=0xDFFF)?,
        _ => u.int_in_range(0u32..=0x10FFFF)?,
    })
}

/// Python text, possibly empty, possibly with lone surrogates.
pub fn text(u: &mut Unstructured) -> Result<Vec<u32>> {
    let len = u.int_in_range(0usize..=64)?;
    let mut out = Vec::with_capacity(len);
    for _ in 0..len {
        if u.is_empty() {
            break;
        }
        out.push(code_point(u)?);
    }
    Ok(out)
}

/// Text that looks like a Unicode character name (for `lookup`).
pub fn name_text(u: &mut Unstructured) -> Result<Vec<u32>> {
    const PREFIXES: &[&str] = &[
        "LATIN SMALL LETTER ",
        "LATIN CAPITAL LETTER ",
        "HANGUL SYLLABLE ",
        "CJK UNIFIED IDEOGRAPH-",
        "CJK COMPATIBILITY IDEOGRAPH-",
        "TANGUT IDEOGRAPH-",
        "KHITAN SMALL SCRIPT CHARACTER-",
        "NUSHU CHARACTER-",
        "GREEK SMALL LETTER ",
        "DIGIT ",
        "",
    ];
    const ALPHA: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef0123456789 -_";
    let mut out: Vec<u32> = u.choose(PREFIXES)?.chars().map(u32::from).collect();
    let len = u.int_in_range(0usize..=24)?;
    for _ in 0..len {
        if u.is_empty() {
            break;
        }
        if u.ratio(1u8, 16)? {
            out.push(code_point(u)?);
        } else {
            out.push(u32::from(*u.choose(ALPHA)?));
        }
    }
    Ok(out)
}

/// An int, either small, near an i32/i64 boundary, or bigger than i64.
/// Positive values in `4097..MAX_TEXT_LEN` (e.g. `i32::MAX + 1`)
/// are never produced: as widths they
/// legitimately allocate gigabytes, which libFuzzer reports as OOM.
pub fn int(u: &mut Unstructured) -> Result<BigInt> {
    Ok(match u.int_in_range(0u8..=7)? {
        0..=3 => BigInt::from(u.int_in_range(-16i64..=300)?),
        4 => BigInt::from(*u.choose(&[
            i64::MAX,
            i64::MIN,
            i64::MAX - 1,
            i64::MIN + 1,
            // MAX_TEXT_LEN + 1 = i64::MAX / 4 - 13: smallest width rejected before allocating.
            i64::MAX / 4,
            i64::MAX / 4 - 13,
            i64::MAX / 2,
            i64::from(i32::MIN),
            i64::from(i32::MIN) - 1,
            -1,
            0,
        ])?),
        5 => BigInt::from(u.int_in_range(i64::MIN..=-1)?),
        6 => {
            let mag = BigInt::from(u.arbitrary::<u64>()?) << u.int_in_range(64u32..=200)?;
            if u.arbitrary()? {
                -mag
            } else {
                mag
            }
        }
        _ => BigInt::from(u.int_in_range(0i64..=4096)?),
    })
}

/// `tabsize` for expandtabs: small, negative, zero, or outside C int range.
/// Values in `65536..=i32::MAX` are skipped: with tabs they make a legitimate
/// multi-gigabyte string, which the fuzzer would report as an OOM.
pub fn tabsize_arg(u: &mut Unstructured) -> Result<Value> {
    Ok(match u.int_in_range(0u8..=9)? {
        0 => Value::Bool(u.arbitrary()?),
        1 => Value::Int(BigInt::from(*u.choose(&[
            i64::from(i32::MIN),
            i64::from(i32::MIN) - 1,
            i64::from(i32::MAX) + 1,
            i64::MAX,
            i64::MIN,
        ])?)),
        2 => Value::Int(BigInt::from(u.int_in_range(i64::from(i32::MIN)..=-1)?)),
        3 => {
            let v = value(u, 1)?;
            match v {
                Value::Int(_) => Value::Int(BigInt::from(-1)),
                other => other,
            }
        }
        _ => Value::Int(BigInt::from(u.int_in_range(-4i64..=65535)?)),
    })
}

/// Any built-in value (or an `Other`), nested up to `depth`.
pub fn value(u: &mut Unstructured, depth: u8) -> Result<Value> {
    let max = if depth == 0 { 9 } else { 13 };
    Ok(match u.int_in_range(0u8..=max)? {
        0 => Value::None,
        1 => Value::Bool(u.arbitrary()?),
        2 => Value::Int(int(u)?),
        3 => Value::Float(f64::from_bits(u.arbitrary()?)),
        4 | 5 => Value::Str(text(u)?),
        6 => Value::Bytes(u.arbitrary()?),
        7 => Value::ByteArray(u.arbitrary()?),
        8 => Value::Other((*u.choose(&["MyStr", "object", "Decimal", "function"])?).to_string()),
        9 => Value::Float(*u.choose(&[f64::NAN, -0.0, 0.0, f64::INFINITY, 1.5])?),
        10 => Value::List(seq(u, depth)?),
        11 => Value::Tuple(seq(u, depth)?),
        12 => Value::Set(seq(u, depth)?),
        _ => {
            let n = u.int_in_range(0usize..=4)?;
            let mut pairs = Vec::with_capacity(n);
            for _ in 0..n {
                pairs.push((value(u, depth - 1)?, value(u, depth - 1)?));
            }
            Value::Dict(pairs)
        }
    })
}

fn seq(u: &mut Unstructured, depth: u8) -> Result<Vec<Value>> {
    let n = u.int_in_range(0usize..=4)?;
    (0..n).map(|_| value(u, depth - 1)).collect()
}

/// Mostly a `str`, sometimes any other value (to reach type errors).
pub fn str_arg(u: &mut Unstructured) -> Result<Value> {
    if u.ratio(1u8, 10)? {
        value(u, 2)
    } else {
        Ok(Value::Str(text(u)?))
    }
}

/// Mostly a single-character `str` (for per-character APIs), sometimes anything.
pub fn char_arg(u: &mut Unstructured) -> Result<Value> {
    match u.int_in_range(0u8..=9)? {
        0 => value(u, 2),
        1 => Ok(Value::Str(text(u)?)),
        _ => Ok(Value::Str(vec![code_point(u)?])),
    }
}

/// Mostly an int (or bool), sometimes anything.
pub fn int_arg(u: &mut Unstructured) -> Result<Value> {
    match u.int_in_range(0u8..=9)? {
        0 => value(u, 1),
        1 => Ok(Value::Bool(u.arbitrary()?)),
        _ => Ok(Value::Int(int(u)?)),
    }
}

/// Omitted (`None` = use default) or a value built by `f`.
pub fn opt<F>(u: &mut Unstructured, f: F) -> Result<Option<Value>>
where
    F: FnOnce(&mut Unstructured) -> Result<Value>,
{
    if u.arbitrary()? {
        Ok(None)
    } else {
        f(u).map(Some)
    }
}

/// `None`, a `str`, or anything (for `sep`/`chars`).
pub fn opt_str_or_none(u: &mut Unstructured) -> Result<Option<Value>> {
    opt(u, |u| {
        Ok(match u.int_in_range(0u8..=5)? {
            0 => Value::None,
            1 => value(u, 1)?,
            _ => Value::Str(text(u)?),
        })
    })
}

/// A normalization form name: valid, near-miss, or any value.
pub fn form_arg(u: &mut Unstructured) -> Result<Value> {
    const FORMS: &[&str] = &["NFC", "NFD", "NFKC", "NFKD", "nfc", "NFC ", "NF", "", "NFKC\0"];
    Ok(match u.int_in_range(0u8..=9)? {
        0 => value(u, 1)?,
        1 => Value::Str(text(u)?),
        _ => Value::Str(u.choose(FORMS)?.chars().map(u32::from).collect()),
    })
}
