//! Single-character matchers with sre's IGNORECASE rules (`_compiler._compile`
//! for literals and `_optimize_charset` for sets).

use std::collections::HashMap;
use std::sync::OnceLock;

use super::casefix::extra_cases;
use super::parser::{Category, SetItem};
use crate::pyre::unicode15 as pystr;

const BMP_MAX: u32 = 0xFFFF;

fn as_char(c: u32) -> Option<char> {
    char::from_u32(c)
}

/// `_sre.unicode_tolower`.
pub(crate) fn uni_lower(c: u32) -> u32 {
    as_char(c).map_or(c, |ch| pystr::simple_lower(ch) as u32)
}

/// `_PyUnicode_ToUppercase`: for characters with a multi-character full
/// mapping (e.g. `ß` -> `SS`) CPython returns its first character.
fn uni_upper(c: u32) -> u32 {
    as_char(c).map_or(c, |ch| {
        pystr::upper_char(ch).chars().next().map_or(c, |u| u as u32)
    })
}

/// `_sre.ascii_tolower`.
pub(crate) fn ascii_lower(c: u32) -> u32 {
    if (u32::from(b'A')..=u32::from(b'Z')).contains(&c) {
        c + 32
    } else {
        c
    }
}

fn ascii_upper(c: u32) -> u32 {
    if (u32::from(b'a')..=u32::from(b'z')).contains(&c) {
        c - 32
    } else {
        c
    }
}

/// `_sre.unicode_iscased`.
fn uni_iscased(c: u32) -> bool {
    c != uni_lower(c) || c != uni_upper(c)
}

/// `_sre.ascii_iscased`.
fn ascii_iscased(c: u32) -> bool {
    u8::try_from(c).is_ok_and(|b| b.is_ascii_alphabetic())
}

/// BMP characters grouped by their simple lowercase (only those that differ).
fn inverse_lower() -> &'static HashMap<u32, Vec<u32>> {
    static INV: OnceLock<HashMap<u32, Vec<u32>>> = OnceLock::new();
    INV.get_or_init(|| {
        let mut m: HashMap<u32, Vec<u32>> = HashMap::new();
        for c in 0..=BMP_MAX {
            let l = uni_lower(c);
            if l != c {
                m.entry(l).or_default().push(c);
            }
        }
        m
    })
}

/// Case-folding mode of a compiled matcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fold {
    /// `re.ASCII | re.IGNORECASE`
    Ascii,
    /// Unicode `re.IGNORECASE` (with `_casefix` extra cases).
    Unicode,
}

impl Fold {
    fn lower(self, c: u32) -> u32 {
        match self {
            Fold::Ascii => ascii_lower(c),
            Fold::Unicode => uni_lower(c),
        }
    }
    fn iscased(self, c: u32) -> bool {
        match self {
            Fold::Ascii => ascii_iscased(c),
            Fold::Unicode => uni_iscased(c),
        }
    }
}

pub(crate) fn category_matches(cat: Category, c: char, unicode: bool) -> bool {
    match cat {
        Category::Digit => is_digit(c, unicode),
        Category::NotDigit => !is_digit(c, unicode),
        Category::Space => is_space(c, unicode),
        Category::NotSpace => !is_space(c, unicode),
        Category::Word => is_word(c, unicode),
        Category::NotWord => !is_word(c, unicode),
    }
}

fn is_digit(c: char, unicode: bool) -> bool {
    if unicode {
        pystr::is_decimal(c)
    } else {
        c.is_ascii_digit()
    }
}

fn is_space(c: char, unicode: bool) -> bool {
    if unicode {
        pystr::is_space(c)
    } else {
        matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c')
    }
}

/// `\w` for the given mode (`SRE_UNI_IS_WORD` / `SRE_IS_WORD`).
pub(crate) fn is_word(c: char, unicode: bool) -> bool {
    if unicode {
        pystr::is_word(c)
    } else {
        c.is_ascii_alphanumeric() || c == '_'
    }
}

/// A compiled character set (`IN`, `IN_IGNORE`, `IN_UNI_IGNORE`).
#[derive(Debug, Clone)]
pub(crate) struct CharSet {
    negate: bool,
    /// `None`: plain `IN`. `Some`: the char is lowered before testing.
    fold: Option<Fold>,
    unicode_cats: bool,
    lits: Vec<u32>,
    ranges: Vec<(u32, u32)>,
    cats: Vec<Category>,
    /// Folded sets only: lowered non-BMP literals.
    tail_lits: Vec<u32>,
    /// Folded sets only: ranges reaching past the BMP (`RANGE_UNI_IGNORE` when unicode).
    tail_ranges: Vec<(u32, u32)>,
    /// Bit `c` is [`CharSet::matches_slow`] of ASCII `c`, precomputed.
    ascii: u128,
}

impl CharSet {
    pub(crate) fn new(negate: bool, items: &[SetItem], fold: Option<Fold>, unicode: bool) -> Self {
        let mut set = CharSet {
            negate,
            fold: None,
            unicode_cats: unicode,
            lits: Vec::new(),
            ranges: Vec::new(),
            cats: Vec::new(),
            tail_lits: Vec::new(),
            tail_ranges: Vec::new(),
            ascii: 0,
        };
        let mut hascased = false;
        for it in items {
            match *it {
                SetItem::Cat(c) => set.cats.push(c),
                SetItem::Lit(c) => {
                    if let Some(f) = fold {
                        let lo = f.lower(c);
                        if f.iscased(lo) {
                            hascased = true;
                        }
                        if c > BMP_MAX {
                            set.tail_lits.push(lo);
                        }
                    }
                    set.lits.push(c);
                }
                SetItem::Range(a, b) => {
                    if let Some(f) = fold {
                        if b > BMP_MAX {
                            hascased = true;
                            set.tail_ranges.push((a, b));
                        } else if !hascased {
                            hascased = (a..=b).any(|c| f.iscased(c));
                        }
                    }
                    set.ranges.push((a, b));
                }
            }
        }
        if hascased {
            set.fold = fold;
        }
        set.ascii = (0u8..128)
            .filter(|&b| set.matches_slow(char::from(b)))
            .fold(0, |acc, b| acc | (1u128 << b));
        set
    }

    fn plain_contains(&self, c: u32) -> bool {
        self.lits.contains(&c) || self.ranges.iter().any(|&(a, b)| a <= c && c <= b)
    }

    fn bmp_contains(&self, c: u32) -> bool {
        c <= BMP_MAX
            && (self.lits.contains(&c) || self.ranges.iter().any(|&(a, b)| a <= c && c <= b))
    }

    #[inline]
    pub(crate) fn matches(&self, ch: char) -> bool {
        let c = ch as u32;
        if c < 128 {
            return self.ascii & (1u128 << c) != 0;
        }
        self.matches_slow(ch)
    }

    /// `matches` without the ASCII table (used to build it).
    fn matches_slow(&self, ch: char) -> bool {
        let hit = match self.fold {
            None => {
                self.plain_contains(ch as u32)
                    || self
                        .cats
                        .iter()
                        .any(|&k| category_matches(k, ch, self.unicode_cats))
            }
            Some(fold) => {
                let l = fold.lower(ch as u32);
                let lch = as_char(l).unwrap_or(ch);
                self.charmap_contains(fold, l)
                    || self.tail_lits.contains(&l)
                    || self.tail_ranges.iter().any(|&(a, b)| {
                        (a <= l && l <= b)
                            || (fold == Fold::Unicode && {
                                let u = uni_upper(l);
                                a <= u && u <= b
                            })
                    })
                    || self
                        .cats
                        .iter()
                        .any(|&k| category_matches(k, lch, self.unicode_cats))
            }
        };
        hit != self.negate
    }

    /// Whether the folded BMP charmap built by `_optimize_charset` has `l`.
    fn charmap_contains(&self, fold: Fold, l: u32) -> bool {
        if l > BMP_MAX {
            return false;
        }
        match fold {
            Fold::Ascii => {
                self.bmp_contains(l) || (ascii_upper(l) != l && self.bmp_contains(ascii_upper(l)))
            }
            Fold::Unicode => {
                let inv = inverse_lower();
                let check = |t: u32| {
                    (uni_lower(t) == t && self.bmp_contains(t))
                        || inv
                            .get(&t)
                            .is_some_and(|v| v.iter().any(|&p| self.bmp_contains(p)))
                };
                check(l) || extra_cases(l).is_some_and(|ex| ex.iter().any(|&t| check(t)))
            }
        }
    }
}

/// A matcher that consumes exactly one character.
#[derive(Debug, Clone)]
pub(crate) enum Matcher {
    Lit(u32),
    NotLit(u32),
    /// Folded literal: `lower(ch) in targets` (negated for `NOT_LITERAL`).
    Fold {
        targets: Vec<u32>,
        fold: Fold,
        negate: bool,
    },
    Any,
    AnyAll,
    Set(Box<CharSet>),
}

impl Matcher {
    /// Compile `LITERAL`/`NOT_LITERAL` under the given folding.
    pub(crate) fn literal(c: u32, negate: bool, fold: Option<Fold>) -> Self {
        match fold {
            Some(f) if f.iscased(c) => {
                let lo = f.lower(c);
                let mut targets = vec![lo];
                if f == Fold::Unicode {
                    if let Some(ex) = extra_cases(lo) {
                        targets.extend_from_slice(ex);
                    }
                }
                Matcher::Fold {
                    targets,
                    fold: f,
                    negate,
                }
            }
            _ => {
                if negate {
                    Matcher::NotLit(c)
                } else {
                    Matcher::Lit(c)
                }
            }
        }
    }

    #[inline]
    pub(crate) fn matches(&self, ch: char) -> bool {
        match self {
            Matcher::Lit(c) => ch as u32 == *c,
            Matcher::NotLit(c) => ch as u32 != *c,
            Matcher::Fold {
                targets,
                fold,
                negate,
            } => {
                // Both folds lower ASCII exactly like `to_ascii_lowercase`.
                let l = if ch.is_ascii() {
                    u32::from(ch.to_ascii_lowercase())
                } else {
                    fold.lower(ch as u32)
                };
                targets.contains(&l) != *negate
            }
            Matcher::Any => ch != '\n',
            Matcher::AnyAll => true,
            Matcher::Set(s) => s.matches(ch),
        }
    }
}
