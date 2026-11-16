//! Cached `pc_re` patterns and group extraction shared by the parsers (task `tz`).
//!
//! Every upstream regex goes through `pc_re` so CPython's `re` semantics (`\Z`, `$`
//! before a trailing newline, ...) hold. A [`Regex`] is declared as a `static` and
//! compiles its pattern on first use.

use std::sync::{Arc, OnceLock};

use pc_re_port_core::matching::{Match, Mode};
use pc_re_port_core::Pattern;

use crate::error::{Error, Result};

/// A pattern compiled once, on first use (`re.compile(SOURCE)` at module level).
#[derive(Debug)]
pub struct Regex {
    source: &'static str,
    compiled: OnceLock<std::result::Result<Arc<Pattern>, Error>>,
}

impl Regex {
    /// A lazily compiled pattern with no flags.
    #[must_use]
    pub const fn new(source: &'static str) -> Self {
        Self {
            source,
            compiled: OnceLock::new(),
        }
    }

    /// The compiled pattern.
    ///
    /// # Errors
    /// The error `re.compile` would raise (never, for upstream's patterns).
    pub fn pattern(&self) -> Result<&Arc<Pattern>> {
        self.compiled
            .get_or_init(|| {
                let cps: Vec<u32> = self.source.chars().map(u32::from).collect();
                compile(&cps)
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    /// `pattern.match(text)`: anchored at the start, may end anywhere.
    ///
    /// # Errors
    /// Engine errors (recursion limits), as `re` raises them.
    pub fn match_at_start(&self, text: &[u32]) -> Result<Option<Captures>> {
        run(self.pattern()?, &Arc::from(text), Mode::Match)
    }
}

/// `re.compile(source)` with no flags, uncached.
///
/// # Errors
/// The error `re.compile` raises, converted to a core [`Error`].
pub fn compile(source: &[u32]) -> Result<Arc<Pattern>> {
    Pattern::compile(source, &num_bigint::BigInt::from(0))
        .map(Arc::new)
        .map_err(re_error)
}

/// Runs a compiled pattern over the whole of `text`. Callers trying several patterns
/// build the shared `Arc` once.
///
/// # Errors
/// Engine errors, as `re` raises them.
pub fn run(pattern: &Arc<Pattern>, text: &Arc<[u32]>, mode: Mode) -> Result<Option<Captures>> {
    let found = Match::run(pattern, text, 0, text.len(), mode).map_err(re_error)?;
    Ok(found.map(|inner| Captures { inner }))
}

/// A successful match, with groups read by name or number.
#[derive(Debug, Clone)]
pub struct Captures {
    inner: Match,
}

impl Captures {
    /// Group `index` (0 is the whole match), or `None` if it didn't take part.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&[u32]> {
        let (start, end) = self.inner.span_of(index);
        let start = usize::try_from(start).ok()?;
        let end = usize::try_from(end).ok()?;
        self.inner.string().get(start..end)
    }

    /// Named group `name`, or `None` if it didn't take part (or doesn't exist).
    #[must_use]
    pub fn name(&self, name: &str) -> Option<&[u32]> {
        let (_, &index) = self
            .inner
            .re()
            .groupindex()
            .iter()
            .find(|(key, _)| key.iter().copied().eq(name.chars().map(u32::from)))?;
        self.get(index)
    }
}

impl Named for Captures {
    fn name(&self, name: &str) -> Option<&[u32]> {
        Captures::name(self, name)
    }
}

/// Reading a match's named groups, whether `pc_re` or a hand-written matcher found it.
pub trait Named {
    /// Named group `name`, or `None` if it didn't take part.
    fn name(&self, name: &str) -> Option<&[u32]>;
}

/// The most named groups any of upstream's patterns has.
const MAX_GROUPS: usize = 8;

/// The named groups found by a hand-written matcher.
///
/// The matchers replace `pc_re` for upstream's fixed patterns, which are deterministic
/// (every choice point has at most one way to finish the match), so a single greedy
/// pass finds exactly the groups `re` reports. Tests compare them with `pc_re`.
#[derive(Debug, Clone)]
pub struct Groups<'t> {
    text: &'t [u32],
    spans: [(&'static str, usize, usize); MAX_GROUPS],
    len: usize,
}

impl<'t> Groups<'t> {
    /// No groups yet, over `text`.
    #[must_use]
    pub fn new(text: &'t [u32]) -> Self {
        Self {
            text,
            spans: [("", 0, 0); MAX_GROUPS],
            len: 0,
        }
    }

    /// Records group `name` as `text[start..end]`.
    pub fn set(&mut self, name: &'static str, start: usize, end: usize) {
        if let Some(slot) = self.spans.get_mut(self.len) {
            *slot = (name, start, end);
            self.len += 1;
        }
    }
}

impl Named for Groups<'_> {
    fn name(&self, name: &str) -> Option<&[u32]> {
        let &(_, start, end) = self
            .spans
            .get(..self.len)?
            .iter()
            .find(|(key, _, _)| *key == name)?;
        self.text.get(start..end)
    }
}

/// Whether `text[i]` exists and is one of `chars` (ASCII).
#[must_use]
pub fn is_at(text: &[u32], i: usize, chars: &str) -> bool {
    text.get(i)
        .is_some_and(|&c| chars.chars().any(|k| u32::from(k) == c))
}

/// `[0-9]{n}` at `i`: its end.
#[must_use]
pub fn digits(text: &[u32], i: usize, n: usize) -> Option<usize> {
    let end = i.checked_add(n)?;
    text.get(i..end)?
        .iter()
        .all(|c| (0x30..=0x39).contains(c))
        .then_some(end)
}

/// `[0-9]+` at `i` (greedy): its end.
#[must_use]
pub fn digit_run(text: &[u32], i: usize) -> Option<usize> {
    let count = text
        .get(i..)?
        .iter()
        .take_while(|c| (0x30..=0x39).contains(*c))
        .count();
    (count > 0).then_some(i + count)
}

/// `([,.][0-9]+)?` at `i` (greedy): its end, `i` when absent.
#[must_use]
pub fn fraction(text: &[u32], i: usize) -> usize {
    if is_at(text, i, ",.") {
        if let Some(end) = digit_run(text, i + 1) {
            return end;
        }
    }
    i
}

/// `[0-9]+([,.][0-9]+)?` at `i`: its end.
#[must_use]
pub fn number(text: &[u32], i: usize) -> Option<usize> {
    digit_run(text, i).map(|end| fraction(text, end))
}

/// `int(text)` for a group its pattern restricts to `[0-9]`. That guarantee is why the
/// digit values are only clamped (`.min(9)`), never checked.
fn digit_values(text: &[u32]) -> impl Iterator<Item = u32> + '_ {
    text.iter()
        .map(|&c| c.saturating_sub(u32::from('0')).min(9))
}

/// [`digit_values`] as an `i64`, for groups of a few digits; an absent group is 0.
#[must_use]
pub fn small_int(text: Option<&[u32]>) -> i64 {
    digit_values(text.unwrap_or_default()).fold(0, |acc, d| acc * 10 + i64::from(d))
}

/// [`digit_values`] as a `BigInt`, for groups of any length.
#[must_use]
pub fn big_int(text: &[u32]) -> num_bigint::BigInt {
    digit_values(text).fold(num_bigint::BigInt::from(0), |acc, d| acc * 10 + d)
}

/// Converts a `pc_re` error to the built-in exception `re` raises.
#[must_use]
pub fn re_error(error: pc_re_port_core::Error) -> Error {
    use pc_re_port_core::Error as Re;
    let text = error.to_string();
    match error {
        Re::Type(_) => Error::type_error(text.as_str()),
        Re::Overflow(_) => Error::overflow(text.as_str()),
        Re::Attribute(_) => Error::attribute(text.as_str()),
        _ => Error::value(text.as_str()),
    }
}

/// Inputs for comparing the hand-written matchers with `pc_re`: the seeds, random edits
/// of them, and random strings, over the characters the patterns care about.
#[cfg(test)]
pub(crate) fn sample_texts(seeds: &[&str], count: usize) -> Vec<Vec<u32>> {
    const ALPHABET: &[char] = &[
        '0', '1', '2', '5', '9', '+', '-', ':', ',', '.', 'T', 'Z', 'P', 'Y', 'M', 'W', 'D', 'H',
        'S', '\n', ' ', 'a', '_', '\u{e9}', '\u{663}',
    ];
    let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut next = |bound: usize| -> usize {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        usize::try_from(state % u64::try_from(bound).unwrap_or(1)).unwrap_or(0)
    };
    let mut out: Vec<Vec<u32>> = seeds
        .iter()
        .map(|s| s.chars().map(u32::from).collect())
        .collect();
    for n in 0..count {
        let mut text: Vec<u32> = if n % 4 == 0 || seeds.is_empty() {
            Vec::new()
        } else {
            out[next(seeds.len())].clone()
        };
        let random_len = if text.is_empty() { next(14) } else { 0 };
        for _ in 0..random_len {
            text.push(u32::from(ALPHABET[next(ALPHABET.len())]));
        }
        for _ in 0..next(4) {
            let c = u32::from(ALPHABET[next(ALPHABET.len())]);
            let at = next(text.len() + 1);
            match next(3) {
                0 => text.insert(at, c),
                1 if at < text.len() => {
                    text.remove(at);
                }
                _ if at < text.len() => text[at] = c,
                _ => text.push(c),
            }
        }
        out.push(text);
    }
    out
}

/// Checks a hand-written matcher against `pattern.match` on `texts`, group by group.
#[cfg(test)]
pub(crate) fn assert_same_groups<'t, G: Named>(
    pattern: &Arc<Pattern>,
    texts: &'t [Vec<u32>],
    names: &[&str],
    fast: impl Fn(&'t [u32]) -> Option<G>,
) {
    for text in texts {
        let slow = run(pattern, &Arc::from(text.as_slice()), Mode::Match)
            .ok()
            .flatten();
        let quick = fast(text);
        assert_eq!(slow.is_some(), quick.is_some(), "{text:?}");
        if let (Some(slow), Some(quick)) = (slow, quick) {
            for name in names {
                assert_eq!(slow.name(name), quick.name(name), "{name} in {text:?}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cps(s: &str) -> Vec<u32> {
        s.chars().map(u32::from).collect()
    }

    static DIGITS: Regex = Regex::new(r"(?P<a>[0-9]+)(?P<b>x)?");

    #[test]
    fn named_groups() -> Result<()> {
        let found = DIGITS.match_at_start(&cps("12y"))?;
        let Some(caps) = found else {
            return Err(Error::value("no match"));
        };
        assert_eq!(caps.name("a"), Some(cps("12").as_slice()));
        assert_eq!(caps.name("b"), None);
        assert_eq!(caps.name("missing"), None);
        assert_eq!(small_int(caps.name("a")), 12);
        assert_eq!(big_int(&cps("12")), num_bigint::BigInt::from(12));
        Ok(())
    }
}
