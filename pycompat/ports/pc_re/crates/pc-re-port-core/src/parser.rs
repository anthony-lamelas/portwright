//! Port of CPython 3.12 `re._parser` for str patterns: sre's parse tree, with
//! the original's error messages, positions and optimizations.

use indexmap::IndexMap;

use crate::alpha;
use crate::error::{Error, ReError, Result};
use crate::flags;
use crate::frames::frame;
use crate::pyrepr;
use crate::sre::{at, ch, op, MAXGROUPS, MAXREPEAT};
use crate::text::{cps, push_str, PyStr};
use crate::unicode;

/// `_parser.MAXWIDTH`: larger than any width sre can represent.
pub const MAXWIDTH: u128 = 1 << 64;

/// Flags that may only be turned on for the whole pattern.
const GLOBAL_FLAGS: u32 = flags::DEBUG | flags::TEMPLATE;
/// `a`, `u` and `L`.
const TYPE_FLAGS: u32 = flags::ASCII | flags::LOCALE | flags::UNICODE;

/// One member of a character set (`IN`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SetItem {
    /// `NEGATE`
    Negate,
    /// `LITERAL c`
    Literal(u32),
    /// `RANGE (lo, hi)`
    Range(u32, u32),
    /// `CATEGORY code`
    Category(u32),
    /// `CHARSET` bitmap words (compiler only).
    Charset(Vec<u32>),
    /// `BIGCHARSET` words (compiler only).
    BigCharset(Vec<u32>),
    /// `RANGE_UNI_IGNORE (lo, hi)` (compiler only).
    RangeUniIgnore(u32, u32),
}

/// A parsed subpattern: sre's `SubPattern.data`.
pub type SubPattern = Vec<Node>;

/// One `(op, av)` item of the parse tree.
#[derive(Debug, Clone)]
pub enum Node {
    /// `LITERAL c`
    Literal(u32),
    /// `NOT_LITERAL c`
    NotLiteral(u32),
    /// `ANY`
    Any,
    /// `IN [...]`
    In(Vec<SetItem>),
    /// `AT code`
    At(u32),
    /// `BRANCH (None, [...])`
    Branch(Vec<SubPattern>),
    /// `SUBPATTERN (group, add_flags, del_flags, p)`
    Subpattern {
        /// Group number, if capturing.
        group: Option<usize>,
        /// Inline flags turned on.
        add_flags: u32,
        /// Inline flags turned off.
        del_flags: u32,
        /// Contents.
        p: SubPattern,
    },
    /// `MAX_REPEAT` / `MIN_REPEAT` / `POSSESSIVE_REPEAT (min, max, item)`.
    Repeat {
        /// One of `op::MAX_REPEAT`, `op::MIN_REPEAT`, `op::POSSESSIVE_REPEAT`.
        kind: u32,
        /// Minimum count.
        min: u32,
        /// Maximum count (`MAXREPEAT` for unbounded).
        max: u32,
        /// Repeated item.
        item: SubPattern,
    },
    /// `ATOMIC_GROUP p`
    AtomicGroup(SubPattern),
    /// `ASSERT` / `ASSERT_NOT (dir, p)`.
    Assert {
        /// `ASSERT_NOT` when true.
        negate: bool,
        /// `1` lookahead, `-1` lookbehind.
        dir: i32,
        /// Contents.
        p: SubPattern,
    },
    /// `GROUPREF g`
    GroupRef(usize),
    /// `GROUPREF_EXISTS (g, yes, no)`
    GroupRefExists {
        /// Referenced group.
        group: usize,
        /// Branch taken when the group matched.
        yes: SubPattern,
        /// Branch taken otherwise.
        no: Option<SubPattern>,
    },
}

impl Node {
    /// Python `==` between two items when factoring a branch prefix: items
    /// holding subpatterns compare by identity, so they never compare equal.
    fn same(&self, other: &Node) -> bool {
        match (self, other) {
            (Node::Literal(a), Node::Literal(b)) | (Node::NotLiteral(a), Node::NotLiteral(b)) => {
                a == b
            }
            (Node::Any, Node::Any) => true,
            (Node::In(a), Node::In(b)) => a == b,
            (Node::At(a), Node::At(b)) => a == b,
            (Node::GroupRef(a), Node::GroupRef(b)) => a == b,
            _ => false,
        }
    }
}

/// Parser state (`_parser.State`).
#[derive(Debug, Clone, Default)]
pub struct State {
    /// Low bits of the global flags (all `re` flags fit in them).
    pub flags: u32,
    /// Group names to numbers, in definition order.
    pub groupdict: IndexMap<PyStr, usize>,
    /// Widths of closed groups (`None` while open); index 0 is the whole match.
    pub groupwidths: Vec<Option<(u128, u128)>>,
    lookbehindgroups: Option<usize>,
    grouprefpos: IndexMap<usize, usize>,
}

impl State {
    /// Number of groups including group 0.
    pub fn groups(&self) -> usize {
        self.groupwidths.len()
    }

    fn checkgroup(&self, gid: usize) -> bool {
        matches!(self.groupwidths.get(gid), Some(Some(_)))
    }
}

/// Width `(min, max)` of a subpattern (`SubPattern.getwidth`).
pub fn getwidth(p: &[Node], groupwidths: &[Option<(u128, u128)>]) -> (u128, u128) {
    let mut lo: u128 = 0;
    let mut hi: u128 = 0;
    for node in p {
        match node {
            Node::Branch(items) => {
                let mut i = MAXWIDTH;
                let mut j = 0;
                for item in items {
                    let (l, h) = getwidth(item, groupwidths);
                    i = i.min(l);
                    j = j.max(h);
                }
                lo = lo.saturating_add(i);
                hi = hi.saturating_add(j);
            }
            Node::AtomicGroup(sub) | Node::Subpattern { p: sub, .. } => {
                let (i, j) = getwidth(sub, groupwidths);
                lo = lo.saturating_add(i);
                hi = hi.saturating_add(j);
            }
            Node::Repeat { min, max, item, .. } => {
                let (i, j) = getwidth(item, groupwidths);
                lo = lo.saturating_add(i.saturating_mul(u128::from(*min)));
                if *max == MAXREPEAT && j != 0 {
                    hi = MAXWIDTH;
                } else {
                    hi = hi.saturating_add(j.saturating_mul(u128::from(*max)));
                }
            }
            Node::Any | Node::In(_) | Node::Literal(_) | Node::NotLiteral(_) => {
                lo = lo.saturating_add(1);
                hi = hi.saturating_add(1);
            }
            Node::GroupRef(g) => {
                let (i, j) = groupwidths.get(*g).copied().flatten().unwrap_or((0, 0));
                lo = lo.saturating_add(i);
                hi = hi.saturating_add(j);
            }
            Node::GroupRefExists { yes, no, .. } => {
                let (mut i, mut j) = getwidth(yes, groupwidths);
                if let Some(no) = no {
                    let (l, h) = getwidth(no, groupwidths);
                    i = i.min(l);
                    j = j.max(h);
                } else {
                    i = 0;
                }
                lo = lo.saturating_add(i);
                hi = hi.saturating_add(j);
            }
            Node::At(_) | Node::Assert { .. } => {}
        }
    }
    (lo.min(MAXWIDTH), hi.min(MAXWIDTH))
}

/// Deepest Python frame `SubPattern.getwidth` reaches when called on `p` at
/// relative depth `depth`, where `p`'s width isn't cached yet.
///
/// Python caches a width on each `SubPattern` object it computes. When
/// `getwidth` runs, every capturing group's contents are already cached (by
/// `closegroup`), and nothing else is: other subpatterns are only reached
/// through traversals that start at a capturing group, `_compile_info`'s
/// top-level call or a look-behind, and those never overlap.
pub fn getwidth_depth(p: &[Node], depth: usize) -> usize {
    let child = |sub: &[Node]| getwidth_depth(sub, depth + 1);
    p.iter()
        .map(|node| match node {
            Node::Branch(items) => items.iter().map(|i| child(i)).max().unwrap_or(depth),
            Node::Subpattern { group: Some(_), .. } => depth + 1,
            Node::AtomicGroup(sub) | Node::Subpattern { p: sub, .. } => child(sub),
            Node::Repeat { item, .. } => child(item),
            Node::GroupRefExists { yes, no, .. } => {
                child(yes).max(no.as_deref().map_or(depth, child))
            }
            _ => depth,
        })
        .fold(depth, usize::max)
}

/// One token: a character, or a backslash and the character after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Tok {
    c: u32,
    esc: Option<u32>,
}

impl Tok {
    fn len(self) -> usize {
        if self.esc.is_some() {
            2
        } else {
            1
        }
    }

    fn is(self, ch: char) -> bool {
        self.esc.is_none() && self.c == u32::from(ch)
    }

    fn is_in(self, set: &str) -> bool {
        self.esc.is_none() && set.chars().any(|ch| self.c == u32::from(ch))
    }

    fn text(self) -> PyStr {
        let mut out = vec![self.c];
        out.extend(self.esc);
        out
    }
}

const DIGITS: &str = "0123456789";
const OCTDIGITS: &str = "01234567";
const HEXDIGITS: &str = "0123456789abcdefABCDEF";
const WHITESPACE: &str = " \t\n\r\x0b\x0c";
const SPECIAL_CHARS: &str = ".\\[{()*+?^$|";
const REPEAT_CHARS: &str = "*+?{";

fn is_ascii_letter(c: u32) -> bool {
    c < 128 && (c as u8).is_ascii_alphabetic()
}

/// `FLAGS[char]` for inline flag letters.
fn flag_of(t: Tok) -> Option<u32> {
    if t.esc.is_some() {
        return None;
    }
    Some(match char::from_u32(t.c)? {
        'i' => flags::IGNORECASE,
        'L' => flags::LOCALE,
        'm' => flags::MULTILINE,
        's' => flags::DOTALL,
        'x' => flags::VERBOSE,
        'a' => flags::ASCII,
        't' => flags::TEMPLATE,
        'u' => flags::UNICODE,
        _ => return None,
    })
}

/// `_parser.Tokenizer`.
struct Source<'a> {
    s: &'a [u32],
    index: usize,
    next: Option<Tok>,
    /// Python frame depth of the parser function using the tokenizer now
    /// (see [`crate::frames`]). Its methods run one frame deeper, and
    /// `__next`, `tell` and `error.__init__` two frames deeper.
    depth: usize,
}

fn msg_with(prefix: &str, text: &[u32]) -> PyStr {
    let mut m = cps(prefix);
    m.extend_from_slice(text);
    m
}

fn msg_repr(prefix: &str, text: &[u32]) -> PyStr {
    msg_with(prefix, &pyrepr::str_repr(text))
}

/// `int(digits)` for ASCII digits, with CPython's digit limit; `None` means the
/// value is larger than `u64::MAX`.
fn int_digits(digits: &[u32]) -> Result<Option<u64>> {
    if digits.len() > 4300 {
        return Err(Error::Value(format!(
            "Exceeds the limit (4300 digits) for integer string conversion: value has {} \
             digits; use sys.set_int_max_str_digits() to increase the limit",
            digits.len()
        )));
    }
    let mut v: u64 = 0;
    for &d in digits {
        let Some(next) = v
            .checked_mul(10)
            .and_then(|v| v.checked_add(u64::from(d.wrapping_sub(0x30))))
        else {
            return Ok(None);
        };
        v = next;
    }
    Ok(Some(v))
}

/// Decimal text of ASCII digits without leading zeros (Python's `%d` of `int(...)`).
fn digits_text(digits: &[u32]) -> String {
    let s: String = digits
        .iter()
        .filter_map(|&c| char::from_u32(c))
        .collect::<String>();
    let t = s.trim_start_matches('0');
    if t.is_empty() {
        "0".to_owned()
    } else {
        t.to_owned()
    }
}

impl<'a> Source<'a> {
    fn new(s: &'a [u32]) -> Result<Self> {
        let mut src = Source {
            s,
            index: 0,
            next: None,
            depth: 0,
        };
        src.advance()?;
        Ok(src)
    }

    /// Runs `f` with the tokenizer used from depth `depth`.
    fn within<T>(&mut self, depth: usize, f: impl FnOnce(&mut Self) -> T) -> T {
        let saved = std::mem::replace(&mut self.depth, depth);
        let out = f(self);
        self.depth = saved;
        out
    }

    /// `__next`, called from a tokenizer method.
    fn advance(&mut self) -> Result<()> {
        frame(self.depth + 2)?;
        let Some(&c) = self.s.get(self.index) else {
            self.next = None;
            return Ok(());
        };
        let mut index = self.index;
        let mut esc = None;
        if c == 0x5C {
            index += 1;
            match self.s.get(index) {
                Some(&e) => esc = Some(e),
                None => {
                    return Err(ReError::new(
                        cps("bad escape (end of pattern)"),
                        self.s,
                        self.s.len().saturating_sub(1),
                    )
                    .into())
                }
            }
        }
        self.index = index + 1;
        self.next = Some(Tok { c, esc });
        Ok(())
    }

    fn next_is(&self, ch: char) -> bool {
        self.next.is_some_and(|t| t.is(ch))
    }

    fn next_in(&self, set: &str) -> bool {
        self.next.is_some_and(|t| t.is_in(set))
    }

    fn matches(&mut self, ch: char) -> Result<bool> {
        if self.next_is(ch) {
            self.advance()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn get(&mut self) -> Result<Option<Tok>> {
        let this = self.next;
        self.advance()?;
        Ok(this)
    }

    fn getwhile(&mut self, n: usize, set: &str) -> Result<PyStr> {
        let mut result = PyStr::new();
        for _ in 0..n {
            match self.next {
                Some(t) if t.is_in(set) => {
                    result.push(t.c);
                    self.advance()?;
                }
                _ => break,
            }
        }
        Ok(result)
    }

    fn getuntil(&mut self, terminator: char, name: &str) -> Result<PyStr> {
        let mut result = PyStr::new();
        loop {
            let c = self.next;
            self.advance()?;
            match c {
                None => {
                    if result.is_empty() {
                        return Err(self.inner_error(cps(&format!("missing {name}")), 0));
                    }
                    return Err(self.inner_error(
                        cps(&format!("missing {terminator}, unterminated name")),
                        result.len(),
                    ));
                }
                Some(t) if t.is(terminator) => {
                    if result.is_empty() {
                        return Err(self.inner_error(cps(&format!("missing {name}")), 1));
                    }
                    break;
                }
                Some(t) => result.extend(t.text()),
            }
        }
        Ok(result)
    }

    fn tell(&self) -> usize {
        self.index - self.next.map_or(0, Tok::len)
    }

    fn seek(&mut self, index: usize) -> Result<()> {
        self.index = index;
        self.advance()
    }

    /// `source.error(msg, offset)`; its `tell` and `error.__init__` run two
    /// frames down, so this can be a `RecursionError` instead.
    fn error(&self, msg: PyStr, offset: usize) -> Error {
        self.error_from(self.depth + 2, msg, offset)
    }

    /// `self.error(msg, offset)` raised inside a tokenizer method.
    fn inner_error(&self, msg: PyStr, offset: usize) -> Error {
        self.error_from(self.depth + 3, msg, offset)
    }

    fn error_from(&self, deepest: usize, msg: PyStr, offset: usize) -> Error {
        match frame(deepest) {
            Ok(()) => ReError::new(msg, self.s, self.tell().saturating_sub(offset)).into(),
            Err(e) => e,
        }
    }

    fn checkgroupname(&self, name: &[u32], offset: usize) -> Result<()> {
        if !unicode::is_identifier(name) {
            return Err(self.inner_error(
                msg_repr("bad character in group name ", name),
                name.len() + offset,
            ));
        }
        Ok(())
    }
}

fn hex_value(digits: &[u32]) -> u32 {
    digits.iter().fold(0u32, |acc, &d| {
        let v = char::from_u32(d).and_then(|c| c.to_digit(16)).unwrap_or(0);
        acc.wrapping_mul(16).wrapping_add(v)
    })
}

fn oct_value(digits: &[u32]) -> u32 {
    digits
        .iter()
        .fold(0u32, |acc, &d| acc * 8 + d.wrapping_sub(0x30))
}

/// The escapes shared by `_class_escape` and `_escape` (`\x`, `\u`, `\U`, `\N`).
/// `Ok(None)` means "not one of these".
fn common_escape(source: &mut Source<'_>, escape: &mut PyStr, c: u32) -> Result<Option<u32>> {
    let (want, count) = match char::from_u32(c) {
        Some('x') => (4, 2),
        Some('u') => (6, 4),
        Some('U') => (10, 8),
        Some('N') => {
            if !source.matches('{')? {
                return Err(source.error(cps("missing {"), 0));
            }
            let charname = source.getuntil('}', "character name")?;
            // `unicodedata.lookup` UTF-8-encodes the name first; a lone surrogate
            // raises UnicodeEncodeError (a ValueError, not KeyError), which the
            // caller's `except ValueError` reports as "bad escape \N".
            if charname.iter().any(|&u| (0xD800..=0xDFFF).contains(&u)) {
                return Err(source.error(msg_with("bad escape ", escape), escape.len()));
            }
            return match unicode::lookup_name(&charname) {
                Some(c) => Ok(Some(c)),
                None => Err(source.error(
                    msg_repr("undefined character name ", &charname),
                    charname.len() + 4,
                )),
            };
        }
        _ => return Ok(None),
    };
    escape.extend(source.getwhile(count, HEXDIGITS)?);
    if escape.len() != want {
        return Err(source.error(msg_with("incomplete escape ", escape), escape.len()));
    }
    let value = hex_value(escape.get(2..).unwrap_or(&[]));
    if want == 10 && value > 0x10FFFF {
        return Err(source.error(msg_with("bad escape ", escape), escape.len()));
    }
    Ok(Some(value))
}

/// What an escape inside a set stands for.
enum ClassCode {
    Literal(u32),
    Category(u32),
}

fn category_escape(e: u32) -> Option<u32> {
    Some(match char::from_u32(e)? {
        'd' => ch::DIGIT,
        'D' => ch::NOT_DIGIT,
        's' => ch::SPACE,
        'S' => ch::NOT_SPACE,
        'w' => ch::WORD,
        'W' => ch::NOT_WORD,
        _ => return None,
    })
}

fn simple_escape(e: u32) -> Option<u32> {
    Some(match char::from_u32(e)? {
        'a' => 7,
        'b' => 8,
        'f' => 12,
        'n' => 10,
        'r' => 13,
        't' => 9,
        'v' => 11,
        '\\' => 0x5C,
        _ => return None,
    })
}

fn class_escape(source: &mut Source<'_>, tok: Tok) -> Result<ClassCode> {
    let c = tok.esc.unwrap_or(tok.c);
    if let Some(v) = simple_escape(c) {
        return Ok(ClassCode::Literal(v));
    }
    if let Some(cat) = category_escape(c) {
        return Ok(ClassCode::Category(cat));
    }
    let mut escape = tok.text();
    if let Some(v) = common_escape(source, &mut escape, c)? {
        return Ok(ClassCode::Literal(v));
    }
    if (0x30..=0x37).contains(&c) {
        escape.extend(source.getwhile(2, OCTDIGITS)?);
        let v = oct_value(escape.get(1..).unwrap_or(&[]));
        if v > 0o377 {
            return Err(source.error(
                msg_with("octal escape value ", &escape)
                    .into_iter()
                    .chain(cps(" outside of range 0-0o377"))
                    .collect(),
                escape.len(),
            ));
        }
        return Ok(ClassCode::Literal(v));
    }
    if !(0x30..=0x39).contains(&c) && escape.len() == 2 && !is_ascii_letter(c) {
        return Ok(ClassCode::Literal(c));
    }
    Err(source.error(msg_with("bad escape ", &escape), escape.len()))
}

fn escape(source: &mut Source<'_>, tok: Tok, state: &State) -> Result<Node> {
    let c = tok.esc.unwrap_or(tok.c);
    match char::from_u32(c) {
        Some('A') => return Ok(Node::At(at::BEGINNING_STRING)),
        Some('b') => return Ok(Node::At(at::BOUNDARY)),
        Some('B') => return Ok(Node::At(at::NON_BOUNDARY)),
        Some('Z') => return Ok(Node::At(at::END_STRING)),
        _ => {}
    }
    if let Some(cat) = category_escape(c) {
        return Ok(Node::In(vec![SetItem::Category(cat)]));
    }
    if let Some(v) = simple_escape(c) {
        return Ok(Node::Literal(v));
    }
    let mut escape = tok.text();
    if let Some(v) = common_escape(source, &mut escape, c)? {
        return Ok(Node::Literal(v));
    }
    if c == 0x30 {
        escape.extend(source.getwhile(2, OCTDIGITS)?);
        return Ok(Node::Literal(oct_value(escape.get(1..).unwrap_or(&[]))));
    }
    if (0x31..=0x39).contains(&c) {
        if source.next_in(DIGITS) {
            if let Some(t) = source.get()? {
                escape.push(t.c);
            }
            let octal = |x: Option<&u32>| x.is_some_and(|&d| (0x30..=0x37).contains(&d));
            if octal(escape.get(1)) && octal(escape.get(2)) && source.next_in(OCTDIGITS) {
                if let Some(t) = source.get()? {
                    escape.push(t.c);
                }
                let v = oct_value(escape.get(1..).unwrap_or(&[]));
                if v > 0o377 {
                    return Err(source.error(
                        msg_with("octal escape value ", &escape)
                            .into_iter()
                            .chain(cps(" outside of range 0-0o377"))
                            .collect(),
                        escape.len(),
                    ));
                }
                return Ok(Node::Literal(v));
            }
        }
        let group = escape
            .get(1..)
            .unwrap_or(&[])
            .iter()
            .fold(0usize, |acc, &d| acc * 10 + (d as usize).wrapping_sub(0x30));
        // `state.groups` / `state.checkgroup(group)` (a property inside).
        frame(source.depth + 2)?;
        if group < state.groups() {
            if !state.checkgroup(group) {
                return Err(source.error(cps("cannot refer to an open group"), escape.len()));
            }
            checklookbehindgroup(state, group, source, source.depth + 1)?;
            return Ok(Node::GroupRef(group));
        }
        return Err(source.error(
            cps(&format!("invalid group reference {group}")),
            escape.len() - 1,
        ));
    }
    if escape.len() == 2 && !is_ascii_letter(c) {
        return Ok(Node::Literal(c));
    }
    Err(source.error(msg_with("bad escape ", &escape), escape.len()))
}

/// `state.checklookbehindgroup(gid, source)`, running at depth `depth`.
fn checklookbehindgroup(
    state: &State,
    gid: usize,
    source: &Source<'_>,
    depth: usize,
) -> Result<()> {
    if let Some(lb) = state.lookbehindgroups {
        // `self.checkgroup(gid)` reads the `groups` property; errors call
        // `source.error`, whose `tell` runs at the same depth.
        frame(depth + 2)?;
        if !state.checkgroup(gid) {
            return Err(source.error(cps("cannot refer to an open group"), 0));
        }
        if gid >= lb {
            return Err(source.error(
                cps("cannot refer to group defined in the same lookbehind subpattern"),
                0,
            ));
        }
    }
    Ok(())
}

/// `dict.fromkeys(items)`: drop later duplicates.
fn uniq(items: Vec<SetItem>) -> Vec<SetItem> {
    let mut out: Vec<SetItem> = Vec::with_capacity(items.len());
    for item in items {
        if !out.contains(&item) {
            out.push(item);
        }
    }
    out
}

fn parse_sub(
    source: &mut Source<'_>,
    state: &mut State,
    mut verbose: bool,
    nested: usize,
) -> Result<SubPattern> {
    frame(nested + 2)?;
    let mut items: Vec<SubPattern> = Vec::new();
    loop {
        let first = nested == 0 && items.is_empty();
        items.push(source.within(nested + 1, |s| parse(s, state, verbose, nested + 1, first))?);
        if !source.matches('|')? {
            break;
        }
        if nested == 0 {
            verbose = state.flags & flags::VERBOSE != 0;
        }
    }
    if items.len() == 1 {
        return Ok(items.pop().unwrap_or_default());
    }
    let mut subpattern = SubPattern::new();
    // check if all items share a common prefix
    while let Some(prefix) = items.first().and_then(|i| i.first()) {
        let all = items
            .iter()
            .enumerate()
            .all(|(k, item)| k == 0 || item.first().is_some_and(|n| n.same(prefix)));
        if !all {
            break;
        }
        let mut taken = None;
        for item in &mut items {
            let n = item.remove(0);
            if taken.is_none() {
                taken = Some(n);
            }
        }
        subpattern.extend(taken);
    }
    // check if the branch can be replaced by a character set
    let mut set = Vec::new();
    let mut ok = true;
    for item in &items {
        match item.as_slice() {
            [Node::Literal(c)] => set.push(SetItem::Literal(*c)),
            [Node::In(av)] if av.first() != Some(&SetItem::Negate) => {
                set.extend(av.iter().cloned());
            }
            _ => {
                ok = false;
                break;
            }
        }
    }
    if ok {
        subpattern.push(Node::In(uniq(set)));
        return Ok(subpattern);
    }
    subpattern.push(Node::Branch(items));
    Ok(subpattern)
}

fn range_error(source: &Source<'_>, this: Tok, that: Tok) -> Error {
    let mut m = cps("bad character range ");
    m.extend(this.text());
    m.push(u32::from('-'));
    m.extend(that.text());
    source.error(m, this.len() + 1 + that.len())
}

fn parse_set(source: &mut Source<'_>) -> Result<Node> {
    let here = source.tell() - 1;
    let negate = source.matches('^')?;
    let mut set: Vec<SetItem> = Vec::new();
    let to_item = |code: ClassCode| match code {
        ClassCode::Literal(c) => SetItem::Literal(c),
        ClassCode::Category(c) => SetItem::Category(c),
    };
    loop {
        let Some(this) = source.get()? else {
            return Err(source.error(cps("unterminated character set"), source.tell() - here));
        };
        if this.is(']') && !set.is_empty() {
            break;
        }
        let code1 = if this.esc.is_some() {
            source.within(source.depth + 1, |s| class_escape(s, this))?
        } else {
            ClassCode::Literal(this.c)
        };
        if source.matches('-')? {
            let Some(that) = source.get()? else {
                return Err(source.error(cps("unterminated character set"), source.tell() - here));
            };
            if that.is(']') {
                set.push(to_item(code1));
                set.push(SetItem::Literal(u32::from('-')));
                break;
            }
            let code2 = if that.esc.is_some() {
                source.within(source.depth + 1, |s| class_escape(s, that))?
            } else {
                ClassCode::Literal(that.c)
            };
            match (code1, code2) {
                (ClassCode::Literal(lo), ClassCode::Literal(hi)) => {
                    if hi < lo {
                        return Err(range_error(source, this, that));
                    }
                    set.push(SetItem::Range(lo, hi));
                }
                _ => return Err(range_error(source, this, that)),
            }
        } else {
            set.push(to_item(code1));
        }
    }
    let mut set = uniq(set);
    if let [SetItem::Literal(c)] = set.as_slice() {
        return Ok(if negate {
            Node::NotLiteral(*c)
        } else {
            Node::Literal(*c)
        });
    }
    if negate {
        set.insert(0, SetItem::Negate);
    }
    Ok(Node::In(set))
}

/// Parses `{m,n}` after `{`; `Ok(None)` means "a literal `{`" (already handled).
fn parse_braces(source: &mut Source<'_>, here: usize) -> Result<Option<(u32, u32)>> {
    let mut lo = PyStr::new();
    let mut hi = PyStr::new();
    while source.next_in(DIGITS) {
        if let Some(t) = source.get()? {
            lo.push(t.c);
        }
    }
    if source.matches(',')? {
        while source.next_in(DIGITS) {
            if let Some(t) = source.get()? {
                hi.push(t.c);
            }
        }
    } else {
        hi.clone_from(&lo);
    }
    if !source.matches('}')? {
        source.seek(here)?;
        return Ok(None);
    }
    let too_large = || Error::Overflow("the repetition number is too large".into());
    let mut min = 0u32;
    let mut max = MAXREPEAT;
    if !lo.is_empty() {
        match int_digits(&lo)? {
            Some(v) if v < u64::from(MAXREPEAT) => min = u32::try_from(v).unwrap_or(MAXREPEAT),
            _ => return Err(too_large()),
        }
    }
    if !hi.is_empty() {
        match int_digits(&hi)? {
            Some(v) if v < u64::from(MAXREPEAT) => max = u32::try_from(v).unwrap_or(MAXREPEAT),
            _ => return Err(too_large()),
        }
        if max < min {
            return Err(source.error(
                cps("min repeat greater than max repeat"),
                source.tell() - here,
            ));
        }
    }
    Ok(Some((min, max)))
}

#[expect(
    clippy::too_many_lines,
    reason = "mirrors _parser._parse, one branch per special character"
)]
fn parse(
    source: &mut Source<'_>,
    state: &mut State,
    mut verbose: bool,
    nested: usize,
    first: bool,
) -> Result<SubPattern> {
    frame(nested + 1)?;
    let mut subpattern = SubPattern::new();
    while let Some(this) = source.next {
        if this.is('|') || this.is(')') {
            break;
        }
        frame(nested + 2)?;
        source.advance()?;

        if verbose {
            if this.is_in(WHITESPACE) {
                continue;
            }
            if this.is('#') {
                loop {
                    match source.get()? {
                        None => break,
                        Some(t) if t.is('\n') => break,
                        Some(_) => {}
                    }
                }
                continue;
            }
        }

        if this.esc.is_some() {
            subpattern.push(source.within(nested + 1, |s| escape(s, this, state))?);
        } else if !this.is_in(SPECIAL_CHARS) {
            subpattern.push(Node::Literal(this.c));
        } else if this.is('[') {
            subpattern.push(parse_set(source)?);
        } else if this.is_in(REPEAT_CHARS) {
            let here = source.tell();
            let (min, max) = match char::from_u32(this.c) {
                Some('?') => (0, 1),
                Some('*') => (0, MAXREPEAT),
                Some('+') => (1, MAXREPEAT),
                _ => {
                    if source.next_is('}') {
                        subpattern.push(Node::Literal(this.c));
                        continue;
                    }
                    match parse_braces(source, here)? {
                        Some(mm) => mm,
                        None => {
                            subpattern.push(Node::Literal(this.c));
                            continue;
                        }
                    }
                }
            };
            let offset = source.tell() - here + 1;
            let item = match subpattern.last() {
                None | Some(Node::At(_)) => {
                    return Err(source.error(cps("nothing to repeat"), offset));
                }
                Some(Node::Repeat { .. }) => {
                    return Err(source.error(cps("multiple repeat"), offset));
                }
                Some(_) => match subpattern.pop() {
                    Some(Node::Subpattern {
                        group: None,
                        add_flags: 0,
                        del_flags: 0,
                        p,
                    }) => p,
                    Some(node) => vec![node],
                    None => Vec::new(),
                },
            };
            let kind = if source.matches('?')? {
                op::MIN_REPEAT
            } else if source.matches('+')? {
                op::POSSESSIVE_REPEAT
            } else {
                op::MAX_REPEAT
            };
            subpattern.push(Node::Repeat {
                kind,
                min,
                max,
                item,
            });
        } else if this.is('.') {
            subpattern.push(Node::Any);
        } else if this.is('(') {
            if let Some(node) =
                parse_group(source, state, &mut verbose, nested, first, &subpattern)?
            {
                subpattern.push(node);
            }
        } else if this.is('^') {
            subpattern.push(Node::At(at::BEGINNING));
        } else {
            // `$`
            subpattern.push(Node::At(at::END));
        }
    }

    // unpack non-capturing groups
    let mut i = subpattern.len();
    while i > 0 {
        i -= 1;
        if matches!(
            subpattern.get(i),
            Some(Node::Subpattern {
                group: None,
                add_flags: 0,
                del_flags: 0,
                ..
            })
        ) {
            if let Node::Subpattern { p, .. } = subpattern.remove(i) {
                subpattern.splice(i..i, p);
            }
        }
    }
    Ok(subpattern)
}

/// Parses after `(`; `Ok(None)` for items that add nothing (comments, global flags).
#[expect(
    clippy::too_many_lines,
    reason = "mirrors the group branch of _parser._parse"
)]
fn parse_group(
    source: &mut Source<'_>,
    state: &mut State,
    verbose: &mut bool,
    nested: usize,
    first: bool,
    subpattern: &[Node],
) -> Result<Option<Node>> {
    let start = source.tell() - 1;
    let mut capture = true;
    let mut atomic = false;
    let mut name: Option<PyStr> = None;
    let mut add_flags = 0;
    let mut del_flags = 0;
    let unterminated = |source: &Source<'_>| {
        source.error(
            cps("missing ), unterminated subpattern"),
            source.tell() - start,
        )
    };
    if source.matches('?')? {
        let Some(ch) = source.get()? else {
            return Err(source.error(cps("unexpected end of pattern"), 0));
        };
        if ch.is('P') {
            if source.matches('<')? {
                let n = source.getuntil('>', "group name")?;
                source.checkgroupname(&n, 1)?;
                name = Some(n);
            } else if source.matches('=')? {
                let n = source.getuntil(')', "group name")?;
                source.checkgroupname(&n, 1)?;
                let Some(&gid) = state.groupdict.get(&n) else {
                    return Err(source.error(msg_repr("unknown group name ", &n), n.len() + 1));
                };
                if !state.checkgroup(gid) {
                    return Err(source.error(cps("cannot refer to an open group"), n.len() + 1));
                }
                checklookbehindgroup(state, gid, source, nested + 1)?;
                return Ok(Some(Node::GroupRef(gid)));
            } else {
                let Some(ch) = source.get()? else {
                    return Err(source.error(cps("unexpected end of pattern"), 0));
                };
                return Err(
                    source.error(msg_with("unknown extension ?P", &ch.text()), ch.len() + 2)
                );
            }
        } else if ch.is(':') {
            capture = false;
        } else if ch.is('#') {
            loop {
                if source.next.is_none() {
                    return Err(source.error(
                        cps("missing ), unterminated comment"),
                        source.tell() - start,
                    ));
                }
                if source.get()?.is_some_and(|t| t.is(')')) {
                    break;
                }
            }
            return Ok(None);
        } else if ch.is_in("=!<") {
            let mut ch = ch;
            let mut dir = 1;
            let mut saved = None;
            if ch.is('<') {
                let Some(c2) = source.get()? else {
                    return Err(source.error(cps("unexpected end of pattern"), 0));
                };
                if !c2.is_in("=!") {
                    return Err(
                        source.error(msg_with("unknown extension ?<", &c2.text()), c2.len() + 2)
                    );
                }
                ch = c2;
                dir = -1;
                saved = Some(state.lookbehindgroups);
                if state.lookbehindgroups.is_none() {
                    state.lookbehindgroups = Some(state.groups());
                }
            }
            let p = source.within(nested + 1, |s| parse_sub(s, state, *verbose, nested + 1))?;
            if let Some(None) = saved {
                state.lookbehindgroups = None;
            }
            if !source.matches(')')? {
                return Err(unterminated(source));
            }
            return Ok(Some(Node::Assert {
                negate: ch.is('!'),
                dir,
                p,
            }));
        } else if ch.is('(') {
            let condname = source.getuntil(')', "group name")?;
            let condgroup;
            if !condname.iter().all(|&c| (0x30..=0x39).contains(&c)) {
                source.checkgroupname(&condname, 1)?;
                let Some(&g) = state.groupdict.get(&condname) else {
                    return Err(source.error(
                        msg_repr("unknown group name ", &condname),
                        condname.len() + 1,
                    ));
                };
                condgroup = g;
            } else {
                let value = int_digits(&condname)?;
                if value == Some(0) {
                    return Err(source.error(cps("bad group number"), condname.len() + 1));
                }
                let g = value
                    .and_then(|v| usize::try_from(v).ok())
                    .filter(|&v| v < MAXGROUPS);
                let Some(g) = g else {
                    return Err(source.error(
                        cps(&format!(
                            "invalid group reference {}",
                            digits_text(&condname)
                        )),
                        condname.len() + 1,
                    ));
                };
                condgroup = g;
                let pos = source.tell() - condname.len() - 1;
                state.grouprefpos.entry(condgroup).or_insert(pos);
            }
            checklookbehindgroup(state, condgroup, source, nested + 1)?;
            let yes =
                source.within(nested + 1, |s| parse(s, state, *verbose, nested + 1, false))?;
            let no = if source.matches('|')? {
                let no =
                    source.within(nested + 1, |s| parse(s, state, *verbose, nested + 1, false))?;
                if source.next_is('|') {
                    return Err(
                        source.error(cps("conditional backref with more than two branches"), 0)
                    );
                }
                Some(no)
            } else {
                None
            };
            if !source.matches(')')? {
                return Err(unterminated(source));
            }
            return Ok(Some(Node::GroupRefExists {
                group: condgroup,
                yes,
                no,
            }));
        } else if ch.is('>') {
            capture = false;
            atomic = true;
        } else if flag_of(ch).is_some() || ch.is('-') {
            match source.within(nested + 1, |s| parse_flags(s, state, ch))? {
                None => {
                    if !first || !subpattern.is_empty() {
                        return Err(source.error(
                            cps("global flags not at the start of the expression"),
                            source.tell() - start,
                        ));
                    }
                    *verbose = state.flags & flags::VERBOSE != 0;
                    return Ok(None);
                }
                Some((a, d)) => {
                    add_flags = a;
                    del_flags = d;
                    capture = false;
                }
            }
        } else {
            return Err(source.error(msg_with("unknown extension ?", &ch.text()), ch.len() + 1));
        }
    }

    let group = if capture {
        let gid = state.groups();
        state.groupwidths.push(None);
        if state.groups() > MAXGROUPS {
            return Err(source.error(
                cps("too many groups"),
                name.as_ref().map_or(0, Vec::len) + 1,
            ));
        }
        if let Some(n) = &name {
            if let Some(&ogid) = state.groupdict.get(n) {
                let mut m = msg_repr("redefinition of group name ", n);
                push_str(&mut m, &format!(" as group {gid}; was group {ogid}"));
                return Err(source.error(m, n.len() + 1));
            }
            state.groupdict.insert(n.clone(), gid);
        }
        Some(gid)
    } else {
        None
    };
    let sub_verbose =
        (*verbose || add_flags & flags::VERBOSE != 0) && del_flags & flags::VERBOSE == 0;
    let p = source.within(nested + 1, |s| parse_sub(s, state, sub_verbose, nested + 1))?;
    if !source.matches(')')? {
        return Err(unterminated(source));
    }
    if let Some(g) = group {
        // `state.closegroup(group, p)` calls `p.getwidth()` two frames down.
        frame(getwidth_depth(&p, nested + 2))?;
        let w = getwidth(&p, &state.groupwidths);
        if let Some(slot) = state.groupwidths.get_mut(g) {
            *slot = Some(w);
        }
    }
    if atomic {
        Ok(Some(Node::AtomicGroup(p)))
    } else {
        Ok(Some(Node::Subpattern {
            group,
            add_flags,
            del_flags,
            p,
        }))
    }
}

fn flag_error(source: &Source<'_>, t: Tok, otherwise: &str) -> Error {
    let alpha = t.esc.is_none() && alpha::is_alpha(t.c);
    source.error(cps(if alpha { "unknown flag" } else { otherwise }), t.len())
}

fn parse_flags(source: &mut Source<'_>, state: &mut State, ch: Tok) -> Result<Option<(u32, u32)>> {
    let mut add_flags = 0;
    let mut del_flags = 0;
    let mut ch = ch;
    if !ch.is('-') {
        loop {
            let flag = flag_of(ch).unwrap_or(0);
            if ch.is('L') {
                return Err(source.error(
                    cps("bad inline flags: cannot use 'L' flag with a str pattern"),
                    0,
                ));
            }
            add_flags |= flag;
            if flag & TYPE_FLAGS != 0 && add_flags & TYPE_FLAGS != flag {
                return Err(source.error(
                    cps("bad inline flags: flags 'a', 'u' and 'L' are incompatible"),
                    0,
                ));
            }
            let Some(next) = source.get()? else {
                return Err(source.error(cps("missing -, : or )"), 0));
            };
            ch = next;
            if ch.is_in(")-:") {
                break;
            }
            if flag_of(ch).is_none() {
                return Err(flag_error(source, ch, "missing -, : or )"));
            }
        }
    }
    if ch.is(')') {
        state.flags |= add_flags;
        return Ok(None);
    }
    if add_flags & GLOBAL_FLAGS != 0 {
        return Err(source.error(cps("bad inline flags: cannot turn on global flag"), 1));
    }
    if ch.is('-') {
        let Some(next) = source.get()? else {
            return Err(source.error(cps("missing flag"), 0));
        };
        ch = next;
        if flag_of(ch).is_none() {
            return Err(flag_error(source, ch, "missing flag"));
        }
        loop {
            let flag = flag_of(ch).unwrap_or(0);
            if flag & TYPE_FLAGS != 0 {
                return Err(source.error(
                    cps("bad inline flags: cannot turn off flags 'a', 'u' and 'L'"),
                    0,
                ));
            }
            del_flags |= flag;
            let Some(next) = source.get()? else {
                return Err(source.error(cps("missing :"), 0));
            };
            ch = next;
            if ch.is(':') {
                break;
            }
            if flag_of(ch).is_none() {
                return Err(flag_error(source, ch, "missing :"));
            }
        }
    }
    if del_flags & GLOBAL_FLAGS != 0 {
        return Err(source.error(cps("bad inline flags: cannot turn off global flag"), 1));
    }
    if add_flags & del_flags != 0 {
        return Err(source.error(cps("bad inline flags: flag turned on and off"), 1));
    }
    Ok(Some((add_flags, del_flags)))
}

/// `_parser.fix_flags` for a str pattern.
fn fix_flags(flags_value: u32) -> Result<u32> {
    if flags_value & flags::LOCALE != 0 {
        return Err(Error::Value(
            "cannot use LOCALE flag with a str pattern".into(),
        ));
    }
    if flags_value & flags::ASCII == 0 {
        Ok(flags_value | flags::UNICODE)
    } else if flags_value & flags::UNICODE != 0 {
        Err(Error::Value(
            "ASCII and UNICODE flags are incompatible".into(),
        ))
    } else {
        Ok(flags_value)
    }
}

/// A parsed pattern: the tree and the final parser state.
#[derive(Debug, Clone)]
pub struct Parsed {
    /// The top-level subpattern.
    pub data: SubPattern,
    /// Final state (flags fixed, groups, names).
    pub state: State,
}

/// `_parser.parse(pattern, flags)`, where `flags` holds the low bits of the
/// caller's flags.
///
/// # Errors
/// `re.error`, `ValueError` (flags), `OverflowError` (repeat counts) and
/// `RecursionError` (nesting), as CPython raises them.
pub fn parse_pattern(pattern: &[u32], flags_value: u32) -> Result<Parsed> {
    let mut source = Source::new(pattern)?;
    let mut state = State {
        flags: flags_value,
        groupwidths: vec![None],
        ..State::default()
    };
    let data = parse_sub(
        &mut source,
        &mut state,
        flags_value & flags::VERBOSE != 0,
        0,
    )?;
    state.flags = fix_flags(state.flags)?;
    if source.next.is_some() {
        return Err(source.error(cps("unbalanced parenthesis"), 0));
    }
    for (&g, &pos) in &state.grouprefpos {
        if g >= state.groups() {
            return Err(
                ReError::new(cps(&format!("invalid group reference {g}")), pattern, pos).into(),
            );
        }
    }
    Ok(Parsed { data, state })
}
