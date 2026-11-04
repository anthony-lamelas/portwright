//! Repeated searches over one subject: `_sre`'s `findall`, scanner
//! (`finditer`) and `split`.

use std::sync::Arc;

use crate::engine::State;
use crate::matching::Match;
use crate::pattern::Pattern;
use crate::text::PyStr;
use crate::value::Value;

/// A scan over `string[pos:endpos]` that follows `_sre`'s scanner rules:
/// after an empty match the next one must advance (`must_advance`).
pub(crate) struct Scanner<'s> {
    code: &'s [u32],
    first: Option<&'s crate::engine::FirstSet>,
    pub(crate) state: State<'s>,
    done: bool,
}

impl<'s> Scanner<'s> {
    pub(crate) fn new(pattern: &'s Pattern, string: &'s [u32], pos: usize, endpos: usize) -> Self {
        Self {
            code: &pattern.code,
            first: pattern.first.as_ref(),
            state: State::new(string, pos, endpos, pattern.groups, &pattern.sets),
            done: false,
        }
    }

    /// The next match, leaving its spans in `self.state`. Returns `false`
    /// once no further match exists.
    pub(crate) fn next_match(&mut self) -> crate::error::Result<bool> {
        if self.done || self.state.start > self.state.end {
            self.done = true;
            return Ok(false);
        }
        self.state.reset();
        self.state.ptr = self.state.start;
        if !self.state.search(self.code, self.first)? {
            self.done = true;
            return Ok(false);
        }
        Ok(true)
    }

    /// The scanner's step after a match was consumed.
    pub(crate) fn advance(&mut self) {
        self.state.must_advance = self.state.ptr == self.state.start;
        self.state.start = self.state.ptr;
    }

    fn slice(&self, string: &[u32], (a, b): (usize, usize)) -> PyStr {
        string.get(a..b).map(<[u32]>::to_vec).unwrap_or_default()
    }

    /// `state_getslice(state, g, string, empty)`: the text of group `g`, or
    /// `''` / `None` when it didn't take part.
    fn group(&self, string: &[u32], g: usize, empty: bool) -> Value {
        match self.state.group_span(g) {
            Some(span) => Value::Str(self.slice(string, span)),
            None if empty => Value::Str(PyStr::new()),
            None => Value::None,
        }
    }
}

/// `pattern.findall(string, pos, endpos)` with a clamped window.
pub fn findall(
    pattern: &Pattern,
    string: &[u32],
    pos: usize,
    endpos: usize,
) -> crate::error::Result<Vec<Value>> {
    let mut scanner = Scanner::new(pattern, string, pos, endpos);
    let mut out = Vec::new();
    while scanner.next_match()? {
        let item = match pattern.groups {
            0 => Value::Str(scanner.slice(string, (scanner.state.start, scanner.state.ptr))),
            1 => scanner.group(string, 1, true),
            n => Value::Tuple((1..=n).map(|g| scanner.group(string, g, true)).collect()),
        };
        out.push(item);
        scanner.advance();
    }
    Ok(out)
}

/// `list(pattern.finditer(string, pos, endpos))` with a clamped window.
pub fn finditer(
    pattern: &Arc<Pattern>,
    string: &Arc<[u32]>,
    pos: usize,
    endpos: usize,
) -> crate::error::Result<Vec<Match>> {
    let mut scanner = Scanner::new(pattern, string, pos, endpos);
    let mut out = Vec::new();
    while scanner.next_match()? {
        out.push(Match::from_state(
            pattern,
            string,
            &scanner.state,
            pos,
            endpos,
        ));
        scanner.advance();
    }
    Ok(out)
}

/// `pattern.split(string, maxsplit)`: `maxsplit == 0` splits everywhere and
/// a negative `maxsplit` doesn't split at all.
pub fn split(
    pattern: &Pattern,
    string: &[u32],
    maxsplit: isize,
) -> crate::error::Result<Vec<Value>> {
    let mut scanner = Scanner::new(pattern, string, 0, string.len());
    let mut out = Vec::new();
    let mut last = 0;
    let mut n: isize = 0;
    while maxsplit == 0 || n < maxsplit {
        if !scanner.next_match()? {
            break;
        }
        out.push(Value::Str(
            scanner.slice(string, (last, scanner.state.start)),
        ));
        for g in 1..=pattern.groups {
            out.push(scanner.group(string, g, false));
        }
        n += 1;
        scanner.advance();
        last = scanner.state.start;
    }
    out.push(Value::Str(scanner.slice(string, (last, string.len()))));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pyrepr;
    use crate::text::{cps, to_string_lossy};
    use num_bigint::BigInt;

    fn show(values: &[Value]) -> String {
        let reprs: Vec<String> = values
            .iter()
            .map(|v| {
                pyrepr::repr(v)
                    .map(|t| to_string_lossy(&t))
                    .unwrap_or_default()
            })
            .collect();
        reprs.join(", ")
    }

    fn compile(p: &str) -> Result<Pattern, String> {
        Pattern::compile(&cps(p), &BigInt::from(0)).map_err(|e| e.to_string())
    }

    #[test]
    fn findall_and_split() -> Result<(), String> {
        let p = compile("x*")?;
        let s = cps("axxb");
        assert_eq!(
            show(&findall(&p, &s, 0, 4).map_err(|e| e.to_string())?),
            "'', 'xx', '', ''"
        );
        assert_eq!(
            show(&split(&p, &s, 0).map_err(|e| e.to_string())?),
            "'', 'a', '', 'b', ''"
        );
        let p = compile("(a)|(b)")?;
        let s = cps("ab-");
        assert_eq!(
            show(&findall(&p, &s, 0, 3).map_err(|e| e.to_string())?),
            "('a', ''), ('', 'b')"
        );
        assert_eq!(
            show(&split(&p, &s, 1).map_err(|e| e.to_string())?),
            "'', 'a', None, 'b-'"
        );
        assert_eq!(
            show(&split(&p, &s, -1).map_err(|e| e.to_string())?),
            "'ab-'"
        );
        Ok(())
    }
}
