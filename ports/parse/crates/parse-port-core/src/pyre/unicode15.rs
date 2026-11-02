//! Python `str` semantics pinned to CPython 3.12's Unicode 15.0.
//!
//! Same API as [`crate::pystr`], which is frozen and takes its Unicode data
//! from the `regex` crate (16.0) and std (17.0). Code points that 15.0 leaves
//! unassigned act as `Cn` here: not printable (so [`repr`] escapes them), not
//! alphanumeric, decimal or identifier characters, uncased, and rejected by
//! [`py_int`] / [`py_float`]. Case mappings whose result is such a code point
//! map to themselves. Use `use crate::pyre::unicode15 as pystr;`.

use std::cmp::Ordering;

use num_bigint::BigInt;

use super::unicode15_tables::{
    CASED_STOP, CASE_IGNORABLE, LOWERCASE, TITLE, UNASSIGNED, UPPERCASE,
};
use crate::error::Error;
use crate::pystr as newer;

pub use crate::pystr::{is_space, INT_MAX_STR_DIGITS};

/// `XID_Continue` additions of Unicode 15.1 (not identifier characters in 3.12).
const XID_CONTINUE_15_1: [char; 4] = ['\u{200C}', '\u{200D}', '\u{30FB}', '\u{FF65}'];

fn in_table(table: &[(u32, u32)], c: char) -> bool {
    let u = c as u32;
    table
        .binary_search_by(|&(lo, hi)| {
            if hi < u {
                Ordering::Less
            } else if lo > u {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        })
        .is_ok()
}

/// True for code points CPython 3.12 sees as unassigned (`Cn`), whatever the
/// newer tables in `regex`/std say about them.
pub fn is_newer_than_py(c: char) -> bool {
    // Every ASCII code point is assigned in all Unicode versions.
    !c.is_ascii() && in_table(UNASSIGNED, c)
}

/// `c.isdecimal()` (general category Nd). This is what `sre`'s `\d` matches.
pub fn is_decimal(c: char) -> bool {
    !is_newer_than_py(c) && newer::is_decimal(c)
}

/// `c.isdigit()`.
pub fn is_digit(c: char) -> bool {
    !is_newer_than_py(c) && newer::is_digit(c)
}

/// `c.isalpha()`.
pub fn is_alpha(c: char) -> bool {
    !is_newer_than_py(c) && newer::is_alpha(c)
}

/// `c.isalnum()`.
pub fn is_alnum(c: char) -> bool {
    !is_newer_than_py(c) && newer::is_alnum(c)
}

/// What `sre` matches with `\w` for `str` patterns: `isalnum() or c == '_'`.
pub fn is_word(c: char) -> bool {
    c == '_' || is_alnum(c)
}

/// `c.isprintable()`.
pub fn is_printable(c: char) -> bool {
    !is_newer_than_py(c) && newer::is_printable(c)
}

/// `s.isidentifier()`, which `sre` uses to validate group names.
pub fn is_identifier(s: &str) -> bool {
    !s.chars()
        .any(|c| is_newer_than_py(c) || XID_CONTINUE_15_1.contains(&c))
        && newer::is_identifier(s)
}

/// Simple lowercase mapping, as `sre` uses for IGNORECASE.
pub fn simple_lower(c: char) -> char {
    if is_newer_than_py(c) {
        return c;
    }
    let l = newer::simple_lower(c);
    if is_newer_than_py(l) {
        c
    } else {
        l
    }
}

/// Simple uppercase mapping (multi-character mappings map to themselves).
pub fn simple_upper(c: char) -> char {
    if is_newer_than_py(c) {
        return c;
    }
    let u = newer::simple_upper(c);
    if is_newer_than_py(u) {
        c
    } else {
        u
    }
}

/// A std full case mapping, unless it involves a post-15.0 code point.
fn masked(c: char, mapped: impl Iterator<Item = char>) -> String {
    if is_newer_than_py(c) {
        return c.to_string();
    }
    let m: String = mapped.collect();
    if m.chars().any(is_newer_than_py) {
        c.to_string()
    } else {
        m
    }
}

/// Full uppercase mapping of one character (`_PyUnicode_ToUpperFull`).
pub fn upper_char(c: char) -> String {
    masked(c, c.to_uppercase())
}

/// `s.upper()` (full mapping).
pub fn upper(s: &str) -> String {
    if s.is_ascii() {
        return s.to_ascii_uppercase();
    }
    s.chars().map(upper_char).collect()
}

/// CPython's `handle_capital_sigma`: is the `Σ` at `i` word-final?
fn final_sigma(chars: &[char], i: usize) -> bool {
    let is_cased = |c: char| in_table(CASED_STOP, c);
    let is_ignorable = |c: char| in_table(CASE_IGNORABLE, c);
    let before = chars[..i].iter().rev().find(|&&c| !is_ignorable(c));
    if !before.is_some_and(|&c| is_cased(c)) {
        return false;
    }
    let after = chars[i + 1..].iter().find(|&&c| !is_ignorable(c));
    !after.is_some_and(|&c| is_cased(c))
}

/// Lowercases `chars[start..]`, with final-sigma context from all of `chars`.
fn lower_from(chars: &[char], start: usize) -> String {
    let mut out = String::with_capacity(chars.len());
    for (i, &c) in chars.iter().enumerate().skip(start) {
        if c == '\u{3A3}' {
            out.push(if final_sigma(chars, i) {
                '\u{3C2}'
            } else {
                '\u{3C3}'
            });
        } else {
            out.push_str(&masked(c, c.to_lowercase()));
        }
    }
    out
}

/// `s.lower()` (full mapping, final sigma aware).
pub fn lower(s: &str) -> String {
    if s.is_ascii() {
        return s.to_ascii_lowercase();
    }
    let chars: Vec<char> = s.chars().collect();
    lower_from(&chars, 0)
}

/// `s.capitalize()`: first character to full titlecase, the rest lowercased
/// (final-sigma context still sees the first character).
pub fn capitalize(s: &str) -> String {
    if s.is_ascii() {
        let mut out = s.to_ascii_lowercase();
        if let Some(first) = out.get_mut(..1) {
            first.make_ascii_uppercase();
        }
        return out;
    }
    let chars: Vec<char> = s.chars().collect();
    let Some(&first) = chars.first() else {
        return String::new();
    };
    let mut out = match TITLE.binary_search_by(|&(k, _)| k.cmp(&first)) {
        Ok(idx) => TITLE
            .get(idx)
            .map_or_else(String::new, |&(_, t)| t.to_owned()),
        Err(_) => upper_char(first),
    };
    out.push_str(&lower_from(&chars, 1));
    out
}

/// `s.isupper()`: no lowercase or titlecase characters, and at least one
/// uppercase one (Unicode 15.0 properties).
pub fn is_upper(s: &str) -> bool {
    if s.is_ascii() {
        let b = s.as_bytes();
        return !b.iter().any(u8::is_ascii_lowercase) && b.iter().any(u8::is_ascii_uppercase);
    }
    let mut cased = false;
    for c in s.chars() {
        if in_table(LOWERCASE, c) || (!is_newer_than_py(c) && newer::is_titlecase(c)) {
            return false;
        }
        cased |= in_table(UPPERCASE, c);
    }
    cased
}

/// Offset from a lone surrogate (U+D800..U+DFFF, not a Rust `char`) to the
/// code point standing in for it: U+DD800..U+DDFFF, unassigned in every
/// Unicode version, so all `str` predicates treat it exactly like `Cs`.
const STANDIN_OFFSET: u32 = 0xD_0000;

/// The `char` standing in for the lone surrogate `cp`, if `cp` is one.
pub fn surrogate_standin(cp: u32) -> Option<char> {
    if (0xD800..=0xDFFF).contains(&cp) {
        char::from_u32(cp + STANDIN_OFFSET)
    } else {
        None
    }
}

/// The lone surrogate that `c` stands in for, if any.
pub fn standin_surrogate(c: char) -> Option<u32> {
    let cp = (c as u32).checked_sub(STANDIN_OFFSET)?;
    (0xD800..=0xDFFF).contains(&cp).then_some(cp)
}

/// Python `repr(s)`; surrogate stand-ins print as `'\udxxx'`.
pub fn repr(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        if let Some(cp) = standin_surrogate(c) {
            out.push_str(&format!("\\u{cp:04x}"));
            continue;
        }
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if is_printable(c) => out.push(c),
            c if (c as u32) < 0x100 => out.push_str(&format!("\\x{:02x}", c as u32)),
            c if (c as u32) < 0x10000 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push_str(&format!("\\U{:08x}", c as u32)),
        }
    }
    out.push(quote);
    out
}

/// `int(s, base)` for a `str` argument, including error messages.
pub fn py_int(s: &str, base: u32) -> Result<BigInt, Error> {
    // Plain ASCII digits of an explicit base that fit a u64: no sign, space,
    // underscore or prefix (a prefix letter is either not a digit of `base`,
    // or names another base and is then read as a digit, as `int` does).
    if (2..=36).contains(&base) && !s.is_empty() && s.len() <= 64 {
        let v = s.chars().try_fold(0u64, |acc, c| {
            acc.checked_mul(u64::from(base))?
                .checked_add(u64::from(c.to_digit(base)?))
        });
        if let Some(v) = v {
            return Ok(BigInt::from(v));
        }
    }
    if base != 1 && base <= 36 && s.chars().any(is_newer_than_py) {
        let shown: String = s.chars().take(200).collect();
        return Err(Error::Value(format!(
            "invalid literal for int() with base {}: {}",
            base,
            repr(&shown)
        )));
    }
    newer::py_int(s, base)
}

/// `float(s)` for a `str` argument, including the error message.
pub fn py_float(s: &str) -> Result<f64, Error> {
    if s.chars().any(is_newer_than_py) {
        return Err(Error::Value(format!(
            "could not convert string to float: {}",
            repr(s)
        )));
    }
    newer::py_float(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_15_code_points_act_as_unassigned() {
        assert_eq!(repr("\u{1cc00}\u{897}é"), "'\\U0001cc00\\u0897é'");
        assert!(!is_alpha('\u{a7cb}') && !is_decimal('\u{10d40}'));
        assert_eq!(simple_upper('\u{264}'), '\u{264}');
        assert_eq!(upper("\u{19b}a"), "\u{19b}A");
        assert!(!is_identifier("a\u{200d}") && is_identifier("café"));
        assert!(py_int("\u{10d41}", 10).is_err() && py_float("\u{10d41}").is_err());
    }

    #[test]
    fn ascii_fast_paths_match_general_paths() {
        let alphabet: Vec<char> = (0u8..128).map(char::from).collect();
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let digits = "0123456789abcdefxzXOB+-_ ";
        for round in 0..40_000 {
            let len = usize::try_from(next() % 22).unwrap_or(0);
            let pool: Vec<char> = if round % 2 == 0 {
                alphabet.clone()
            } else {
                digits.chars().collect()
            };
            let s: String = (0..len)
                .map(|_| pool[usize::try_from(next()).unwrap_or(0) % pool.len()])
                .collect();
            let chars: Vec<char> = s.chars().collect();
            assert_eq!(
                upper(&s),
                chars.iter().map(|&c| upper_char(c)).collect::<String>()
            );
            assert_eq!(lower(&s), lower_from(&chars, 0));
            let cap = match chars.first() {
                None => String::new(),
                Some(&f) => upper_char(f) + &lower_from(&chars, 1),
            };
            assert_eq!(capitalize(&s), cap);
            let general = !chars
                .iter()
                .any(|&c| in_table(LOWERCASE, c) || newer::is_titlecase(c))
                && chars.iter().any(|&c| in_table(UPPERCASE, c));
            assert_eq!(is_upper(&s), general);
            for base in [0, 2, 8, 10, 11, 12, 16, 33, 34, 36] {
                assert_eq!(py_int(&s, base), newer::py_int(&s, base), "{s:?} {base}");
            }
        }
        assert_eq!(
            py_int("999999999999999999", 10),
            newer::py_int("999999999999999999", 10)
        );
        assert_eq!(py_int("0000", 10), newer::py_int("0000", 10));
        for s in [
            "18446744073709551615",
            "18446744073709551616",
            "zzzzzzzzzzzzzz",
            "0x1f",
            "0b1",
        ] {
            for base in [2, 10, 16, 36] {
                assert_eq!(py_int(s, base), newer::py_int(s, base), "{s:?} {base}");
            }
        }
    }

    #[test]
    fn case_mappings() {
        assert_eq!(lower("ΑΣ"), "ας");
        assert_eq!(lower("ΑΣ\u{2b0}"), "ας\u{2b0}");
        assert_eq!(capitalize("ßAΣ"), "Ssaς");
        assert_eq!(capitalize("ǆa"), "ǅa");
        assert!(is_upper("AB\u{1cc00}") && !is_upper("Ab") && !is_upper("\u{295}A"));
    }
}
