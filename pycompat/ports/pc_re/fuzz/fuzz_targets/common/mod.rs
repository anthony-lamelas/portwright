//! Shared input builders: regex-flavoured text and Python-like `Value` trees.
#![allow(dead_code)]

use arbitrary::{Result, Unstructured};
use num_bigint::BigInt;
use pc_re_port_core::{api, PyStr, Value};

/// Characters that the parser, compiler and template code care about.
const ALPHABET: &[u32] = &[
    b'a' as u32, b'b' as u32, b'c' as u32, b'x' as u32, b'A' as u32, b'Z' as u32, b'0' as u32,
    b'1' as u32, b'2' as u32, b'9' as u32, b'_' as u32, b' ' as u32, b'\n' as u32,
    b'(' as u32, b')' as u32, b'[' as u32, b']' as u32, b'{' as u32, b'}' as u32,
    b'?' as u32, b'*' as u32, b'+' as u32, b'|' as u32, b'^' as u32, b'$' as u32,
    b'.' as u32, b'\\' as u32, b'-' as u32, b',' as u32, b':' as u32, b'=' as u32,
    b'!' as u32, b'<' as u32, b'>' as u32, b'P' as u32, b'g' as u32, b'#' as u32,
    b'&' as u32, b'~' as u32, b'i' as u32, b'm' as u32, b's' as u32, b'u' as u32,
    b'L' as u32, b'N' as u32, b'U' as u32, b'w' as u32, b'W' as u32, b'd' as u32,
    b'D' as u32, b'S' as u32, b'B' as u32, b'z' as u32, b'\'' as u32, b'"' as u32,
    0x00, 0x7f, 0xdf, 0xe9, 0x130, 0x131, 0x17f, 0x1e9e, 0x212a, 0x3a3, 0x3c2, 0x660,
    0xd800, 0xdfff, 0xfb05, 0xffff, 0x10000, 0x10ffff, 0x1f600,
];

/// A short PyStr made mostly of regex-relevant characters, with occasional
/// arbitrary code points (lone surrogates included).
pub fn text(u: &mut Unstructured<'_>, max: usize) -> Result<PyStr> {
    let len = u.int_in_range(0..=max)?;
    let mut out = Vec::with_capacity(len);
    for _ in 0..len {
        if u.is_empty() {
            break;
        }
        let b: u8 = u.arbitrary()?;
        if (b as usize) < ALPHABET.len() * 3 {
            out.push(ALPHABET[b as usize % ALPHABET.len()]);
        } else {
            out.push(u.int_in_range(0..=0x10ffffu32)?);
        }
    }
    Ok(out)
}

/// Small pattern fragments glued together, to get past the parser more often.
pub fn pattern(u: &mut Unstructured<'_>) -> Result<PyStr> {
    const PIECES: &[&str] = &[
        "(", ")", "(?:", "(?P<n>", "(?P=n)", "(?P<m1>", "(?=", "(?!", "(?<=", "(?<!", "(?>",
        "(?(1)", "(?(n)", "(?i)", "(?s:", "(?-i:", "(?x)", "(?a)", "(?u)", "(?L)", "(?#c)", "[",
        "[^", "]", "a-z", "\\w", "\\W", "\\d", "\\s", "\\b", "\\B", "\\A", "\\Z", "\\1", "\\2",
        "\\x41", "\\u00e9", "\\U0001f600", "\\N{LATIN SMALL LETTER A}", "\\0", "\\777", "*",
        "+", "?", "*?", "+?", "??", "*+", "++", "?+", "{2}", "{1,3}", "{,2}", "{2,}?", "{0}",
        "|", "^", "$", ".", "a", "b", "ab", "\u{e9}", "\u{130}", "\u{3a3}", "\u{17f}", "K",
        "\\", "[[:alpha:]]", "[a-\\w]", "-", "&&", "~~", "||",
    ];
    let n = u.int_in_range(0..=12)?;
    let mut out = PyStr::new();
    for _ in 0..n {
        if u.ratio(1, 4)? {
            out.extend(text(u, 4)?);
        } else {
            let p = u.choose(PIECES)?;
            out.extend(p.chars().map(u32::from));
        }
    }
    Ok(out)
}

/// A pattern built either from fragments or from raw text.
pub fn any_pattern(u: &mut Unstructured<'_>) -> Result<PyStr> {
    if u.ratio(2, 3)? {
        pattern(u)
    } else {
        text(u, 24)
    }
}

/// An int with emphasis on boundaries (flags, ssize_t, C int).
pub fn int(u: &mut Unstructured<'_>) -> Result<BigInt> {
    const SPECIAL: &[i128] = &[
        0, 1, -1, 2, 3, 4, 8, 16, 32, 64, 128, 256, 512, 2 | 8 | 16, 32 | 256, 34, 160,
        i32::MAX as i128, i32::MIN as i128, i32::MAX as i128 + 1, i64::MAX as i128,
        i64::MIN as i128, i64::MAX as i128 + 1, 1 << 40, u64::MAX as i128, i128::MAX,
    ];
    Ok(match u.int_in_range(0..=3)? {
        0 => BigInt::from(*u.choose(SPECIAL)?),
        1 => BigInt::from(u.int_in_range(-3i64..=40)?),
        2 => BigInt::from(u.arbitrary::<i64>()?),
        _ => {
            let bytes: Vec<u8> = u.arbitrary()?;
            let bytes = &bytes[..bytes.len().min(40)];
            let n = BigInt::from_signed_bytes_le(bytes);
            if u.arbitrary()? {
                // 2**k style huge values
                BigInt::from(1) << u.int_in_range(0..=200u32)?
            } else {
                n
            }
        }
    })
}

/// A flags value: mostly ints, sometimes the odd types `re` rejects.
pub fn flags(u: &mut Unstructured<'_>) -> Result<Value> {
    Ok(match u.int_in_range(0..=9)? {
        0..=6 => {
            // Combine named flags, avoiding DEBUG most of the time.
            let mut f: i64 = 0;
            for bit in [2, 8, 16, 32, 64, 256, 1] {
                if u.ratio(1, 4)? {
                    f |= bit;
                }
            }
            if u.ratio(1, 16)? {
                f |= 128;
            }
            Value::Int(BigInt::from(f))
        }
        7 => Value::Int(int(u)?),
        8 => Value::Bool(u.arbitrary()?),
        _ => value(u, 1)?,
    })
}

/// Arbitrary Value tree, depth-limited. May contain compiled patterns and matches.
pub fn value(u: &mut Unstructured<'_>, depth: u32) -> Result<Value> {
    let max = if depth == 0 { 11 } else { 17 };
    Ok(match u.int_in_range(0..=max)? {
        0 => Value::None,
        1 => Value::Bool(u.arbitrary()?),
        2 | 3 => Value::Int(int(u)?),
        4 => Value::Float(match u.int_in_range(0..=6)? {
            0 => f64::NAN,
            1 => f64::INFINITY,
            2 => -0.0,
            3 => 1e16,
            4 => 2.0,
            _ => u.arbitrary()?,
        }),
        5 | 6 => Value::Str(text(u, 12)?),
        7 => Value::Bytes(u.arbitrary::<Vec<u8>>()?.into_iter().take(8).collect()),
        8 => Value::ByteArray(u.arbitrary::<Vec<u8>>()?.into_iter().take(8).collect()),
        9 => Value::Decimal(
            (*u.choose(&["0", "1", "-1.5", "2", "1E+2", "0.000", "-0"])?).to_string(),
        ),
        10 => Value::Other((*u.choose(&["object", "Foo", "function", "type"])?).to_string()),
        11 => match compiled(u)? {
            Some(p) => p,
            None => Value::None,
        },
        12 => Value::List(values(u, depth - 1)?),
        13 => Value::Tuple(values(u, depth - 1)?),
        14 => Value::Set(values(u, depth - 1)?),
        15 => Value::FrozenSet(values(u, depth - 1)?),
        16 => {
            let n = u.int_in_range(0..=3)?;
            let mut d = Vec::with_capacity(n);
            for _ in 0..n {
                d.push((value(u, depth - 1)?, value(u, depth - 1)?));
            }
            Value::Dict(d)
        }
        _ => match a_match(u)? {
            Some(m) => m,
            None => Value::None,
        },
    })
}

fn values(u: &mut Unstructured<'_>, depth: u32) -> Result<Vec<Value>> {
    let n = u.int_in_range(0..=3)?;
    (0..n).map(|_| value(u, depth)).collect()
}

/// A compiled Pattern value, if the generated pattern compiles.
pub fn compiled(u: &mut Unstructured<'_>) -> Result<Option<Value>> {
    let p = pattern(u)?;
    let flags = Value::Int(BigInt::from(*u.choose(&[0i64, 2, 8, 16, 32, 64, 256, 2 | 256])?));
    let r = api::compile(&[Value::Str(p)], &[("flags".to_string(), flags)]).ok();
    drain();
    Ok(r)
}

/// A Match value, if the generated search matches.
pub fn a_match(u: &mut Unstructured<'_>) -> Result<Option<Value>> {
    let p = pattern(u)?;
    let s = text(u, 16)?;
    let r = api::search(&[Value::Str(p), Value::Str(s)], &[]).ok();
    drain();
    Ok(r.filter(|v| matches!(v, Value::Match(_))))
}

/// Discard DEBUG output so it can't pile up across iterations.
pub fn drain() {
    let _ = pc_re_port_core::stdout::take();
}

/// Exercise every Match accessor on a match value.
pub fn poke_match(u: &mut Unstructured<'_>, m: &Value) {
    let m = m.clone();
    let idx = || -> Value { Value::Int(BigInt::from(0)) };
    let _ = api::match_group(&[m.clone()], &[]);
    let _ = api::match_groups(&[m.clone()], &[]);
    let _ = api::match_groupdict(&[m.clone()], &[]);
    let _ = api::match_span(&[m.clone()], &[]);
    let _ = api::match_start(&[m.clone()], &[]);
    let _ = api::match_end(&[m.clone()], &[]);
    let _ = api::match_lastindex(&[m.clone()], &[]);
    let _ = api::match_lastgroup(&[m.clone()], &[]);
    let _ = api::match_pos(&[m.clone()], &[]);
    let _ = api::match_endpos(&[m.clone()], &[]);
    let _ = api::match_string(&[m.clone()], &[]);
    let _ = pc_re_port_core::pyrepr::repr(&m);
    for _ in 0..3 {
        let g = match u.int_in_range(0..=3).unwrap_or(0) {
            0 => idx(),
            1 => Value::Int(BigInt::from(u.int_in_range(-2i64..=6).unwrap_or(1))),
            2 => Value::Str(text(u, 4).unwrap_or_default()),
            _ => value(u, 1).unwrap_or(Value::None),
        };
        let _ = api::match_group(&[m.clone(), g.clone(), idx()], &[]);
        let _ = api::match_start(&[m.clone(), g.clone()], &[]);
        let _ = api::match_end(&[m.clone(), g.clone()], &[]);
        let _ = api::match_span(&[m.clone(), g.clone()], &[]);
        let _ = api::match_groups(&[m.clone(), g.clone()], &[]);
        let _ = api::match_groupdict(&[m.clone(), g], &[]);
    }
    if let Ok(t) = text(u, 16) {
        let _ = api::match_expand(&[m.clone(), Value::Str(t)], &[]);
    }
}
