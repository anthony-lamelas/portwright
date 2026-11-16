//! Frame depths `re.DEBUG` reaches in `SubPattern.dump()` and
//! `_compiler.dis(code)`. The printed text itself isn't reproduced (see
//! `decisions.md`), but deep patterns still raise `RecursionError` there.

use crate::parser::Node;
use crate::sre::{self, op};

/// Deepest Python frame `p.dump()` reaches when it runs at depth `depth`:
/// each item prints its opcode (`_NamedIntConstant.__repr__`, one frame down)
/// and nested subpatterns dump one frame down.
pub fn dump_depth(p: &[Node], depth: usize) -> usize {
    let child = |sub: &[Node]| dump_depth(sub, depth + 1);
    p.iter()
        .map(|node| match node {
            Node::Branch(items) => items.iter().map(|i| child(i)).max().unwrap_or(depth + 1),
            Node::GroupRefExists { yes, no, .. } => {
                child(yes).max(no.as_deref().map_or(depth + 1, child))
            }
            Node::AtomicGroup(sub)
            | Node::Subpattern { p: sub, .. }
            | Node::Repeat { item: sub, .. }
            | Node::Assert { p: sub, .. } => child(sub),
            _ => depth + 1,
        })
        .fold(depth, usize::max)
}

/// Walks `code` the way `_compiler.dis` does, tracking frame depth.
struct Dis<'a> {
    code: &'a [u32],
    /// Deepest Python frame reached so far.
    deepest: usize,
}

impl Dis<'_> {
    fn word(&self, i: usize) -> usize {
        self.code.get(i).map_or(0, |&w| w as usize)
    }

    /// `dis_(start, end)` running at Python frame depth `depth`.
    fn run(&mut self, start: usize, end: usize, depth: usize) {
        if start < end {
            // `print_(op, ...)` prints an opcode: `_NamedIntConstant.__repr__`
            // runs two frames down.
            self.deepest = self.deepest.max(depth + 2);
        }
        let mut i = start;
        while i < end && i < self.code.len() {
            let o = self.code.get(i).copied().unwrap_or(0);
            i += 1;
            match o {
                op::SUCCESS
                | op::FAILURE
                | op::ANY
                | op::ANY_ALL
                | op::MAX_UNTIL
                | op::MIN_UNTIL
                | op::NEGATE => {}
                op::LITERAL
                | op::NOT_LITERAL
                | op::LITERAL_IGNORE
                | op::NOT_LITERAL_IGNORE
                | op::LITERAL_UNI_IGNORE
                | op::NOT_LITERAL_UNI_IGNORE
                | op::LITERAL_LOC_IGNORE
                | op::NOT_LITERAL_LOC_IGNORE
                | op::AT
                | op::CATEGORY
                | op::MARK
                | op::GROUPREF
                | op::GROUPREF_IGNORE
                | op::GROUPREF_UNI_IGNORE
                | op::GROUPREF_LOC_IGNORE
                | op::JUMP => i += 1,
                op::RANGE | op::RANGE_UNI_IGNORE | op::GROUPREF_EXISTS => i += 2,
                op::CHARSET => i += 8,
                op::BIGCHARSET => i += 1 + 64 + 8 * self.word(i),
                op::IN | op::IN_IGNORE | op::IN_UNI_IGNORE | op::IN_LOC_IGNORE => {
                    let to = i + self.word(i);
                    self.run(i + 1, to, depth + 1);
                    i = to;
                }
                op::BRANCH => {
                    let mut skip = self.word(i);
                    while skip != 0 {
                        self.run(i + 1, i + skip, depth + 1);
                        i += skip;
                        skip = self.word(i);
                    }
                    i += 1;
                }
                op::REPEAT
                | op::REPEAT_ONE
                | op::MIN_REPEAT_ONE
                | op::POSSESSIVE_REPEAT
                | op::POSSESSIVE_REPEAT_ONE => {
                    let to = i + self.word(i);
                    self.run(i + 3, to, depth + 1);
                    i = to;
                }
                op::ASSERT | op::ASSERT_NOT => {
                    let to = i + self.word(i);
                    self.run(i + 2, to, depth + 1);
                    i = to;
                }
                op::ATOMIC_GROUP => {
                    let to = i + self.word(i);
                    self.run(i + 1, to, depth + 1);
                    i = to;
                }
                op::INFO => {
                    let to = i + self.word(i);
                    let fl = self.word(i + 1);
                    let mut st = i + 4;
                    if fl & sre::INFO_PREFIX as usize != 0 {
                        st = i + 6 + 2 * self.word(i + 4);
                    }
                    if fl & sre::INFO_CHARSET as usize != 0 {
                        self.run(st, to, depth + 1);
                    }
                    i = to;
                }
                _ => break,
            }
        }
    }
}

/// Deepest Python frame `_compiler.dis(code)` reaches, with
/// `dis_(0, len(code))` at depth 0 (see [`crate::frames`]).
pub fn dis_depth(code: &[u32]) -> usize {
    let mut d = Dis { code, deepest: 0 };
    d.run(0, code.len(), 0);
    d.deepest
}
