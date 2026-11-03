//! `unicodedata.name` and `unicodedata.lookup` (Unicode 15.0, CPython 3.12).
//!
//! Names come from three places, as in CPython: algorithmic Hangul syllables and CJK unified
//! ideographs, the compressed name table in [`table`], and (for `lookup`
//! only) name aliases and named sequences.

#[rustfmt::skip]
mod table;

use std::fmt;
use std::sync::OnceLock;

use self::table as t;
use crate::args::bad_arg_type_name;
use crate::error::Error;
use crate::hangul::{N_COUNT, S_BASE, S_COUNT, T_COUNT, V_COUNT};
use crate::value::{Text, Value};

/// CPython's `NAME_MAXLEN`: longer `lookup` arguments (in UTF-8 bytes) are rejected.
const NAME_MAXLEN: usize = 256;

const HANGUL_PREFIX: &[u8] = b"HANGUL SYLLABLE ";
const CJK_PREFIX: &[u8] = b"CJK UNIFIED IDEOGRAPH-";
const CJK_UNIFIED: &str = "CJK UNIFIED IDEOGRAPH";

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

/// Why `lookup` failed: an ordinary Python exception, or the `UnicodeEncodeError` CPython
/// raises while converting a `str` holding lone surrogates to UTF-8.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LookupError {
    /// A `TypeError` or `KeyError`.
    Python(Error),
    /// `UnicodeEncodeError('utf-8', name, start, end, 'surrogates not allowed')`.
    Surrogates {
        /// Index of the first lone surrogate.
        start: usize,
        /// Index just past the run of surrogates starting at `start`.
        end: usize,
    },
}

impl LookupError {
    /// The `reason` argument of the `UnicodeEncodeError`.
    pub const SURROGATE_REASON: &'static str = "surrogates not allowed";
}

impl fmt::Display for LookupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Python(e) => e.fmt(f),
            Self::Surrogates { start, end } => {
                write!(f, "surrogates not allowed at {start}..{end}")
            }
        }
    }
}

impl std::error::Error for LookupError {}

impl From<Error> for LookupError {
    fn from(e: Error) -> Self {
        Self::Python(e)
    }
}

/// The decoded name table: names in code point order plus an index sorted by name.
#[derive(Debug, Default)]
struct NameTable {
    names: Vec<(u32, Box<str>)>,
    by_name: Vec<u32>,
}

impl NameTable {
    fn name_of(&self, cp: u32) -> Option<&str> {
        let i = self.names.binary_search_by_key(&cp, |&(c, _)| c).ok()?;
        self.names.get(i).map(|(_, n)| &**n)
    }

    fn code_of(&self, upper: &str) -> Option<u32> {
        let entry = |i: &u32| self.names.get(usize::try_from(*i).ok()?);
        let pos = self
            .by_name
            .binary_search_by(|i| match entry(i) {
                Some((_, n)) => (**n).cmp(upper),
                None => std::cmp::Ordering::Less,
            })
            .ok()?;
        self.by_name.get(pos).and_then(entry).map(|&(c, _)| c)
    }
}

fn table() -> &'static NameTable {
    static TABLE: OnceLock<NameTable> = OnceLock::new();
    // A decode failure is caught by the `table_decodes_every_name` test.
    TABLE.get_or_init(|| decode_table().unwrap_or_default())
}

/// Value of one symbol of the generated alphabet (printable ASCII without `"`).
fn symbol(b: u8) -> Option<usize> {
    let v = match b {
        0x20 | 0x21 => b - 0x20,
        0x23..=0x7E => b - 0x21,
        _ => return None,
    };
    Some(usize::from(v))
}

/// Symbols below this rank are one-symbol word codes (must match `gen_names.py`).
const ONE: usize = 64;
/// Lead symbols `ONE..ONE + TWO_LEADS` start two-symbol word codes.
const TWO_LEADS: usize = 28;
/// Header `k * HDR_N + n` (shared words, new words); `ESCAPE` spells both out.
const HDR_N: usize = 9;
const ESCAPE: usize = 90;
const BASE: usize = 94;

/// Decodes `NAME_DATA`: per name a header (words shared with the previous name, new word
/// count) followed by the new words' dictionary ranks.
fn decode_table() -> Option<NameTable> {
    let words: Vec<&str> = t::WORDS.split(' ').collect();
    let mut data = t::NAME_DATA.bytes();
    let mut next = || data.next().and_then(symbol);
    let mut jumps = t::NAME_JUMPS.iter().peekable();
    let mut names: Vec<(u32, Box<str>)> = Vec::with_capacity(t::NAME_COUNT);
    let mut prev: Vec<&str> = Vec::new();
    let mut cp = 0u32;
    for index in 0..t::NAME_COUNT {
        cp = match jumps.peek() {
            Some(&&(at, jump)) if usize::from(at) == index => {
                jumps.next();
                jump
            }
            _ => cp.checked_add(1)?,
        };
        let header = next()?;
        let (shared, new) = if header == ESCAPE {
            (next()?, next()?)
        } else {
            (header / HDR_N, header % HDR_N)
        };
        prev.truncate(shared);
        for _ in 0..new {
            let lead = next()?;
            let rank = if lead < ONE {
                lead
            } else if lead < ONE + TWO_LEADS {
                ONE + (lead - ONE) * BASE + next()?
            } else {
                let hi = next()?;
                let lo = next()?;
                ONE + TWO_LEADS * BASE + ((lead - ONE - TWO_LEADS) * BASE + hi) * BASE + lo
            };
            prev.push(words.get(rank)?);
        }
        names.push((cp, prev.join(" ").into_boxed_str()));
    }
    let mut by_name: Vec<u32> = (0..names.len())
        .map(u32::try_from)
        .collect::<Result<_, _>>()
        .ok()?;
    by_name.sort_unstable_by(|&a, &b| {
        let key = |i: u32| {
            usize::try_from(i)
                .ok()
                .and_then(|i| names.get(i))
                .map(|(_, n)| &**n)
        };
        key(a).cmp(&key(b))
    });
    Some(NameTable { names, by_name })
}

fn is_unified_ideograph(cp: u32) -> bool {
    t::HEX_RANGES
        .iter()
        .any(|&(p, first, last)| p == CJK_UNIFIED && (first..=last).contains(&cp))
}

fn jamo(table: &[&'static str], i: u32) -> Option<&'static str> {
    table.get(usize::try_from(i).ok()?).copied()
}

/// The name of `cp` as `unicodedata.name` reports it, if it has one.
fn name_of(cp: u32) -> Option<String> {
    if (S_BASE..S_BASE + S_COUNT).contains(&cp) {
        let s = cp - S_BASE;
        let l = jamo(&JAMO_L, s / N_COUNT)?;
        let v = jamo(&JAMO_V, (s % N_COUNT) / T_COUNT)?;
        let t = jamo(&JAMO_T, s % T_COUNT)?;
        return Some(format!("HANGUL SYLLABLE {l}{v}{t}"));
    }
    if let Some(&(prefix, _, _)) = t::HEX_RANGES
        .iter()
        .find(|&&(_, first, last)| (first..=last).contains(&cp))
    {
        return Some(format!("{prefix}-{cp:X}"));
    }
    table().name_of(cp).map(str::to_owned)
}

/// Upstream `char_name(ch, default=None)` = `unicodedata.name`.
/// `Ok(None)` means the caller must return `default` unchanged.
///
/// # Errors
/// `TypeError` unless `ch` is a one-character `str`; `ValueError('no such name')` when
/// `ch` has no name and `default` is missing or `None`.
pub fn char_name(ch: &Value, default: Option<&Value>) -> crate::Result<Option<String>> {
    let cp = match ch {
        Value::Str(s) if s.len() == 1 => s.first().copied().unwrap_or_default(),
        other => {
            return Err(Error::type_error(format!(
                "name() argument 1 must be a unicode character, not {}",
                bad_arg_type_name(other)
            )));
        }
    };
    match name_of(cp) {
        Some(n) => Ok(Some(n)),
        None if default.is_some_and(|d| *d != Value::None) => Ok(None),
        None => Err(Error::value_error("no such name")),
    }
}

/// UTF-8 encoding of `text`, or the span of the first run of lone surrogates.
fn utf8(text: &[u32]) -> Result<Vec<u8>, LookupError> {
    let mut out = String::with_capacity(text.len());
    for (i, &cp) in text.iter().enumerate() {
        match char::from_u32(cp) {
            Some(c) => out.push(c),
            None => {
                let run = text
                    .get(i..)
                    .unwrap_or_default()
                    .iter()
                    .take_while(|&&c| (0xD800..=0xDFFF).contains(&c))
                    .count()
                    .max(1);
                return Err(LookupError::Surrogates {
                    start: i,
                    end: i + run,
                });
            }
        }
    }
    Ok(out.into_bytes())
}

/// CPython's `find_syllable`: longest (case-sensitive) jamo name at the start of `s`.
/// Ties keep the first entry; an empty jamo always matches. Returns `(len, index)`.
fn find_syllable(s: &[u8], table: &[&str]) -> (usize, Option<u32>) {
    let mut best: Option<(usize, u32)> = None;
    for (i, jamo) in (0u32..).zip(table) {
        let len = jamo.len();
        if best.is_some_and(|(l, _)| len <= l) {
            continue;
        }
        if s.starts_with(jamo.as_bytes()) {
            best = Some((len, i));
        }
    }
    best.map_or((0, None), |(l, i)| (l, Some(i)))
}

fn hangul_code(rest: &[u8]) -> Option<u32> {
    let (l_len, l) = find_syllable(rest, &JAMO_L);
    let rest = rest.get(l_len..)?;
    let (v_len, v) = find_syllable(rest, &JAMO_V);
    let rest = rest.get(v_len..)?;
    let (t_len, t) = find_syllable(rest, &JAMO_T);
    if t_len != rest.len() {
        return None;
    }
    Some(S_BASE + (l? * V_COUNT + v?) * T_COUNT + t?)
}

fn cjk_code(digits: &[u8]) -> Option<u32> {
    if digits.len() != 4 && digits.len() != 5 {
        return None;
    }
    let mut v = 0u32;
    for &d in digits {
        let digit = match d {
            b'0'..=b'9' => d - b'0',
            b'A'..=b'F' => d - b'A' + 10,
            _ => return None,
        };
        v = v * 16 + u32::from(digit);
    }
    is_unified_ideograph(v).then_some(v)
}

/// Phrasebook names of the form `PREFIX-%X` (CJK compatibility, Khitan, Nushu).
fn hex_range_code(upper: &str) -> Option<u32> {
    t::HEX_RANGES
        .iter()
        .filter(|&&(p, _, _)| p != CJK_UNIFIED)
        .find_map(|&(prefix, first, last)| {
            let hex = upper.strip_prefix(prefix)?.strip_prefix('-')?;
            let cp = u32::from_str_radix(hex, 16).ok()?;
            ((first..=last).contains(&cp) && format!("{cp:X}") == hex).then_some(cp)
        })
}

/// CPython's `_getcode` with named sequences enabled.
fn code_of(name: &[u8]) -> Option<Text> {
    if let Some(rest) = name.strip_prefix(HANGUL_PREFIX) {
        return hangul_code(rest).map(|c| vec![c]);
    }
    if let Some(rest) = name.strip_prefix(CJK_PREFIX) {
        return cjk_code(rest).map(|c| vec![c]);
    }
    // Every stored name is ASCII; CPython folds the argument with ASCII-only `Py_TOUPPER`.
    let upper = std::str::from_utf8(name).ok()?.to_ascii_uppercase();
    if let Some(cp) = table().code_of(&upper).or_else(|| hex_range_code(&upper)) {
        return Some(vec![cp]);
    }
    if let Some(&(_, cp)) = t::ALIASES.iter().find(|&&(n, _)| n == upper) {
        return Some(vec![cp]);
    }
    t::NAMED_SEQUENCES
        .iter()
        .find(|&&(n, _)| n == upper)
        .map(|&(_, seq)| seq.to_vec())
}

/// Upstream `lookup(name)` = `unicodedata.lookup`: the character (or named sequence)
/// called `name`. `name` may be `str` or `bytes`.
///
/// # Errors
/// `TypeError` for other types; `UnicodeEncodeError` (as [`LookupError::Surrogates`]) for
/// lone surrogates; `KeyError('name too long')` beyond 256 UTF-8 bytes; otherwise
/// `KeyError("undefined character name '...'")`, the name cut at its first NUL.
pub fn lookup(name: &Value) -> Result<Text, LookupError> {
    let bytes = match name {
        Value::Str(s) => utf8(s)?,
        Value::Bytes(b) => b.clone(),
        Value::ByteArray(_) => {
            return Err(Error::type_error(
                "lookup() argument must be read-only bytes-like object, not bytearray",
            )
            .into())
        }
        other => {
            return Err(Error::type_error(format!(
                "a bytes-like object is required, not '{}'",
                other.type_name()
            ))
            .into())
        }
    };
    if bytes.len() > NAME_MAXLEN {
        return Err(Error::key_error("name too long").into());
    }
    code_of(&bytes).ok_or_else(|| {
        let shown = bytes.split(|&b| b == 0).next().unwrap_or_default();
        Error::key_error(format!(
            "undefined character name '{}'",
            String::from_utf8_lossy(shown)
        ))
        .into()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> Value {
        Value::Str(text.chars().map(u32::from).collect())
    }

    fn found(name: &str) -> Option<Text> {
        lookup(&s(name)).ok()
    }

    #[test]
    fn table_decodes_every_name() {
        assert_eq!(table().names.len(), t::NAME_COUNT);
    }

    #[test]
    fn names() {
        let n = |c: u32| char_name(&Value::Str(vec![c]), None).ok().flatten();
        assert_eq!(n(0x61).as_deref(), Some("LATIN SMALL LETTER A"));
        assert_eq!(n(0xAC00).as_deref(), Some("HANGUL SYLLABLE GA"));
        assert_eq!(n(0xD7A3).as_deref(), Some("HANGUL SYLLABLE HIH"));
        assert_eq!(n(0x4E00).as_deref(), Some("CJK UNIFIED IDEOGRAPH-4E00"));
        assert_eq!(
            n(0xF900).as_deref(),
            Some("CJK COMPATIBILITY IDEOGRAPH-F900")
        );
        assert_eq!(n(0x2014).as_deref(), Some("EM DASH"));
        assert_eq!(n(0x1F600).as_deref(), Some("GRINNING FACE"));
        assert_eq!(n(0x17000), None);
        assert!(char_name(&Value::Str(vec![0]), None).is_err());
        assert_eq!(char_name(&Value::Str(vec![0]), Some(&s("?"))), Ok(None));
    }

    #[test]
    fn lookups() {
        assert_eq!(found("latin small letter a"), Some(vec![0x61]));
        assert_eq!(found("HANGUL SYLLABLE GAG"), Some(vec![0xAC01]));
        assert_eq!(found("hangul syllable gag"), None);
        assert_eq!(found("CJK UNIFIED IDEOGRAPH-04E00"), Some(vec![0x4E00]));
        assert_eq!(found("CJK UNIFIED IDEOGRAPH-4e00"), None);
        assert_eq!(
            found("cjk compatibility ideograph-f900"),
            Some(vec![0xF900])
        );
        assert_eq!(found("NUL"), Some(vec![0]));
        assert_eq!(found("BYTE ORDER MARK"), Some(vec![0xFEFF]));
        assert_eq!(
            found("LATIN CAPITAL LETTER A WITH MACRON AND GRAVE"),
            Some(vec![0x100, 0x300])
        );
        assert_eq!(found("SPACE "), None);
        assert_eq!(
            lookup(&Value::Str(vec![0x78, 0xD800, 0xDC00, 0x79])),
            Err(LookupError::Surrogates { start: 1, end: 3 })
        );
        let e = lookup(&s("x\0y"));
        assert_eq!(
            e,
            Err(Error::key_error("undefined character name 'x'").into())
        );
    }
}
