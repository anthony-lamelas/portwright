//! Unicode 15.0 character data exactly as CPython 3.12 sees it.
//!
//! Tables are generated from the interpreter itself (`tools/gen_unicode.py`),
//! so they match `str` methods, `_sre` and `unicodedata` code point for code
//! point, surrogates included. Don't use `char` methods or the `regex` crate's
//! tables instead: their Unicode versions differ.

use std::cmp::Ordering;

use crate::unicode_tables as t;

fn in_ranges(table: &[(u32, u32)], c: u32) -> bool {
    table
        .binary_search_by(|&(lo, hi)| {
            if hi < c {
                Ordering::Less
            } else if lo > c {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        })
        .is_ok()
}

fn mapped(table: &[(u32, u32)], c: u32) -> u32 {
    table
        .binary_search_by_key(&c, |&(k, _)| k)
        .ok()
        .and_then(|i| table.get(i))
        .map_or(c, |&(_, v)| v)
}

/// `str.isprintable()` for one code point (decides escaping in `repr`).
pub fn is_printable(c: u32) -> bool {
    in_ranges(t::PRINTABLE, c)
}

/// `str.isdecimal()`: sre's Unicode `\d`.
pub fn is_decimal(c: u32) -> bool {
    in_ranges(t::DECIMAL, c)
}

/// `unicodedata.decimal(c)` (what `int()` uses for each digit).
pub fn decimal_value(c: u32) -> Option<u32> {
    let i = t::DECIMAL_VALUE
        .binary_search_by(|&(lo, hi, _)| {
            if hi < c {
                Ordering::Less
            } else if lo > c {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        })
        .ok()?;
    t::DECIMAL_VALUE.get(i).map(|&(lo, _, v)| v + (c - lo))
}

/// `str.isalnum()`: sre's Unicode `\w` is this or `_`.
pub fn is_alnum(c: u32) -> bool {
    in_ranges(t::ALNUM, c)
}

/// sre's Unicode word character (`\w`, `\b`): alphanumeric or `_`.
pub fn is_word(c: u32) -> bool {
    c == u32::from(b'_') || is_alnum(c)
}

/// `str.isspace()`: sre's Unicode `\s`.
pub fn is_space(c: u32) -> bool {
    in_ranges(t::SPACE, c)
}

/// Whether `s.isidentifier()` (no keyword check, like Python).
pub fn is_identifier(s: &[u32]) -> bool {
    match s.split_first() {
        None => false,
        Some((&first, rest)) => {
            in_ranges(t::XID_START, first) && rest.iter().all(|&c| in_ranges(t::XID_CONTINUE, c))
        }
    }
}

/// `_sre.unicode_tolower(c)` (`Py_UNICODE_TOLOWER`).
pub fn to_lower(c: u32) -> u32 {
    mapped(t::LOWER, c)
}

/// `Py_UNICODE_TOUPPER(c)`: the first code point of `chr(c).upper()`.
pub fn to_upper(c: u32) -> u32 {
    mapped(t::UPPER, c)
}

/// `_sre.unicode_iscased(c)`.
pub fn is_cased(c: u32) -> bool {
    in_ranges(t::CASED, c)
}

/// ASCII `\w` (`(?a)`): `[a-zA-Z0-9_]`.
pub fn is_ascii_word(c: u32) -> bool {
    c < 128 && (c == u32::from(b'_') || (c as u8).is_ascii_alphanumeric())
}

/// ASCII `\d`.
pub fn is_ascii_digit(c: u32) -> bool {
    (0x30..=0x39).contains(&c)
}

/// ASCII `\s` (sre `SRE_IS_SPACE`): `[ \t\n\r\x0b\x0c]`.
pub fn is_ascii_space(c: u32) -> bool {
    matches!(c, 0x09..=0x0D | 0x20)
}

/// `_sre.ascii_tolower`.
pub fn ascii_lower(c: u32) -> u32 {
    if (0x41..=0x5A).contains(&c) {
        c + 32
    } else {
        c
    }
}

/// `_sre.ascii_iscased`.
pub fn ascii_is_cased(c: u32) -> bool {
    c < 128 && (c as u8).is_ascii_alphabetic()
}

const JAMO_L: [&str; 19] = [
    "G", "GG", "N", "D", "DD", "R", "M", "B", "BB", "S", "SS", "", "J", "JJ", "C", "K", "T", "P",
    "H",
];
const JAMO_V: [&str; 21] = [
    "A", "AE", "YA", "YAE", "EO", "E", "YEO", "YE", "O", "WA", "WAE", "OE", "YO", "U", "WEO", "WE",
    "WI", "YU", "EU", "YI", "I",
];
const JAMO_T: [&str; 28] = [
    "", "G", "GG", "GS", "N", "NJ", "NH", "D", "L", "LG", "LM", "LB", "LS", "LT", "LP", "LH", "M",
    "B", "BS", "S", "SS", "NG", "J", "C", "K", "T", "P", "H",
];

/// Longest entry of `table` that prefixes `s`, as `(index, length)`.
fn longest_jamo(table: &[&str], s: &str) -> Option<(usize, usize)> {
    table
        .iter()
        .enumerate()
        .filter(|(_, j)| s.starts_with(**j))
        .max_by_key(|(_, j)| j.len())
        .map(|(i, j)| (i, j.len()))
}

fn hangul(rest: &str) -> Option<u32> {
    let (l, n1) = longest_jamo(&JAMO_L, rest)?;
    let rest = &rest[n1..];
    let (v, n2) = longest_jamo(&JAMO_V, rest)?;
    let rest = &rest[n2..];
    let (tt, n3) = longest_jamo(&JAMO_T, rest)?;
    if n3 != rest.len() {
        return None;
    }
    u32::try_from((l * 21 + v) * 28 + tt)
        .ok()
        .map(|i| 0xAC00 + i)
}

fn cjk(rest: &str) -> Option<u32> {
    if !(rest.len() == 4 || rest.len() == 5)
        || !rest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b))
    {
        return None;
    }
    let c = u32::from_str_radix(rest, 16).ok()?;
    in_ranges(t::CJK_UNIFIED, c).then_some(c)
}

/// `ord(unicodedata.lookup(name))` for names (and name aliases) of single
/// code points, as `\N{...}` uses it; `None` where `re` reports
/// `undefined character name`. Names match ASCII-case-insensitively, except
/// the algorithmic `HANGUL SYLLABLE ` and `CJK UNIFIED IDEOGRAPH-` forms,
/// which CPython only accepts in upper case. Named sequences aren't single
/// code points, so they are `None` too.
pub fn lookup_name(name: &[u32]) -> Option<u32> {
    let name = crate::text::to_string(name)?;
    if !name.is_ascii() {
        return None;
    }
    if let Some(rest) = name.strip_prefix("HANGUL SYLLABLE ") {
        if let Some(c) = hangul(rest) {
            return Some(c);
        }
    }
    if let Some(rest) = name.strip_prefix("CJK UNIFIED IDEOGRAPH-") {
        return cjk(rest);
    }
    let upper = name.to_ascii_uppercase();
    t::NAMES
        .binary_search_by(|&(n, _)| n.cmp(upper.as_str()))
        .ok()
        .and_then(|i| t::NAMES.get(i))
        .map(|&(_, c)| c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::cps;

    fn look(s: &str) -> Option<u32> {
        lookup_name(&cps(s))
    }

    #[test]
    fn names() {
        assert_eq!(look("EM DASH"), Some(0x2014));
        assert_eq!(look("latin small letter a"), Some(0x61));
        assert_eq!(look("LATIN SMALL LETTER SHARP S"), Some(0xDF));
        assert_eq!(look("LATIN CAPITAL LETTER GHA"), Some(0x1A2));
        assert_eq!(look("BYTE ORDER MARK"), Some(0xFEFF));
        assert_eq!(look("HANGUL SYLLABLE GA"), Some(0xAC00));
        assert_eq!(look("HANGUL SYLLABLE HIH"), Some(0xD7A3));
        assert_eq!(look("hangul syllable ga"), None);
        assert_eq!(look("CJK UNIFIED IDEOGRAPH-4E00"), Some(0x4E00));
        assert_eq!(look("CJK UNIFIED IDEOGRAPH-4e00"), None);
        assert_eq!(look("TANGUT IDEOGRAPH-17000"), None);
        assert_eq!(look("LATIN CAPITAL LETTER A WITH MACRON AND GRAVE"), None);
        assert_eq!(look("NOPE"), None);
        assert_eq!(look("é"), None);
    }

    #[test]
    fn classes() {
        assert!(is_decimal(0x663)); // ARABIC-INDIC DIGIT THREE
        assert_eq!(decimal_value(0x663), Some(3));
        assert_eq!(decimal_value(0x39), Some(9));
        assert_eq!(decimal_value(0xB2), None); // superscript two: digit, not decimal
        assert!(is_word(0xB2) && is_word(u32::from(b'_')) && !is_word(0x20));
        assert!(is_space(0x1C) && is_space(0x85) && is_space(0xA0) && !is_space(0x200B));
        assert!(!is_printable(0xD800) && !is_printable(0xA0) && is_printable(0x20));
        assert!(is_identifier(&cps("é1")) && !is_identifier(&cps("1a")) && !is_identifier(&[]));
        assert!(is_identifier(&cps("_x")) && !is_identifier(&cps("a-b")));
    }

    #[test]
    fn case() {
        assert_eq!(to_lower(0x130), 0x69); // İ -> i (first of the full mapping)
        assert_eq!(to_lower(0x212A), 0x6B); // KELVIN SIGN -> k
        assert_eq!(to_upper(0xDF), 0x53); // ß -> S
        assert_eq!(to_upper(0x3C2), 0x3A3); // ς -> Σ
        assert_eq!(to_lower(0xD800), 0xD800);
        assert!(is_cased(0x41) && !is_cased(0x31));
        assert_eq!(ascii_lower(0x41), 0x61);
        assert_eq!(ascii_lower(0xC9), 0xC9);
    }
}
