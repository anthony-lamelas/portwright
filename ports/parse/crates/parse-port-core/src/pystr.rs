//! Python `str` semantics that Rust's std does not match: character
//! predicates (`isdecimal`, `isdigit`, `isalpha`, `isalnum`, `isspace`,
//! `isprintable`, `isidentifier`, `isupper`), case mappings used by `sre`,
//! `repr(str)`, `int(str, base)` and `float(str)`.
//!
//! Unicode category data comes from the `regex` crate's tables, which may be a
//! slightly newer Unicode version than CPython 3.12 (Unicode 15.0).

use std::sync::LazyLock;

use num_bigint::BigInt;
use num_traits::{Num, Zero};
use regex::Regex;

use crate::error::Error;

/// Patterns below are fixed literals; the unit tests prove they all compile.
fn class(re: &'static str) -> Option<Regex> {
    Regex::new(re).ok()
}

static RE_ND: LazyLock<Option<Regex>> = LazyLock::new(|| class(r"\A\p{Nd}\z"));
static RE_L: LazyLock<Option<Regex>> = LazyLock::new(|| class(r"\A\p{L}\z"));
static RE_N: LazyLock<Option<Regex>> = LazyLock::new(|| class(r"\A\p{N}\z"));
static RE_LT: LazyLock<Option<Regex>> = LazyLock::new(|| class(r"\A\p{Lt}\z"));
static RE_NONPRINT: LazyLock<Option<Regex>> = LazyLock::new(|| class(r"\A[\p{C}\p{Z}]\z"));
static RE_XID_START: LazyLock<Option<Regex>> = LazyLock::new(|| class(r"\A\p{XID_Start}\z"));
static RE_XID_CONT: LazyLock<Option<Regex>> = LazyLock::new(|| class(r"\A\p{XID_Continue}\z"));

fn char_is(re: &Option<Regex>, c: char) -> bool {
    let mut buf = [0u8; 4];
    re.as_ref()
        .is_some_and(|re| re.is_match(c.encode_utf8(&mut buf)))
}

/// Characters with `Numeric_Type=Digit` that are not decimal (CPython 3.12).
const DIGIT_EXTRA: &[(u32, u32)] = &[
    (0xB2, 0xB3),
    (0xB9, 0xB9),
    (0x1369, 0x1371),
    (0x19DA, 0x19DA),
    (0x2070, 0x2070),
    (0x2074, 0x2079),
    (0x2080, 0x2089),
    (0x2460, 0x2468),
    (0x2474, 0x247C),
    (0x2488, 0x2490),
    (0x24EA, 0x24EA),
    (0x24F5, 0x24FD),
    (0x24FF, 0x24FF),
    (0x2776, 0x277E),
    (0x2780, 0x2788),
    (0x278A, 0x2792),
    (0x10A40, 0x10A43),
    (0x10E60, 0x10E68),
    (0x11052, 0x1105A),
    (0x1F100, 0x1F10A),
];

/// `c.isdecimal()` (general category Nd). This is what `sre`'s `\d` matches.
pub fn is_decimal(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_digit();
    }
    char_is(&RE_ND, c)
}

/// Decimal value of a character for which [`is_decimal`] is true.
pub fn decimal_value(c: char) -> Option<u32> {
    if c.is_ascii_digit() {
        return Some(c as u32 - '0' as u32);
    }
    if !is_decimal(c) {
        return None;
    }
    // Nd characters come in whole runs of ten, each starting at a zero.
    let mut zero = c as u32;
    while zero > 0 {
        match char::from_u32(zero - 1) {
            Some(p) if is_decimal(p) => zero -= 1,
            _ => break,
        }
    }
    Some((c as u32 - zero) % 10)
}

/// `c.isdigit()`: decimal digits plus `Numeric_Type=Digit` (e.g. `²`).
pub fn is_digit(c: char) -> bool {
    if is_decimal(c) {
        return true;
    }
    let u = c as u32;
    DIGIT_EXTRA.iter().any(|&(lo, hi)| lo <= u && u <= hi)
}

/// `c.isalpha()` (general category L*).
pub fn is_alpha(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_alphabetic();
    }
    char_is(&RE_L, c)
}

/// `c.isalnum()`: alpha, or any numeric (N*) character.
pub fn is_alnum(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_alphanumeric();
    }
    is_alpha(c) || char_is(&RE_N, c)
}

/// What `sre` matches with `\w` for `str` patterns: `isalnum() or c == '_'`.
pub fn is_word(c: char) -> bool {
    c == '_' || is_alnum(c)
}

/// `c.isspace()` (CPython 3.12 table). This is what `sre`'s `\s` matches.
pub fn is_space(c: char) -> bool {
    matches!(
        c as u32,
        0x09..=0x0D
            | 0x1C..=0x20
            | 0x85
            | 0xA0
            | 0x1680
            | 0x2000..=0x200A
            | 0x2028
            | 0x2029
            | 0x202F
            | 0x205F
            | 0x3000
    )
}

/// `c.isprintable()`.
pub fn is_printable(c: char) -> bool {
    if c == ' ' {
        return true;
    }
    if c.is_ascii() {
        return c.is_ascii_graphic();
    }
    !char_is(&RE_NONPRINT, c)
}

/// `c.istitle()` for a single character (general category Lt).
pub fn is_titlecase(c: char) -> bool {
    !c.is_ascii() && char_is(&RE_LT, c)
}

/// `s.isidentifier()`, which `sre` uses to validate group names.
pub fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        None => false,
        Some(first) => {
            (first == '_' || char_is(&RE_XID_START, first))
                && chars.all(|c| char_is(&RE_XID_CONT, c))
        }
    }
}

/// Simple (single-character) lowercase mapping, as `sre` uses for IGNORECASE.
pub fn simple_lower(c: char) -> char {
    if c.is_ascii() {
        return c.to_ascii_lowercase();
    }
    if c == '\u{130}' {
        return 'i';
    }
    let mut it = c.to_lowercase();
    match (it.next(), it.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

/// Simple (single-character) uppercase mapping. Approximation: characters
/// whose full uppercase mapping is several characters map to themselves.
pub fn simple_upper(c: char) -> char {
    if c.is_ascii() {
        return c.to_ascii_uppercase();
    }
    let mut it = c.to_uppercase();
    match (it.next(), it.next()) {
        (Some(u), None) => u,
        _ => c,
    }
}

/// `s.lower()` (full mapping, final sigma aware).
pub fn lower(s: &str) -> String {
    s.to_lowercase()
}

/// `s.upper()` (full mapping).
pub fn upper(s: &str) -> String {
    s.to_uppercase()
}

fn title_char(c: char) -> String {
    match c {
        '\u{1C4}'..='\u{1C6}' => "\u{1C5}".into(),
        '\u{1C7}'..='\u{1C9}' => "\u{1C8}".into(),
        '\u{1CA}'..='\u{1CC}' => "\u{1CB}".into(),
        '\u{1F1}'..='\u{1F3}' => "\u{1F2}".into(),
        _ => c.to_uppercase().collect(),
    }
}

/// `s.capitalize()` (first character titlecased, the rest lowercased).
pub fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => {
            let mut out = title_char(first);
            let rest: String = chars.collect();
            // Lowercasing the rest on its own keeps the final-sigma rule local to it,
            // as CPython does (it looks at neighbours inside the whole string; the
            // difference only shows for a leading cased letter before a sigma).
            let lowered = lower(&format!("{first}{rest}"));
            let first_lower_len: usize = first.to_lowercase().map(char::len_utf8).sum();
            out.push_str(&lowered[first_lower_len..]);
            out
        }
    }
}

/// `s.isupper()`.
pub fn is_upper(s: &str) -> bool {
    let mut cased = false;
    for c in s.chars() {
        if c.is_lowercase() || is_titlecase(c) {
            return false;
        }
        if c.is_uppercase() {
            cased = true;
        }
    }
    cased
}

/// `repr(s)` for a Python `str`.
pub fn repr(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7F => {
                out.push_str(&format!("\\x{:02x}", c as u32))
            }
            c if c.is_ascii() || is_printable(c) => out.push(c),
            c if (c as u32) < 0x100 => out.push_str(&format!("\\x{:02x}", c as u32)),
            c if (c as u32) < 0x10000 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push_str(&format!("\\U{:08x}", c as u32)),
        }
    }
    out.push(quote);
    out
}

/// CPython's `_PyUnicode_TransformDecimalAndSpaceToASCII`: Unicode spaces
/// become `' '`, Unicode decimal digits become ASCII. `None` if any other
/// non-ASCII character is present (CPython then fails the conversion).
fn to_ascii_numeric(s: &str) -> Option<String> {
    s.chars()
        .map(|c| {
            if c.is_ascii() {
                Some(c)
            } else if is_space(c) {
                Some(' ')
            } else {
                decimal_value(c).and_then(|d| char::from_digit(d, 10))
            }
        })
        .collect()
}

fn is_ascii_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c')
}

/// Python's limit from `sys.get_int_max_str_digits()` (default 4300).
pub const INT_MAX_STR_DIGITS: usize = 4300;

/// `int(s, base)` for a `str` argument, including error messages.
///
/// Supports bases 0 and 2..=36, Unicode decimal digits, surrounding
/// whitespace, a sign, base prefixes and single underscores between digits.
pub fn py_int(s: &str, base: u32) -> Result<BigInt, Error> {
    if base == 1 || base > 36 {
        return Err(Error::Value(
            "int() base must be >= 2 and <= 36, or 0".into(),
        ));
    }
    let invalid = || {
        let shown: String = s.chars().take(200).collect();
        Error::Value(format!(
            "invalid literal for int() with base {}: {}",
            base,
            repr(&shown)
        ))
    };
    let ascii = to_ascii_numeric(s).ok_or_else(invalid)?;
    let t = ascii.trim_matches(is_ascii_space);
    let (neg, mut body) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let mut base = base;
    let lower2: String = body
        .chars()
        .take(2)
        .collect::<String>()
        .to_ascii_lowercase();
    let prefix_base = match lower2.as_str() {
        "0x" => Some(16),
        "0o" => Some(8),
        "0b" => Some(2),
        _ => None,
    };
    let mut had_prefix = false;
    if let Some(pb) = prefix_base {
        if base == 0 || base == pb {
            base = pb;
            body = &body[2..];
            had_prefix = true;
        }
    }
    let base0 = base == 0;
    if base0 {
        base = 10;
    }
    // Underscore rules: single underscores between digits; one may directly
    // follow a base prefix.
    let mut digits = String::with_capacity(body.len());
    let mut prev_underscore = false;
    for (i, c) in body.chars().enumerate() {
        if c == '_' {
            if prev_underscore || (i == 0 && !had_prefix) {
                return Err(invalid());
            }
            prev_underscore = true;
            continue;
        }
        if !c.is_digit(base) {
            return Err(invalid());
        }
        prev_underscore = false;
        digits.push(c);
    }
    if digits.is_empty() || prev_underscore {
        return Err(invalid());
    }
    if base0 && digits.len() > 1 && digits.starts_with('0') && digits.chars().any(|c| c != '0') {
        return Err(invalid());
    }
    if !base.is_power_of_two() && digits.len() > INT_MAX_STR_DIGITS {
        return Err(Error::Value(format!(
            "Exceeds the limit ({}) for integer string conversion: value has {} digits; \
             use sys.set_int_max_str_digits() to increase the limit",
            INT_MAX_STR_DIGITS,
            digits.len()
        )));
    }
    let value = BigInt::from_str_radix(&digits, base).map_err(|_| invalid())?;
    Ok(if neg && !value.is_zero() {
        -value
    } else {
        value
    })
}

/// `float(s)` for a `str` argument, including the error message.
pub fn py_float(s: &str) -> Result<f64, Error> {
    let invalid = || Error::Value(format!("could not convert string to float: {}", repr(s)));
    let ascii = to_ascii_numeric(s).ok_or_else(invalid)?;
    let t = ascii.trim_matches(is_ascii_space);
    // Underscores are only allowed between two digits.
    let chars: Vec<char> = t.chars().collect();
    let mut cleaned = String::with_capacity(t.len());
    for (i, &c) in chars.iter().enumerate() {
        if c == '_' {
            let before = i > 0 && chars[i - 1].is_ascii_digit();
            let after = chars.get(i + 1).is_some_and(char::is_ascii_digit);
            if !(before && after) {
                return Err(invalid());
            }
            continue;
        }
        cleaned.push(c);
    }
    let unsigned = cleaned.strip_prefix(['+', '-']).unwrap_or(&cleaned);
    let lowered = unsigned.to_ascii_lowercase();
    if matches!(lowered.as_str(), "inf" | "infinity" | "nan") {
        let neg = cleaned.starts_with('-');
        let v = if lowered == "nan" {
            f64::NAN
        } else {
            f64::INFINITY
        };
        return Ok(if neg { -v } else { v });
    }
    // Rust accepts exactly Python's decimal grammar for finite values, but also
    // spellings like "infinity" handled above; reject anything non-numeric.
    if !unsigned
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'))
    {
        return Err(invalid());
    }
    cleaned.parse::<f64>().map_err(|_| invalid())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_compile() {
        for re in [
            &RE_ND,
            &RE_L,
            &RE_N,
            &RE_LT,
            &RE_NONPRINT,
            &RE_XID_START,
            &RE_XID_CONT,
        ] {
            assert!(re.is_some());
        }
    }

    #[test]
    fn predicates() {
        assert!(is_decimal('٣'));
        assert!(!is_decimal('²'));
        assert!(is_digit('²'));
        assert!(is_word('é') && is_word('_') && !is_word('\u{301}'));
        assert!(is_space('\x1c') && !is_space('\u{200b}'));
        assert!(is_identifier("café") && !is_identifier("a²") && !is_identifier("1a"));
        assert_eq!(decimal_value('٣'), Some(3));
        assert_eq!(simple_lower('\u{130}'), 'i');
    }

    #[test]
    fn repr_matches_python() {
        assert_eq!(repr("abc"), "'abc'");
        assert_eq!(repr("a'b"), "\"a'b\"");
        assert_eq!(repr("a'\"b"), "'a\\'\"b'");
        assert_eq!(repr("\n\x01é\u{200b}"), "'\\n\\x01é\\u200b'");
    }

    #[test]
    fn int_and_float() {
        assert_eq!(py_int("١٢", 10).unwrap(), BigInt::from(12));
        assert_eq!(py_int(" -0x1_f ", 16).unwrap(), BigInt::from(-31));
        assert_eq!(
            py_int("", 10).unwrap_err(),
            Error::Value("invalid literal for int() with base 10: ''".into())
        );
        assert!(py_int("1__0", 10).is_err());
        assert_eq!(py_float(" 1_0.5 ").unwrap(), 10.5);
        assert!(py_float("-INF").unwrap().is_infinite());
        assert_eq!(py_float("١.٥").unwrap(), 1.5);
        assert_eq!(
            py_float("x").unwrap_err(),
            Error::Value("could not convert string to float: 'x'".into())
        );
        assert!(py_float("1_").is_err() && py_float("").is_err() && py_float("in f").is_err());
    }
}
