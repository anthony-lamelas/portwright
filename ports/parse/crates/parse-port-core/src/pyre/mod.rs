//! `pyre`: a Python `re` (sre) compatible backtracking regex engine.
//!
//! Patterns use Python syntax and are parsed by a port of `re._parser`, so
//! errors carry CPython's exact messages and positions. Text is matched as
//! `&[char]`; all positions are code points.

pub(crate) mod cache;
mod casefix;
mod charset;
mod compile;
pub mod dt_format;
mod parser;
pub mod unicode15;
mod unicode15_tables;
mod vm;

use std::sync::Arc;

use indexmap::IndexMap;

use crate::error::{Error, RegexError};
use cache::BoundedCache;
use compile::{Inst, Program};
pub use parser::DEFAULT_MAX_NESTED;
use parser::{FLAG_DOTALL, FLAG_IGNORECASE};
use vm::{Mode, Vm};

/// Message of the `RecursionError` sre's parser hits on deeply nested groups.
pub const RECURSION_MSG: &str = "maximum recursion depth exceeded";

/// Whether a compile error stands for Python's `RecursionError`.
///
/// The frozen [`Error`] has no such variant, so the parser reports it as
/// `Error::Value(RECURSION_MSG)` (sre never raises that as a `ValueError`);
/// the PyO3 layer turns it into `RecursionError`.
pub fn is_recursion(e: &Error) -> bool {
    matches!(e, Error::Value(m) if m == RECURSION_MSG)
}

/// Compile flags (`re.I`, `re.S`). Inline flags (`(?imsxau)`, scoped
/// `(?i:...)`/`(?-i:...)`) are also understood.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Flags {
    /// `re.IGNORECASE`
    pub ignorecase: bool,
    /// `re.DOTALL`
    pub dotall: bool,
}

impl Flags {
    fn bits(self) -> u32 {
        let mut f = 0;
        if self.ignorecase {
            f |= FLAG_IGNORECASE;
        }
        if self.dotall {
            f |= FLAG_DOTALL;
        }
        f
    }
}

/// A compiled pattern (`re.Pattern`).
#[derive(Debug, Clone)]
pub struct Regex {
    pattern: String,
    flags: u32,
    groups: usize,
    groupindex: IndexMap<String, usize>,
    prog: std::sync::Arc<Program>,
}

impl std::fmt::Debug for Program {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Program")
            .field("insts", &self.insts.len())
            .finish()
    }
}

/// Spans of a successful match. `spans[0]` is the whole match; `None` is an
/// unmatched group (Python span `(-1, -1)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Captures {
    spans: Vec<Option<(usize, usize)>>,
}

impl Captures {
    /// Span of group `i` in code points, if it participated.
    pub fn span(&self, i: usize) -> Option<(usize, usize)> {
        self.spans.get(i).copied().flatten()
    }

    /// Number of groups including group 0.
    pub fn len(&self) -> usize {
        self.spans.len()
    }

    /// Always false: group 0 is always present.
    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    /// The text of group `i`, if it participated.
    pub fn text(&self, text: &[char], i: usize) -> Option<String> {
        self.span(i)
            .and_then(|(s, e)| text.get(s..e))
            .map(|cs| cs.iter().collect())
    }
}

/// sre `state_init` clamping of `pos`/`endpos` to `0..=len`.
pub fn clamp_pos(pos: isize, endpos: isize, len: usize) -> (usize, usize) {
    let clamp = |v: isize| -> usize {
        if v < 0 {
            0
        } else {
            usize::try_from(v).unwrap_or(usize::MAX).min(len)
        }
    };
    (clamp(pos), clamp(endpos))
}

impl Regex {
    /// `re.compile(pattern, flags)`. Errors are `re.error`. The rare non-`re.error`
    /// failures (`OverflowError`, `ValueError`) are folded into a position-less
    /// `RegexError`; use [`Regex::compile`] to keep their Python type.
    pub fn new(pattern: &str, flags: Flags) -> Result<Regex, RegexError> {
        Self::compile(pattern, flags).map_err(|e| match e {
            Error::Regex(r) => r,
            other => RegexError::new(other.to_string(), pattern, None),
        })
    }

    /// `re.compile(pattern, flags)` with exact Python exception types:
    /// `Error::Regex` (`re.error`), `Error::Overflow` (repeat count too large)
    /// or `Error::Value` (incompatible flags).
    pub fn compile(pattern: &str, flags: Flags) -> Result<Regex, Error> {
        Self::compile_nested(pattern, flags, parser::DEFAULT_MAX_NESTED)
    }

    /// [`Regex::compile`] where group nesting needing more than `max_nested`
    /// of sre's parser frames (two per group level) fails as Python's
    /// `RecursionError` (see [`is_recursion`]). Upstream's limit is
    /// `sys.getrecursionlimit()` minus the Python frames already in use.
    ///
    /// # Errors
    /// The same as [`Regex::compile`], plus the recursion error.
    pub fn compile_nested(pattern: &str, flags: Flags, max_nested: usize) -> Result<Regex, Error> {
        let parsed = parser::parse(pattern, flags.bits(), max_nested)?;
        let prog =
            compile::compile(&parsed.seq, parsed.flags, &parsed.groupwidths).map_err(|mut e| {
                e.pattern = pattern.to_string();
                Error::Regex(e)
            })?;
        Ok(Regex {
            pattern: pattern.to_string(),
            flags: parsed.flags,
            groups: parsed.groups,
            groupindex: parsed.groupdict,
            prog: std::sync::Arc::new(prog),
        })
    }

    /// [`Regex::compile`] through a process-wide cache, like `re`'s
    /// `_cache`. Only successes are cached; a `Regex` is immutable, so
    /// sharing one is indistinguishable from compiling it again.
    ///
    /// # Errors
    /// The same as [`Regex::compile`].
    pub fn compile_cached(pattern: &str, flags: Flags) -> Result<Arc<Regex>, Error> {
        Self::compile_cached_nested(pattern, flags, || parser::DEFAULT_MAX_NESTED)
    }

    /// [`Regex::compile_cached`] with [`Regex::compile_nested`]'s limit,
    /// computed only on a cache miss. As with `re._cache`, a cached success
    /// is returned whatever the limit.
    ///
    /// # Errors
    /// The same as [`Regex::compile_nested`].
    pub fn compile_cached_nested(
        pattern: &str,
        flags: Flags,
        max_nested: impl FnOnce() -> usize,
    ) -> Result<Arc<Regex>, Error> {
        static CACHE: [BoundedCache<Regex>; 4] = [const { BoundedCache::new(512) }; 4];
        let slot = usize::from(flags.ignorecase) | (usize::from(flags.dotall) << 1);
        CACHE[slot].get_or_try_insert(pattern, || {
            Self::compile_nested(pattern, flags, max_nested())
        })
    }

    /// `Pattern.pattern`
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// `Pattern.flags` (includes `re.UNICODE` unless `(?a)` was used).
    pub fn flags_value(&self) -> u32 {
        self.flags
    }

    /// `Pattern.groups`
    pub fn groups(&self) -> usize {
        self.groups
    }

    /// `Pattern.groupindex`
    pub fn groupindex(&self) -> &IndexMap<String, usize> {
        &self.groupindex
    }

    fn captures(&self, vm: &Vm<'_>, start: usize, end: usize) -> Captures {
        let mut spans = Vec::with_capacity(self.groups + 1);
        spans.push(Some((start, end)));
        for g in 0..self.groups {
            spans.push(vm.span(g));
        }
        Captures { spans }
    }

    fn anchored(&self) -> bool {
        matches!(
            self.prog.insts.first(),
            Some(Inst::At(
                compile::At::Beginning | compile::At::BeginningString
            ))
        )
    }

    /// `Pattern.match(text, pos, endpos)` with already clamped bounds.
    /// `pos > endpos` is allowed and only an empty match can succeed.
    pub fn match_at(&self, text: &[char], pos: usize, endpos: usize) -> Option<Captures> {
        self.match_mode(text, pos, endpos, Mode::default())
    }

    /// `Pattern.fullmatch(text, pos, endpos)`.
    pub fn fullmatch(&self, text: &[char], pos: usize, endpos: usize) -> Option<Captures> {
        self.match_mode(
            text,
            pos,
            endpos,
            Mode {
                match_all: true,
                must_advance: false,
            },
        )
    }

    fn match_mode(&self, text: &[char], pos: usize, endpos: usize, mode: Mode) -> Option<Captures> {
        let end = endpos.min(text.len());
        let mut vm = Vm::new(&self.prog, self.groups);
        let e = vm.run(text, pos, end, mode)?;
        Some(self.captures(&vm, pos, e))
    }

    /// `Pattern.search(text, pos, endpos)` with already clamped bounds.
    pub fn search(&self, text: &[char], pos: usize, endpos: usize) -> Option<Captures> {
        self.search_from(text, pos, endpos, false)
    }

    /// Search as `finditer` does after an empty match: with `must_advance`,
    /// a match that is empty at `pos` is rejected.
    pub fn search_from(
        &self,
        text: &[char],
        pos: usize,
        endpos: usize,
        must_advance: bool,
    ) -> Option<Captures> {
        let end = endpos.min(text.len());
        if pos > end {
            return None;
        }
        let mut vm = Vm::new(&self.prog, self.groups);
        let last = if self.anchored() { pos } else { end };
        let first = self.prog.first_matcher();
        for start in pos..=last {
            // A run from here would fail on its first character.
            if let Some(m) = first {
                if !(start < end && text.get(start).is_some_and(|&c| m.matches(c))) {
                    continue;
                }
            }
            let mode = Mode {
                match_all: false,
                must_advance: must_advance && start == pos,
            };
            if let Some(e) = vm.run(text, start, end, mode) {
                return Some(self.captures(&vm, start, e));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests;
