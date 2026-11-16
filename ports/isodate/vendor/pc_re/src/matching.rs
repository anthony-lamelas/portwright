//! Match objects (`re.Match`) and the matching entry points of `re.Pattern`.

use std::sync::Arc;

use num_bigint::{BigInt, Sign};
use num_traits::ToPrimitive;

use crate::engine::State;
use crate::error::{Error, Result};
use crate::pattern::Pattern;
use crate::pyrepr;
use crate::text::{push_str, PyStr};
use crate::value::Value;

/// How a match attempt is anchored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// `pattern.search`: the first position where the pattern matches.
    Search,
    /// `pattern.match`: anchored at `pos`.
    Match,
    /// `pattern.fullmatch`: anchored at `pos` and at `endpos`.
    FullMatch,
}

/// A successful match.
#[derive(Debug, Clone)]
pub struct Match {
    /// `match.re`.
    pub(crate) pattern: Arc<Pattern>,
    /// `match.string`.
    pub(crate) string: Arc<[u32]>,
    /// `match.pos` (clamped as `state_init` does).
    pub(crate) pos: usize,
    /// `match.endpos` (clamped).
    pub(crate) endpos: usize,
    /// Spans of group 0..=groups; `None` for groups that didn't take part.
    pub(crate) spans: Vec<Option<(usize, usize)>>,
    /// `match.lastindex`.
    pub(crate) lastindex: Option<usize>,
}

/// Clamps `pos` / `endpos` to `0..=len` as `state_init` does.
pub fn clamp(value: isize, len: usize) -> usize {
    usize::try_from(value).map_or(0, |v| v.min(len))
}

impl Match {
    /// Builds a match from a successful engine state (`pattern_new_match`).
    pub(crate) fn from_state(
        pattern: &Arc<Pattern>,
        string: &Arc<[u32]>,
        state: &State<'_>,
        pos: usize,
        endpos: usize,
    ) -> Self {
        let mut spans = Vec::with_capacity(pattern.groups + 1);
        spans.push(Some((state.start, state.ptr)));
        for g in 1..=pattern.groups {
            spans.push(state.group_span(g));
        }
        Self {
            pattern: Arc::clone(pattern),
            string: Arc::clone(string),
            pos,
            endpos,
            spans,
            lastindex: usize::try_from(state.lastindex).ok(),
        }
    }

    /// `match.re`.
    #[must_use]
    pub fn re(&self) -> &Pattern {
        &self.pattern
    }

    /// `match.string`.
    #[must_use]
    pub fn string(&self) -> &[u32] {
        &self.string
    }

    /// `match.pos` and `match.endpos`.
    #[must_use]
    pub fn bounds(&self) -> (usize, usize) {
        (self.pos, self.endpos)
    }

    /// `match.lastindex`.
    #[must_use]
    pub fn lastindex(&self) -> Option<usize> {
        self.lastindex
    }

    /// Runs `pattern` on `string[pos:endpos]` (already clamped).
    pub fn run(
        pattern: &Arc<Pattern>,
        string: &Arc<[u32]>,
        pos: usize,
        endpos: usize,
        mode: Mode,
    ) -> Result<Option<Self>> {
        let mut state = State::new(string, pos, endpos, pattern.groups, &pattern.sets);
        let found = match mode {
            Mode::Search => state.search(&pattern.code, pattern.first.as_ref())?,
            Mode::Match => state.sre_match(&pattern.code)?,
            Mode::FullMatch => {
                state.match_all = true;
                state.sre_match(&pattern.code)?
            }
        };
        Ok(found.then(|| Self::from_state(pattern, string, &state, pos, endpos)))
    }

    /// The text of a span.
    fn slice(&self, (a, b): (usize, usize)) -> PyStr {
        self.string
            .get(a..b)
            .map(<[u32]>::to_vec)
            .unwrap_or_default()
    }

    /// The text of group `g`, or `None` if it didn't participate.
    pub fn group_text(&self, g: usize) -> Option<PyStr> {
        self.spans.get(g).copied().flatten().map(|s| self.slice(s))
    }

    /// The span of group `g` with `-1` for groups that didn't participate.
    pub fn span_of(&self, g: usize) -> (isize, isize) {
        match self.spans.get(g).copied().flatten() {
            Some((a, b)) => (to_isize(a), to_isize(b)),
            None => (-1, -1),
        }
    }

    /// `match_getindex`: a group number from an index or name.
    ///
    /// # Errors
    /// `IndexError: no such group`, or `TypeError` for unhashable names when
    /// the pattern has named groups.
    pub fn group_index(&self, index: &Value) -> Result<usize> {
        let no_such = || Error::Index("no such group".into());
        let i: Option<BigInt> = match index {
            Value::Int(n) => Some(n.clone()),
            Value::Bool(b) => Some(BigInt::from(u8::from(*b))),
            other => {
                if self.pattern.groupindex.is_empty() {
                    None
                } else {
                    crate::api::check_hashable(other)?;
                    match other {
                        Value::Str(s) => self.pattern.groupindex.get(s).map(|&g| BigInt::from(g)),
                        _ => None,
                    }
                }
            }
        };
        let i = i.ok_or_else(no_such)?;
        if i.sign() == Sign::Minus {
            return Err(no_such());
        }
        match i.to_usize() {
            Some(g) if g < self.spans.len() => Ok(g),
            _ => Err(no_such()),
        }
    }

    /// `match.group(index)` for one index: the text, or `default` if the
    /// group didn't participate.
    ///
    /// # Errors
    /// As [`Match::group_index`].
    pub fn get_slice(&self, index: &Value, default: &Value) -> Result<Value> {
        let g = self.group_index(index)?;
        Ok(self
            .group_text(g)
            .map_or_else(|| default.clone(), Value::Str))
    }

    /// `match.lastgroup`.
    pub fn lastgroup(&self) -> Option<&PyStr> {
        let i = self.lastindex?;
        self.pattern.indexgroup.get(i)?.as_ref()
    }

    /// `repr(match)`: `<re.Match object; span=(a, b), match=%.50R>`.
    pub fn repr(&self) -> PyStr {
        let (a, b) = self.span_of(0);
        let mut out = PyStr::new();
        push_str(
            &mut out,
            &format!("<re.Match object; span=({a}, {b}), match="),
        );
        let text = self.group_text(0).unwrap_or_default();
        out.extend(pyrepr::str_repr(&text).into_iter().take(50));
        out.push(u32::from('>'));
        out
    }
}

fn to_isize(x: usize) -> isize {
    isize::try_from(x).unwrap_or(isize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::{cps, to_string_lossy};

    fn run(p: &str, s: &str, mode: Mode) -> Option<Match> {
        let pat = Arc::new(Pattern::compile(&cps(p), &BigInt::from(0)).ok()?);
        let string: Arc<[u32]> = cps(s).into();
        let len = string.len();
        Match::run(&pat, &string, 0, len, mode).ok().flatten()
    }

    fn show(p: &str, s: &str, mode: Mode) -> String {
        match run(p, s, mode) {
            None => "None".into(),
            Some(m) => {
                let groups: Vec<String> = (0..m.spans.len())
                    .map(|g| format!("{:?}", m.span_of(g)))
                    .collect();
                format!(
                    "{} {:?} {}",
                    groups.join(""),
                    m.lastindex,
                    to_string_lossy(&m.repr())
                )
            }
        }
    }

    #[test]
    fn basics() {
        assert_eq!(
            show("b+", "abbbc", Mode::Search),
            "(1, 4) None <re.Match object; span=(1, 4), match='bbb'>"
        );
        assert_eq!(show("b+", "abbbc", Mode::Match), "None");
        assert_eq!(
            show("(a)|(b)", "b", Mode::FullMatch).split(' ').next(),
            Some("(0,")
        );
        assert_eq!(
            show("(?:(a)|b)*", "ab", Mode::Match),
            "(0, 2)(0, 1) Some(1) <re.Match object; span=(0, 2), match='ab'>"
        );
        assert_eq!(
            show("(?:ab)*+c", "ababc", Mode::Match),
            "(0, 5) None <re.Match object; span=(0, 5), match='ababc'>"
        );
        assert_eq!(
            show("(a)\\1", "xaa", Mode::Search).split(' ').next(),
            Some("(1,")
        );
    }
}
