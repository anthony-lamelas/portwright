//! `str` case conversions and character-class predicates (CPython 3.12 `unicodeobject.c`).

use crate::args::self_text;
use crate::error::Result;
use crate::tables::casing as t;
use crate::value::{Text, Value};

/// CPython's type record for one code point.
#[derive(Debug, Clone, Copy)]
struct Record {
    flags: u32,
    upper: i32,
    lower: i32,
    title: i32,
    fold: i32,
}

fn record(cp: u32) -> Record {
    let cp = cp.min(0x10FFFF);
    let block = usize::from(
        t::INDEX1
            .get((cp >> t::SHIFT) as usize)
            .copied()
            .unwrap_or(0),
    );
    let offset = (cp & ((1 << t::SHIFT) - 1)) as usize;
    let index = t::INDEX2
        .get((block << t::SHIFT) + offset)
        .copied()
        .unwrap_or(0);
    let (flags, upper, lower, title, fold) = t::RECORDS
        .get(usize::from(index))
        .copied()
        .unwrap_or((0, 0, 0, 0, 0));
    Record {
        flags,
        upper,
        lower,
        title,
        fold,
    }
}

fn flags(cp: u32) -> u32 {
    record(cp).flags
}

fn has(cp: u32, flag: u32) -> bool {
    flags(cp) & flag != 0
}

/// Appends the full mapping of `cp` (delta or `EXT` entry) to `out`.
fn push_mapping(out: &mut Text, cp: u32, value: i32, extended: bool) {
    if extended {
        let start = usize::try_from(value).unwrap_or(0);
        let len = t::EXT.get(start).copied().unwrap_or(0) as usize;
        if let Some(seq) = t::EXT.get(start + 1..start + 1 + len) {
            out.extend_from_slice(seq);
        }
    } else {
        // Deltas are generated as `cp - mapped`, so this never leaves 0..=0x10FFFF.
        let mapped = i64::from(cp) + i64::from(value);
        out.push(u32::try_from(mapped).unwrap_or(cp));
    }
}

fn push_upper(out: &mut Text, cp: u32) {
    let r = record(cp);
    push_mapping(out, cp, r.upper, r.flags & t::EXT_UPPER != 0);
}

fn push_lower_simple(out: &mut Text, cp: u32) {
    let r = record(cp);
    push_mapping(out, cp, r.lower, r.flags & t::EXT_LOWER != 0);
}

fn push_title(out: &mut Text, cp: u32) {
    let r = record(cp);
    push_mapping(out, cp, r.title, r.flags & t::EXT_TITLE != 0);
}

fn push_fold(out: &mut Text, cp: u32) {
    let r = record(cp);
    push_mapping(out, cp, r.fold, r.flags & t::EXT_FOLD != 0);
}

const CAPITAL_SIGMA: u32 = 0x3A3;
const FINAL_SIGMA: u32 = 0x3C2;
const SMALL_SIGMA: u32 = 0x3C3;

/// Whether the first non-case-ignorable code point of `side` is cased.
fn next_is_cased<'a>(side: impl Iterator<Item = &'a u32>) -> bool {
    side.map(|&c| flags(c))
        .find(|f| f & t::CASE_IGNORABLE == 0)
        .is_some_and(|f| f & t::CASED != 0)
}

/// CPython `handle_capital_sigma`: final sigma when preceded (skipping case-ignorables)
/// by a cased letter and not followed by one.
fn sigma_at(s: &[u32], i: usize) -> u32 {
    let before = s.get(..i).unwrap_or(&[]);
    let after = s.get(i + 1..).unwrap_or(&[]);
    if next_is_cased(before.iter().rev()) && !next_is_cased(after.iter()) {
        FINAL_SIGMA
    } else {
        SMALL_SIGMA
    }
}

/// CPython `lower_ucs4`: full lowercase with the final-sigma rule.
fn push_lower(out: &mut Text, s: &[u32], i: usize, cp: u32) {
    if cp == CAPITAL_SIGMA {
        out.push(sigma_at(s, i));
    } else {
        push_lower_simple(out, cp);
    }
}

/// `str.upper(s)`.
///
/// # Errors
/// `TypeError` when `s` is not a `str`.
pub fn upper(s: &Value) -> Result<Text> {
    let s = self_text("upper", s)?;
    let mut out = Text::with_capacity(s.len());
    for &c in s {
        push_upper(&mut out, c);
    }
    Ok(out)
}

/// `str.lower(s)`, including the final-sigma rule.
///
/// # Errors
/// `TypeError` when `s` is not a `str`.
pub fn lower(s: &Value) -> Result<Text> {
    let s = self_text("lower", s)?;
    let mut out = Text::with_capacity(s.len());
    for (i, &c) in s.iter().enumerate() {
        push_lower(&mut out, s, i, c);
    }
    Ok(out)
}

/// `str.casefold(s)` (no final-sigma rule).
///
/// # Errors
/// `TypeError` when `s` is not a `str`.
pub fn casefold(s: &Value) -> Result<Text> {
    let s = self_text("casefold", s)?;
    let mut out = Text::with_capacity(s.len());
    for &c in s {
        push_fold(&mut out, c);
    }
    Ok(out)
}

/// `str.title(s)`.
///
/// # Errors
/// `TypeError` when `s` is not a `str`.
pub fn title(s: &Value) -> Result<Text> {
    let s = self_text("title", s)?;
    let mut out = Text::with_capacity(s.len());
    let mut previous_is_cased = false;
    for (i, &c) in s.iter().enumerate() {
        // One table lookup per char: the same record gives the mapping and the flags.
        let r = record(c);
        if !previous_is_cased {
            push_mapping(&mut out, c, r.title, r.flags & t::EXT_TITLE != 0);
        } else if c == CAPITAL_SIGMA {
            out.push(sigma_at(s, i));
        } else {
            push_mapping(&mut out, c, r.lower, r.flags & t::EXT_LOWER != 0);
        }
        previous_is_cased = r.flags & t::CASED != 0;
    }
    Ok(out)
}

/// `str.capitalize(s)`: titlecase first character, lowercase the rest.
///
/// # Errors
/// `TypeError` when `s` is not a `str`.
pub fn capitalize(s: &Value) -> Result<Text> {
    let s = self_text("capitalize", s)?;
    let mut out = Text::with_capacity(s.len());
    for (i, &c) in s.iter().enumerate() {
        if i == 0 {
            push_title(&mut out, c);
        } else {
            push_lower(&mut out, s, i, c);
        }
    }
    Ok(out)
}

/// `str.swapcase(s)`.
///
/// # Errors
/// `TypeError` when `s` is not a `str`.
pub fn swapcase(s: &Value) -> Result<Text> {
    let s = self_text("swapcase", s)?;
    let mut out = Text::with_capacity(s.len());
    for (i, &c) in s.iter().enumerate() {
        let f = flags(c);
        if f & t::UPPER != 0 {
            push_lower(&mut out, s, i, c);
        } else if f & t::LOWER != 0 {
            push_upper(&mut out, c);
        } else {
            out.push(c);
        }
    }
    Ok(out)
}

/// Non-empty and every character has one of `flags`.
fn all_have(name: &str, s: &Value, flags: u32) -> Result<bool> {
    let s = self_text(name, s)?;
    Ok(!s.is_empty() && s.iter().all(|&c| has(c, flags)))
}

/// `str.isalpha(s)`.
///
/// # Errors
/// `TypeError` when `s` is not a `str`.
pub fn isalpha(s: &Value) -> Result<bool> {
    all_have("isalpha", s, t::ALPHA)
}

/// `str.isdigit(s)`.
///
/// # Errors
/// `TypeError` when `s` is not a `str`.
pub fn isdigit(s: &Value) -> Result<bool> {
    all_have("isdigit", s, t::DIGIT)
}

/// `str.isdecimal(s)`.
///
/// # Errors
/// `TypeError` when `s` is not a `str`.
pub fn isdecimal(s: &Value) -> Result<bool> {
    all_have("isdecimal", s, t::DECIMAL)
}

/// `str.isnumeric(s)`.
///
/// # Errors
/// `TypeError` when `s` is not a `str`.
pub fn isnumeric(s: &Value) -> Result<bool> {
    all_have("isnumeric", s, t::NUMERIC)
}

/// `str.isalnum(s)`.
///
/// # Errors
/// `TypeError` when `s` is not a `str`.
pub fn isalnum(s: &Value) -> Result<bool> {
    all_have("isalnum", s, t::ALPHA | t::DECIMAL | t::DIGIT | t::NUMERIC)
}

/// `str.isspace(s)` (CPython's whitespace set, `\x1c`-`\x1f` included).
///
/// # Errors
/// `TypeError` when `s` is not a `str`.
pub fn isspace(s: &Value) -> Result<bool> {
    all_have("isspace", s, t::SPACE)
}

/// CPython `_PyUnicode_IsWhitespace` for one code point.
pub fn is_whitespace(cp: u32) -> bool {
    has(cp, t::SPACE)
}

/// `str.isupper(s)`.
///
/// # Errors
/// `TypeError` when `s` is not a `str`.
pub fn isupper(s: &Value) -> Result<bool> {
    let s = self_text("isupper", s)?;
    let mut cased = false;
    for &c in s {
        let f = flags(c);
        if f & (t::LOWER | t::TITLE) != 0 {
            return Ok(false);
        }
        cased |= f & t::UPPER != 0;
    }
    Ok(cased)
}

/// `str.islower(s)`.
///
/// # Errors
/// `TypeError` when `s` is not a `str`.
pub fn islower(s: &Value) -> Result<bool> {
    let s = self_text("islower", s)?;
    let mut cased = false;
    for &c in s {
        let f = flags(c);
        if f & (t::UPPER | t::TITLE) != 0 {
            return Ok(false);
        }
        cased |= f & t::LOWER != 0;
    }
    Ok(cased)
}

/// `str.istitle(s)`.
///
/// # Errors
/// `TypeError` when `s` is not a `str`.
pub fn istitle(s: &Value) -> Result<bool> {
    let s = self_text("istitle", s)?;
    let mut cased = false;
    let mut previous_is_cased = false;
    for &c in s {
        let f = flags(c);
        if f & (t::UPPER | t::TITLE) != 0 {
            if previous_is_cased {
                return Ok(false);
            }
        } else if f & t::LOWER != 0 {
            if !previous_is_cased {
                return Ok(false);
            }
        } else {
            previous_is_cased = false;
            continue;
        }
        previous_is_cased = true;
        cased = true;
    }
    Ok(cased)
}

/// `str.isidentifier(s)`: XID_Start or `_`, then XID_Continue.
///
/// # Errors
/// `TypeError` when `s` is not a `str`.
pub fn isidentifier(s: &Value) -> Result<bool> {
    let s = self_text("isidentifier", s)?;
    Ok(match s.split_first() {
        None => false,
        Some((&first, rest)) => {
            has(first, t::ID_START) && rest.iter().all(|&c| has(c, t::ID_CONTINUE))
        }
    })
}

/// `str.isprintable(s)` (true for the empty string).
///
/// # Errors
/// `TypeError` when `s` is not a `str`.
pub fn isprintable(s: &Value) -> Result<bool> {
    let s = self_text("isprintable", s)?;
    Ok(s.iter().all(|&c| has(c, t::PRINTABLE)))
}

/// `str.isascii(s)` (true for the empty string).
///
/// # Errors
/// `TypeError` when `s` is not a `str`.
pub fn isascii(s: &Value) -> Result<bool> {
    let s = self_text("isascii", s)?;
    Ok(s.iter().all(|&c| c < 0x80))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Value {
        Value::Str(s.chars().map(u32::from).collect())
    }

    fn string(t: Result<Text>) -> String {
        t.unwrap_or_default()
            .into_iter()
            .filter_map(char::from_u32)
            .collect()
    }

    #[test]
    fn sigma_and_titles() {
        assert_eq!(string(lower(&text("ΑΣ ΣΑ Σ"))), "ας σα σ");
        assert_eq!(string(casefold(&text("ΑΣ ß"))), "ασ ss");
        assert_eq!(string(title(&text("hello wORLD ǆ"))), "Hello World ǅ");
        assert_eq!(string(capitalize(&text("ǆa"))), "ǅa");
        assert_eq!(string(upper(&text("ßﬁ"))), "SSFI");
    }

    #[test]
    fn predicates() {
        assert_eq!(istitle(&text("Hello World")), Ok(true));
        assert_eq!(isspace(&text("\x1c\u{85}")), Ok(true));
        assert_eq!(isspace(&text("\u{200b}")), Ok(false));
        assert_eq!(isidentifier(&text("_a1")), Ok(true));
        assert_eq!(isprintable(&text("")), Ok(true));
        assert_eq!(
            upper(&Value::None).map_err(|e| e.message),
            Err("descriptor 'upper' for 'str' objects doesn't apply to a 'NoneType' object".into())
        );
    }
}
