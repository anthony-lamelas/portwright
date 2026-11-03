//! `unicodedata.normalize` and `unicodedata.is_normalized` (CPython 3.12, Unicode 15.0).
//!
//! Mirrors CPython's `nfd_nfkd`, `nfc_nfkc` and `is_normalized_quickcheck`, including
//! the order in which arguments and forms are checked.

use crate::args::bad_arg_type_name;
use crate::error::{Error, Result};
use crate::hangul::{L_BASE, L_COUNT, N_COUNT, S_BASE, S_COUNT, T_BASE, T_COUNT, V_BASE, V_COUNT};
use crate::tables::normalization as t;
use crate::tables::unidata as ud;
use crate::value::{Text, Value};
use std::sync::OnceLock;

/// A normalization form; `Composed` forms are NFC/NFKC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// Canonical decomposition.
    Nfd,
    /// Compatibility decomposition.
    Nfkd,
    /// Canonical decomposition, then canonical composition.
    Nfc,
    /// Compatibility decomposition, then canonical composition.
    Nfkc,
}

impl Form {
    /// Parses a form name exactly as CPython compares it (case- and length-sensitive).
    fn parse(name: &[u32]) -> Option<Self> {
        const NAMES: [(&str, Form); 4] = [
            ("NFC", Form::Nfc),
            ("NFKC", Form::Nfkc),
            ("NFD", Form::Nfd),
            ("NFKD", Form::Nfkd),
        ];
        NAMES.iter().find_map(|&(n, form)| {
            let matches =
                n.len() == name.len() && n.bytes().zip(name).all(|(b, &c)| u32::from(b) == c);
            matches.then_some(form)
        })
    }

    fn is_composed(self) -> bool {
        matches!(self, Form::Nfc | Form::Nfkc)
    }

    fn is_compat(self) -> bool {
        matches!(self, Form::Nfkd | Form::Nfkc)
    }

    /// Shift of this form's 2-bit field in the quick-check byte.
    fn qc_shift(self) -> u32 {
        match self {
            Form::Nfd => 0,
            Form::Nfkd => 2,
            Form::Nfc => 4,
            Form::Nfkc => 6,
        }
    }
}

/// Per-code-point positions in the decomposition and composition tables, so lookups
/// index directly instead of binary-searching. Built once from those same tables.
#[derive(Debug, Clone, Copy, Default)]
struct Slots {
    /// 1 + index into `NFD_MAP`, or 0.
    nfd: u16,
    /// 1 + index into `NFKD_MAP`, or 0.
    nfkd: u16,
    /// First entry in `COMPOSITIONS` with this code point as the first char.
    comp_start: u16,
    /// Number of such entries.
    comp_len: u16,
    /// Listed in `COMPOSITION_FIRSTS`.
    first: bool,
}

const SLOT_SHIFT: u32 = 8;
const SLOT_BLOCK: usize = 1 << SLOT_SHIFT;

/// Two-level table: `index[cp >> SLOT_SHIFT]` picks a block; block 0 is all empty.
struct SlotTable {
    index: Vec<u16>,
    blocks: Vec<[Slots; SLOT_BLOCK]>,
}

impl SlotTable {
    fn build() -> Self {
        let mut table = SlotTable {
            index: vec![0; (0x10FFFF >> SLOT_SHIFT) + 1],
            blocks: vec![[Slots::default(); SLOT_BLOCK]],
        };
        for (i, e) in t::NFD_MAP.iter().enumerate() {
            table.update(e.0, |s| s.nfd = to_u16(i + 1));
        }
        for (i, e) in t::NFKD_MAP.iter().enumerate() {
            table.update(e.0, |s| s.nfkd = to_u16(i + 1));
        }
        for (i, e) in t::COMPOSITIONS.iter().enumerate() {
            table.update(e.0, |s| {
                if s.comp_len == 0 {
                    s.comp_start = to_u16(i);
                }
                s.comp_len += 1;
            });
        }
        for &cp in &t::COMPOSITION_FIRSTS {
            table.update(cp, |s| s.first = true);
        }
        table
    }

    /// Applies `f` to the slots of `cp` (at most U+10FFFF), adding its block if needed.
    fn update(&mut self, cp: u32, f: impl FnOnce(&mut Slots)) {
        let Some(entry) = self.index.get_mut((cp >> SLOT_SHIFT) as usize) else {
            return;
        };
        if *entry == 0 {
            *entry = to_u16(self.blocks.len());
            self.blocks.push([Slots::default(); SLOT_BLOCK]);
        }
        let lo = (cp as usize) & (SLOT_BLOCK - 1);
        if let Some(slot) = self
            .blocks
            .get_mut(usize::from(*entry))
            .and_then(|b| b.get_mut(lo))
        {
            f(slot);
        }
    }

    fn get(&self, cp: u32) -> Slots {
        let block = self
            .index
            .get((cp >> SLOT_SHIFT) as usize)
            .copied()
            .unwrap_or(0);
        self.blocks
            .get(usize::from(block))
            .and_then(|b| b.get((cp as usize) & (SLOT_BLOCK - 1)))
            .copied()
            .unwrap_or_default()
    }
}

// Every index stored in `Slots` fits in `u16`; so does the block count, since there
// are at most 0x1100 blocks.
const _: () = assert!(t::NFD_MAP.len() < u16::MAX as usize);
const _: () = assert!(t::NFKD_MAP.len() < u16::MAX as usize);
const _: () = assert!(t::COMPOSITIONS.len() < u16::MAX as usize);

/// `n` as `u16`; every caller's value is in range (see the asserts above).
fn to_u16(n: usize) -> u16 {
    u16::try_from(n).unwrap_or(u16::MAX)
}

fn slots(cp: u32) -> Slots {
    static TABLE: OnceLock<SlotTable> = OnceLock::new();
    TABLE.get_or_init(SlotTable::build).get(cp)
}

/// CPython's `QuickcheckResult`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuickCheck {
    Yes,
    No,
    Maybe,
}

/// Canonical combining class of `cp`.
fn ccc(cp: u32) -> u8 {
    // No char below U+0300 is a combining mark (checked by a unit test).
    if cp < 0x300 {
        return 0;
    }
    let index = usize::from(ud::record_index(cp.min(0x10FFFF)));
    ud::RECORDS.get(index).map_or(0, |r| r.2)
}

fn qc_field(cp: u32, form: Form) -> u8 {
    // Every char below U+00A0 is Yes for every form (checked by a unit test).
    if cp < 0xA0 {
        return 0;
    }
    (t::quick_check(cp.min(0x10FFFF)) >> form.qc_shift()) & 3
}

/// CPython's `is_normalized_quickcheck`. With `yes_only`, any non-Yes char gives Maybe.
fn quick_check(s: &[u32], form: Form, yes_only: bool) -> QuickCheck {
    if s.iter().all(|&c| c < 0x80) {
        return QuickCheck::Yes;
    }
    let mut result = QuickCheck::Yes;
    let mut prev = 0u8;
    for &cp in s {
        let combining = ccc(cp);
        if combining != 0 && prev > combining {
            return QuickCheck::No;
        }
        prev = combining;
        let field = qc_field(cp, form);
        if yes_only {
            if field != 0 {
                return QuickCheck::Maybe;
            }
        } else {
            match field {
                1 => return QuickCheck::No,
                2 => result = QuickCheck::Maybe,
                _ => {}
            }
        }
    }
    result
}

/// The full decomposition of `cp`, if it has one in the table.
///
/// A char whose NFD (or NFKD) quick-check field is Yes has no such decomposition, so
/// callers skip this for it (checked for every code point by a unit test).
fn lookup_map_table(cp: u32, compat: bool) -> Option<&'static [u32]> {
    let slot = slots(cp);
    let entry = if compat {
        t::NFKD_MAP.get(usize::from(slot.nfkd).checked_sub(1)?)
    } else {
        t::NFD_MAP.get(usize::from(slot.nfd).checked_sub(1)?)
    };
    let data: &[u32] = if compat {
        &t::NFKD_MAP_DATA
    } else {
        &t::NFD_MAP_DATA
    };
    let &(_, start, len) = entry?;
    let start = usize::from(start);
    data.get(start..start + usize::from(len))
}

/// CPython's `nfd_nfkd`: decompose fully, then sort runs of non-starters stably by class.
fn decompose(s: &[u32], compat: bool) -> Text {
    let form = if compat { Form::Nfkd } else { Form::Nfd };
    let mut out = Text::with_capacity(s.len() * 2);
    for &cp in s {
        // Quick-check Yes means no decomposition (Hangul syllables are No).
        if qc_field(cp, form) == 0 {
            out.push(cp);
        } else if (S_BASE..S_BASE + S_COUNT).contains(&cp) {
            let index = cp - S_BASE;
            out.push(L_BASE + index / N_COUNT);
            out.push(V_BASE + (index % N_COUNT) / T_COUNT);
            let trail = T_BASE + index % T_COUNT;
            if trail != T_BASE {
                out.push(trail);
            }
        } else if let Some(d) = lookup_map_table(cp, compat) {
            out.extend_from_slice(d);
        } else {
            out.push(cp);
        }
    }
    sort_marks(&mut out);
    out
}

/// Stably sorts each run of non-starters by canonical combining class.
fn sort_marks(text: &mut [u32]) {
    let mut i = 0;
    while let Some(&c) = text.get(i) {
        i += 1;
        if ccc(c) == 0 {
            continue;
        }
        let start = i - 1;
        while text.get(i).is_some_and(|&c| ccc(c) != 0) {
            i += 1;
        }
        if i - start > 1 {
            if let Some(run) = text.get_mut(start..i) {
                run.sort_by_key(|&c| ccc(c));
            }
        }
    }
}

fn is_first(cp: u32) -> bool {
    slots(cp).first
}

/// The primary composite of `first` and `last`, if any.
///
/// Every second char of a primary composite has NFC quick-check Maybe, so other chars
/// skip the table search (checked over the whole table by a unit test).
fn compose_pair(first: u32, last: u32) -> Option<u32> {
    if qc_field(last, Form::Nfc) != 2 {
        return None;
    }
    compose_pair_table(first, last)
}

/// The composition table entry for `(first, last)`, if any.
fn compose_pair_table(first: u32, last: u32) -> Option<u32> {
    let slot = slots(first);
    let start = usize::from(slot.comp_start);
    t::COMPOSITIONS
        .get(start..start + usize::from(slot.comp_len))?
        .iter()
        .find(|e| e.1 == last)
        .map(|e| e.2)
}

/// CPython's `nfc_nfkc` composition pass over decomposed text.
fn compose(d: &[u32]) -> Text {
    let len = d.len();
    let at = |i: usize| d.get(i).copied().unwrap_or(0);
    let mut skipped = vec![false; len];
    let mut out = Text::with_capacity(len);
    let mut i = 0;
    while i < len {
        if skipped.get(i).copied().unwrap_or(false) {
            i += 1;
            continue;
        }
        let code = at(i);
        // Hangul: <L, V> and optionally a following T.
        if (L_BASE..L_BASE + L_COUNT).contains(&code)
            && i + 1 < len
            && (V_BASE..V_BASE + V_COUNT).contains(&at(i + 1))
        {
            let mut syllable =
                S_BASE + ((code - L_BASE) * V_COUNT + (at(i + 1) - V_BASE)) * T_COUNT;
            i += 2;
            if i < len && T_BASE < at(i) && at(i) < T_BASE + T_COUNT {
                syllable += at(i) - T_BASE;
                i += 1;
            }
            out.push(syllable);
            continue;
        }
        // The scan below would stop at once if the next char is a starter that can't
        // be the second char of a pair (or there is none), leaving `code` unchanged.
        let next_inert = d
            .get(i + 1)
            .is_none_or(|&next| ccc(next) == 0 && qc_field(next, Form::Nfc) != 2);
        if next_inert || !is_first(code) {
            out.push(code);
            i += 1;
            continue;
        }
        let mut current = code;
        let mut comb = 0u8;
        let mut i1 = i + 1;
        while i1 < len {
            let code1 = at(i1);
            let comb1 = ccc(code1);
            if comb != 0 {
                if comb1 == 0 {
                    break;
                }
                if comb >= comb1 {
                    i1 += 1;
                    continue;
                }
            }
            match compose_pair(current, code1) {
                Some(composite) => {
                    current = composite;
                    if let Some(flag) = skipped.get_mut(i1) {
                        *flag = true;
                    }
                    i1 += 1;
                    if !is_first(current) {
                        break;
                    }
                }
                None => {
                    if comb1 == 0 {
                        break;
                    }
                    comb = comb1;
                    i1 += 1;
                }
            }
        }
        out.push(current);
        i += 1;
    }
    out
}

fn normalize_text(s: &[u32], form: Form) -> Text {
    let d = decompose(s, form.is_compat());
    if form.is_composed() {
        compose(&d)
    } else {
        d
    }
}

/// Checks both arguments like CPython's Argument Clinic (`form` first, then `s`).
fn args<'a>(func: &str, form: &'a Value, s: &'a Value) -> Result<(&'a [u32], &'a [u32])> {
    let bad = |n: u8, v: &Value| {
        Error::type_error(format!(
            "{func}() argument {n} must be str, not {}",
            bad_arg_type_name(v)
        ))
    };
    let form = form.as_text().ok_or_else(|| bad(1, form))?;
    let s = s.as_text().ok_or_else(|| bad(2, s))?;
    Ok((form, s))
}

fn parse_form(name: &[u32]) -> Result<Form> {
    Form::parse(name).ok_or_else(|| Error::value_error("invalid normalization form"))
}

/// `unicodedata.normalize(form, s)`.
///
/// Empty `s` is returned before `form` is validated, as in CPython.
///
/// # Errors
/// `TypeError` if `form` or `s` is not a `str`; `ValueError('invalid normalization form')`.
pub fn normalize(form: &Value, s: &Value) -> Result<Text> {
    let (form, s) = args("normalize", form, s)?;
    if s.is_empty() {
        return Ok(Text::new());
    }
    let form = parse_form(form)?;
    if quick_check(s, form, true) == QuickCheck::Yes {
        return Ok(s.to_vec());
    }
    Ok(normalize_text(s, form))
}

/// `unicodedata.is_normalized(form, s)`.
///
/// Empty `s` is `True` before `form` is validated, as in CPython.
///
/// # Errors
/// `TypeError` if `form` or `s` is not a `str`; `ValueError('invalid normalization form')`.
pub fn is_normalized(form: &Value, s: &Value) -> Result<bool> {
    let (form, s) = args("is_normalized", form, s)?;
    if s.is_empty() {
        return Ok(true);
    }
    let form = parse_form(form)?;
    Ok(match quick_check(s, form, false) {
        QuickCheck::Yes => true,
        QuickCheck::No => false,
        QuickCheck::Maybe => normalize_text(s, form) == s,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Value {
        Value::Str(s.chars().map(u32::from).collect())
    }

    fn norm(form: &str, s: &str) -> Text {
        normalize(&text(form), &text(s)).unwrap_or_default()
    }

    fn cps(s: &str) -> Text {
        s.chars().map(u32::from).collect()
    }

    #[test]
    fn forms() {
        assert_eq!(norm("NFD", "\u{e9}"), cps("e\u{301}"));
        assert_eq!(norm("NFC", "e\u{301}"), cps("\u{e9}"));
        assert_eq!(norm("NFKC", "\u{fb01}"), cps("fi"));
        assert_eq!(norm("NFD", "\u{d55c}"), cps("\u{1112}\u{1161}\u{11ab}"));
        assert_eq!(norm("NFC", "\u{1112}\u{1161}\u{11ab}"), cps("\u{d55c}"));
        assert_eq!(norm("NFD", "a\u{301}\u{316}"), cps("a\u{316}\u{301}"));
    }

    #[test]
    fn decomposition_shortcut_matches_table() {
        for cp in 0..=0x10FFFF {
            for (compat, form) in [(false, Form::Nfd), (true, Form::Nfkd)] {
                if qc_field(cp, form) == 0 {
                    assert_eq!(lookup_map_table(cp, compat), None, "{cp:#x}");
                }
            }
        }
    }

    #[test]
    fn hangul_syllables_are_not_quick_check_yes() {
        for cp in S_BASE..S_BASE + S_COUNT {
            assert_ne!(qc_field(cp, Form::Nfd), 0, "{cp:#x}");
            assert_ne!(qc_field(cp, Form::Nfkd), 0, "{cp:#x}");
        }
    }

    #[test]
    fn low_char_shortcuts_match_tables() {
        for cp in 0..0x300 {
            let index = usize::from(ud::record_index(cp));
            assert_eq!(ud::RECORDS.get(index).map_or(0, |r| r.2), 0, "{cp:#x}");
        }
        for cp in 0..0xA0 {
            assert_eq!(t::quick_check(cp), 0, "{cp:#x}");
        }
    }

    #[test]
    fn slot_table_matches_binary_search() {
        for cp in 0..=0x10FFFF {
            for (compat, map, data) in [
                (false, &t::NFD_MAP[..], &t::NFD_MAP_DATA[..]),
                (true, &t::NFKD_MAP[..], &t::NFKD_MAP_DATA[..]),
            ] {
                let expected = map.binary_search_by_key(&cp, |e| e.0).ok().and_then(|i| {
                    let (_, start, len) = map[i];
                    data.get(usize::from(start)..usize::from(start) + usize::from(len))
                });
                assert_eq!(lookup_map_table(cp, compat), expected, "{cp:#x}");
            }
            assert_eq!(
                is_first(cp),
                t::COMPOSITION_FIRSTS.binary_search(&cp).is_ok(),
                "{cp:#x}"
            );
            let in_table = t::COMPOSITIONS.binary_search_by_key(&cp, |e| e.0).is_ok();
            assert_eq!(slots(cp).comp_len != 0, in_table, "{cp:#x}");
        }
        for &(first, last, composite) in &t::COMPOSITIONS {
            assert_eq!(compose_pair_table(first, last), Some(composite));
        }
        assert_eq!(compose_pair_table(0x41, 0x41), None);
    }

    #[test]
    fn composition_shortcut_matches_table() {
        for &(first, last, composite) in &t::COMPOSITIONS {
            assert_eq!(compose_pair(first, last), Some(composite));
        }
    }

    #[test]
    fn errors() {
        let e = normalize(&text("nfc"), &text("a")).map_err(|e| e.message);
        assert_eq!(e, Err("invalid normalization form".to_owned()));
        assert_eq!(normalize(&text("bad"), &text("")), Ok(Text::new()));
        assert_eq!(is_normalized(&text("bad"), &text("")), Ok(true));
        let e = is_normalized(&Value::None, &text("")).map_err(|e| e.message);
        assert_eq!(
            e,
            Err("is_normalized() argument 1 must be str, not None".to_owned())
        );
    }
}
