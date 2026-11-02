//! Port of CPython 3.12 `re._parser`: turns a pattern into an [`Item`] tree,
//! raising the same errors at the same positions.

use indexmap::IndexMap;

use crate::error::{Error, RegexError};
use crate::pyre::unicode15 as pystr;

pub(crate) const FLAG_TEMPLATE: u32 = 1;
pub(crate) const FLAG_IGNORECASE: u32 = 2;
pub(crate) const FLAG_LOCALE: u32 = 4;
pub(crate) const FLAG_MULTILINE: u32 = 8;
pub(crate) const FLAG_DOTALL: u32 = 16;
pub(crate) const FLAG_UNICODE: u32 = 32;
pub(crate) const FLAG_VERBOSE: u32 = 64;
pub(crate) const FLAG_DEBUG: u32 = 128;
pub(crate) const FLAG_ASCII: u32 = 256;

const TYPE_FLAGS: u32 = FLAG_ASCII | FLAG_LOCALE | FLAG_UNICODE;
const GLOBAL_FLAGS: u32 = FLAG_DEBUG | FLAG_TEMPLATE;

/// `_sre.MAXREPEAT`; a repeat bound equal to it means "unbounded".
pub(crate) const MAXREPEAT: u64 = 4_294_967_295;
const MAXGROUPS: u64 = 1_073_741_823;
/// `_compiler.MAXCODE` for 4-byte codes.
pub(crate) const MAXCODE: u128 = (1 << 32) - 1;
const MAXWIDTH: u128 = 1 << 64;

const SPECIAL_CHARS: &str = ".\\[{()*+?^$|";
const REPEAT_CHARS: &str = "*+?{";
const WHITESPACE: &str = " \t\n\r\x0b\x0c";

fn flag_value(c: char) -> Option<u32> {
    Some(match c {
        'i' => FLAG_IGNORECASE,
        'L' => FLAG_LOCALE,
        'm' => FLAG_MULTILINE,
        's' => FLAG_DOTALL,
        'x' => FLAG_VERBOSE,
        'a' => FLAG_ASCII,
        't' => FLAG_TEMPLATE,
        'u' => FLAG_UNICODE,
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Category {
    Digit,
    NotDigit,
    Space,
    NotSpace,
    Word,
    NotWord,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SetItem {
    Lit(u32),
    Range(u32, u32),
    Cat(Category),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AtCode {
    Beginning,
    BeginningString,
    End,
    EndString,
    Boundary,
    NonBoundary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RepKind {
    Greedy,
    Lazy,
    Possessive,
}

pub(crate) type Seq = Vec<Item>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Item {
    Literal(u32),
    NotLiteral(u32),
    Any,
    In {
        negate: bool,
        items: Vec<SetItem>,
    },
    At(AtCode),
    Branch(Vec<Seq>),
    Subpattern {
        group: Option<usize>,
        add: u32,
        del: u32,
        body: Seq,
    },
    Atomic(Seq),
    Repeat {
        min: u64,
        max: u64,
        kind: RepKind,
        body: Seq,
    },
    GroupRef(usize),
    GroupRefExists {
        group: usize,
        yes: Seq,
        no: Option<Seq>,
    },
    Assert {
        behind: bool,
        negate: bool,
        body: Seq,
    },
}

/// Result of a successful parse.
pub(crate) struct Parsed {
    pub seq: Seq,
    pub flags: u32,
    pub groups: usize,
    pub groupdict: IndexMap<String, usize>,
    pub groupwidths: Vec<(u128, u128)>,
}

/// One tokenizer token: a character, or a backslash followed by one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Tok {
    c: char,
    esc: bool,
}

impl Tok {
    fn len(self) -> usize {
        if self.esc {
            2
        } else {
            1
        }
    }
    fn is(self, c: char) -> bool {
        !self.esc && self.c == c
    }
    fn push_to(self, s: &mut String) {
        if self.esc {
            s.push('\\');
        }
        s.push(self.c);
    }
    fn text(self) -> String {
        let mut s = String::new();
        self.push_to(&mut s);
        s
    }
}

struct Source<'a> {
    chars: &'a [char],
    pattern: &'a str,
    index: usize,
    next: Option<Tok>,
}

impl<'a> Source<'a> {
    fn new(pattern: &'a str, chars: &'a [char]) -> Result<Self, RegexError> {
        let mut s = Source {
            chars,
            pattern,
            index: 0,
            next: None,
        };
        s.advance()?;
        Ok(s)
    }

    fn advance(&mut self) -> Result<(), RegexError> {
        let mut index = self.index;
        let Some(&c) = self.chars.get(index) else {
            self.next = None;
            return Ok(());
        };
        let tok = if c == '\\' {
            index += 1;
            match self.chars.get(index) {
                Some(&d) => Tok { c: d, esc: true },
                None => {
                    return Err(RegexError::new(
                        "bad escape (end of pattern)",
                        self.pattern,
                        Some(self.chars.len().saturating_sub(1)),
                    ))
                }
            }
        } else {
            Tok { c, esc: false }
        };
        self.index = index + 1;
        self.next = Some(tok);
        Ok(())
    }

    fn matches(&mut self, c: char) -> Result<bool, RegexError> {
        if self.next.is_some_and(|t| t.is(c)) {
            self.advance()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn get(&mut self) -> Result<Option<Tok>, RegexError> {
        let this = self.next;
        self.advance()?;
        Ok(this)
    }

    fn next_in(&self, set: impl Fn(char) -> bool) -> bool {
        self.next.is_some_and(|t| !t.esc && set(t.c))
    }

    fn getwhile(&mut self, n: usize, set: impl Fn(char) -> bool) -> Result<String, RegexError> {
        let mut out = String::new();
        for _ in 0..n {
            match self.next {
                Some(t) if !t.esc && set(t.c) => {
                    out.push(t.c);
                    self.advance()?;
                }
                _ => break,
            }
        }
        Ok(out)
    }

    fn getuntil(&mut self, terminator: char, name: &str) -> Result<String, RegexError> {
        let mut result = String::new();
        loop {
            let c = self.next;
            self.advance()?;
            match c {
                None => {
                    if result.is_empty() {
                        return Err(self.error(format!("missing {name}"), 0));
                    }
                    return Err(self.error(
                        format!("missing {terminator}, unterminated name"),
                        result.chars().count(),
                    ));
                }
                Some(t) if t.is(terminator) => {
                    if result.is_empty() {
                        return Err(self.error(format!("missing {name}"), 1));
                    }
                    break;
                }
                Some(t) => t.push_to(&mut result),
            }
        }
        Ok(result)
    }

    fn tell(&self) -> usize {
        self.index - self.next.map_or(0, Tok::len)
    }

    fn seek(&mut self, index: usize) -> Result<(), RegexError> {
        self.index = index;
        self.advance()
    }

    fn error(&self, msg: impl Into<String>, offset: usize) -> RegexError {
        RegexError::new(msg, self.pattern, Some(self.tell().saturating_sub(offset)))
    }

    fn checkgroupname(&self, name: &str, offset: usize) -> Result<(), RegexError> {
        if !pystr::is_identifier(name) {
            return Err(self.error(
                format!("bad character in group name {}", pystr::repr(name)),
                name.chars().count() + offset,
            ));
        }
        Ok(())
    }
}

enum PErr {
    Re(RegexError),
    /// sre's parser recursed past Python's recursion limit.
    Recursion,
    Overflow(String),
    Value(String),
}

impl From<RegexError> for PErr {
    fn from(e: RegexError) -> Self {
        PErr::Re(e)
    }
}

impl From<PErr> for Error {
    fn from(e: PErr) -> Self {
        match e {
            PErr::Re(r) => Error::Regex(r),
            PErr::Overflow(m) => Error::Overflow(m),
            PErr::Value(m) => Error::Value(m),
            PErr::Recursion => Error::Value(super::RECURSION_MSG.to_owned()),
        }
    }
}

type PResult<T> = Result<T, PErr>;

/// Deepest `nested` level `_parse_sub` may reach when nothing better is known:
/// what CPython 3.12 (recursion limit 1000) allows for `parse.parse` called
/// from a shallow Python stack. `nested` counts Python frames (`_parse_sub`
/// and `_parse` each add one), so callers that know the stack depth pass
/// their own limit to [`parse`].
pub const DEFAULT_MAX_NESTED: usize = 986;

struct State {
    flags: u32,
    groupdict: IndexMap<String, usize>,
    groupwidths: Vec<Option<(u128, u128)>>,
    lookbehindgroups: Option<usize>,
    grouprefpos: IndexMap<u64, usize>,
    /// Deepest `nested` allowed before `RecursionError`.
    max_nested: usize,
}

impl State {
    fn groups(&self) -> usize {
        self.groupwidths.len()
    }

    fn opengroup(&mut self, name: Option<&str>) -> Result<usize, String> {
        let gid = self.groups();
        self.groupwidths.push(None);
        if self.groups() as u64 > MAXGROUPS {
            return Err("too many groups".to_string());
        }
        if let Some(name) = name {
            if let Some(&ogid) = self.groupdict.get(name) {
                return Err(format!(
                    "redefinition of group name {} as group {gid}; was group {ogid}",
                    pystr::repr(name)
                ));
            }
            self.groupdict.insert(name.to_string(), gid);
        }
        Ok(gid)
    }

    fn checkgroup(&self, gid: usize) -> bool {
        gid < self.groups() && self.groupwidths.get(gid).is_some_and(Option::is_some)
    }

    fn checklookbehindgroup(&self, gid: usize, source: &Source<'_>) -> Result<(), RegexError> {
        if let Some(lb) = self.lookbehindgroups {
            if !self.checkgroup(gid) {
                return Err(source.error("cannot refer to an open group", 0));
            }
            if gid >= lb {
                return Err(source.error(
                    "cannot refer to group defined in the same lookbehind subpattern",
                    0,
                ));
            }
        }
        Ok(())
    }

    fn width(&self, seq: &[Item]) -> (u128, u128) {
        width_of(seq, &|g| {
            self.groupwidths.get(g).copied().flatten().unwrap_or((0, 0))
        })
    }
}

/// `SubPattern.getwidth()`.
pub(crate) fn width_of(seq: &[Item], groupwidth: &dyn Fn(usize) -> (u128, u128)) -> (u128, u128) {
    let mut lo: u128 = 0;
    let mut hi: u128 = 0;
    for item in seq {
        match item {
            Item::Branch(alts) => {
                let mut i = MAXWIDTH;
                let mut j = 0;
                for a in alts {
                    let (l, h) = width_of(a, groupwidth);
                    i = i.min(l);
                    j = j.max(h);
                }
                lo = lo.saturating_add(i);
                hi = hi.saturating_add(j);
            }
            Item::Atomic(body) | Item::Subpattern { body, .. } => {
                let (i, j) = width_of(body, groupwidth);
                lo = lo.saturating_add(i);
                hi = hi.saturating_add(j);
            }
            Item::Repeat { min, max, body, .. } => {
                let (i, j) = width_of(body, groupwidth);
                lo = lo.saturating_add(i.saturating_mul(u128::from(*min)));
                if *max == MAXREPEAT && j != 0 {
                    hi = MAXWIDTH;
                } else {
                    hi = hi.saturating_add(j.saturating_mul(u128::from(*max)));
                }
            }
            Item::Literal(_) | Item::NotLiteral(_) | Item::Any | Item::In { .. } => {
                lo = lo.saturating_add(1);
                hi = hi.saturating_add(1);
            }
            Item::GroupRef(g) => {
                let (i, j) = groupwidth(*g);
                lo = lo.saturating_add(i);
                hi = hi.saturating_add(j);
            }
            Item::GroupRefExists { yes, no, .. } => {
                let (mut i, mut j) = width_of(yes, groupwidth);
                match no {
                    Some(no) => {
                        let (l, h) = width_of(no, groupwidth);
                        i = i.min(l);
                        j = j.max(h);
                    }
                    None => i = 0,
                }
                lo = lo.saturating_add(i);
                hi = hi.saturating_add(j);
            }
            Item::At(_) | Item::Assert { .. } => {}
        }
    }
    (lo.min(MAXWIDTH), hi.min(MAXWIDTH))
}

fn category_escape(c: char) -> Option<Item> {
    let cat = |c| {
        Some(Item::In {
            negate: false,
            items: vec![SetItem::Cat(c)],
        })
    };
    match c {
        'A' => Some(Item::At(AtCode::BeginningString)),
        'b' => Some(Item::At(AtCode::Boundary)),
        'B' => Some(Item::At(AtCode::NonBoundary)),
        'd' => cat(Category::Digit),
        'D' => cat(Category::NotDigit),
        's' => cat(Category::Space),
        'S' => cat(Category::NotSpace),
        'w' => cat(Category::Word),
        'W' => cat(Category::NotWord),
        'Z' => Some(Item::At(AtCode::EndString)),
        _ => None,
    }
}

fn simple_escape(c: char) -> Option<u32> {
    Some(match c {
        'a' => 7,
        'b' => 8,
        'f' => 12,
        'n' => 10,
        'r' => 13,
        't' => 9,
        'v' => 11,
        '\\' => 92,
        _ => return None,
    })
}

fn is_hex(c: char) -> bool {
    c.is_ascii_hexdigit()
}
fn is_oct(c: char) -> bool {
    ('0'..='7').contains(&c)
}
fn is_digit(c: char) -> bool {
    c.is_ascii_digit()
}

/// Shared `\x`, `\u`, `\U`, `\N` handling. `Ok(None)` means "not one of these".
fn hex_escape(source: &mut Source<'_>, c: char) -> Result<Option<u32>, RegexError> {
    let n = match c {
        'x' => 2,
        'u' => 4,
        'U' => 8,
        'N' => {
            if !source.matches('{')? {
                return Err(source.error("missing {", 0));
            }
            let charname = source.getuntil('}', "character name")?;
            // No Unicode name database is available to the port.
            return Err(source.error(
                format!("undefined character name {}", pystr::repr(&charname)),
                charname.chars().count() + 4,
            ));
        }
        _ => return Ok(None),
    };
    let digits = source.getwhile(n, is_hex)?;
    let escape = format!("\\{c}{digits}");
    if digits.len() != n {
        return Err(source.error(
            format!("incomplete escape {escape}"),
            escape.chars().count(),
        ));
    }
    let v = u32::from_str_radix(&digits, 16).unwrap_or(u32::MAX);
    if v > 0x10FFFF {
        return Err(source.error(format!("bad escape {escape}"), escape.chars().count()));
    }
    Ok(Some(v))
}

/// Escape inside a character class: a literal code point or a category.
enum ClassCode {
    Lit(u32),
    Cat(Category),
}

fn class_escape(source: &mut Source<'_>, c: char) -> Result<ClassCode, RegexError> {
    if let Some(v) = simple_escape(c) {
        return Ok(ClassCode::Lit(v));
    }
    if let Some(Item::In { items, .. }) = category_escape(c) {
        if let Some(SetItem::Cat(cat)) = items.first() {
            return Ok(ClassCode::Cat(*cat));
        }
    }
    if let Some(v) = hex_escape(source, c)? {
        return Ok(ClassCode::Lit(v));
    }
    if is_oct(c) {
        let more = source.getwhile(2, is_oct)?;
        let escape = format!("\\{c}{more}");
        let v = u32::from_str_radix(&escape[1..], 8).unwrap_or(0);
        if v > 0o377 {
            return Err(source.error(
                format!("octal escape value {escape} outside of range 0-0o377"),
                escape.chars().count(),
            ));
        }
        return Ok(ClassCode::Lit(v));
    }
    if is_digit(c) || c.is_ascii_alphabetic() {
        return Err(source.error(format!("bad escape \\{c}"), 2));
    }
    Ok(ClassCode::Lit(c as u32))
}

fn escape(source: &mut Source<'_>, c: char, state: &State) -> Result<Item, RegexError> {
    if let Some(item) = category_escape(c) {
        return Ok(item);
    }
    if let Some(v) = simple_escape(c) {
        return Ok(Item::Literal(v));
    }
    if let Some(v) = hex_escape(source, c)? {
        return Ok(Item::Literal(v));
    }
    if c == '0' {
        let more = source.getwhile(2, is_oct)?;
        let v = u32::from_str_radix(&format!("0{more}"), 8).unwrap_or(0);
        return Ok(Item::Literal(v));
    }
    if is_digit(c) {
        let mut escape = format!("\\{c}");
        if source.next_in(is_digit) {
            if let Some(t) = source.get()? {
                escape.push(t.c);
            }
            let b: Vec<char> = escape.chars().collect();
            if b.len() == 3 && is_oct(b[1]) && is_oct(b[2]) && source.next_in(is_oct) {
                if let Some(t) = source.get()? {
                    escape.push(t.c);
                }
                let v = u32::from_str_radix(&escape[1..], 8).unwrap_or(0);
                if v > 0o377 {
                    return Err(source.error(
                        format!("octal escape value {escape} outside of range 0-0o377"),
                        escape.chars().count(),
                    ));
                }
                return Ok(Item::Literal(v));
            }
        }
        let group: usize = escape[1..].parse().unwrap_or(usize::MAX);
        let elen = escape.chars().count();
        if group < state.groups() {
            if !state.checkgroup(group) {
                return Err(source.error("cannot refer to an open group", elen));
            }
            state.checklookbehindgroup(group, source)?;
            return Ok(Item::GroupRef(group));
        }
        return Err(source.error(format!("invalid group reference {group}"), elen - 1));
    }
    if c.is_ascii_alphabetic() {
        return Err(source.error(format!("bad escape \\{c}"), 2));
    }
    Ok(Item::Literal(c as u32))
}

fn uniq(items: Vec<SetItem>) -> Vec<SetItem> {
    let mut out: Vec<SetItem> = Vec::with_capacity(items.len());
    for it in items {
        if !out.contains(&it) {
            out.push(it);
        }
    }
    out
}

fn parse_sub(
    source: &mut Source<'_>,
    state: &mut State,
    mut verbose: bool,
    nested: usize,
) -> PResult<Seq> {
    // `_parse_sub` and `_parse` each take a Python frame per level, so deep
    // nesting raises RecursionError upstream (from 493 nested groups).
    if nested > state.max_nested {
        return Err(PErr::Recursion);
    }
    let mut items: Vec<Seq> = Vec::new();
    loop {
        let first = nested == 0 && items.is_empty();
        items.push(parse_seq(source, state, verbose, nested + 1, first)?);
        if !source.matches('|')? {
            break;
        }
        if nested == 0 {
            verbose = state.flags & FLAG_VERBOSE != 0;
        }
    }
    if items.len() == 1 {
        return Ok(items.pop().unwrap_or_default());
    }

    let mut subpattern: Seq = Vec::new();
    // check if all items share a common prefix
    loop {
        let mut prefix: Option<&Item> = None;
        let mut common = true;
        for item in &items {
            let Some(first) = item.first() else {
                common = false;
                break;
            };
            match prefix {
                None => prefix = Some(first),
                Some(p) if !py_tuple_eq(p, first) => {
                    common = false;
                    break;
                }
                Some(_) => {}
            }
        }
        if !common {
            break;
        }
        let mut moved = None;
        for item in &mut items {
            moved = Some(item.remove(0));
        }
        if let Some(m) = moved {
            subpattern.push(m);
        }
    }

    // check if the branch can be replaced by a character set
    let mut set: Vec<SetItem> = Vec::new();
    let mut all_set = true;
    for item in &items {
        match item.as_slice() {
            [Item::Literal(c)] => set.push(SetItem::Lit(*c)),
            [Item::In {
                negate: false,
                items,
            }] => set.extend_from_slice(items),
            _ => {
                all_set = false;
                break;
            }
        }
    }
    if all_set {
        subpattern.push(Item::In {
            negate: false,
            items: uniq(set),
        });
        return Ok(subpattern);
    }
    subpattern.push(Item::Branch(items));
    Ok(subpattern)
}

/// Python `==` on sre `(op, av)` tuples: items holding a `SubPattern`
/// compare by identity, so they are never equal across alternatives.
fn py_tuple_eq(a: &Item, b: &Item) -> bool {
    match (a, b) {
        (Item::Literal(_), _)
        | (Item::NotLiteral(_), _)
        | (Item::Any, _)
        | (Item::In { .. }, _)
        | (Item::At(_), _)
        | (Item::GroupRef(_), _) => a == b,
        _ => false,
    }
}

fn parse_digits(s: &str) -> u64 {
    // Saturates far above MAXREPEAT; only compared against it.
    s.chars().fold(0u64, |acc, c| {
        acc.saturating_mul(10)
            .saturating_add(u64::from(c.to_digit(10).unwrap_or(0)))
    })
}

fn parse_seq(
    source: &mut Source<'_>,
    state: &mut State,
    mut verbose: bool,
    nested: usize,
    first: bool,
) -> PResult<Seq> {
    let mut sub: Seq = Vec::new();
    while let Some(this) = source.next {
        if this.is('|') || this.is(')') {
            break;
        }
        source.advance()?;

        if verbose {
            if !this.esc && WHITESPACE.contains(this.c) {
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

        if this.esc {
            sub.push(escape(source, this.c, state)?);
            continue;
        }
        let c = this.c;
        if !SPECIAL_CHARS.contains(c) {
            sub.push(Item::Literal(c as u32));
        } else if c == '[' {
            sub.push(parse_class(source)?);
        } else if REPEAT_CHARS.contains(c) {
            let here = source.tell();
            let (min, max) = match c {
                '?' => (0, 1),
                '*' => (0, MAXREPEAT),
                '+' => (1, MAXREPEAT),
                _ => {
                    if source.next.is_some_and(|t| t.is('}')) {
                        sub.push(Item::Literal('{' as u32));
                        continue;
                    }
                    let mut min = 0;
                    let mut max = MAXREPEAT;
                    let mut lo = String::new();
                    let mut hi = String::new();
                    while source.next_in(is_digit) {
                        if let Some(t) = source.get()? {
                            lo.push(t.c);
                        }
                    }
                    if source.matches(',')? {
                        while source.next_in(is_digit) {
                            if let Some(t) = source.get()? {
                                hi.push(t.c);
                            }
                        }
                    } else {
                        hi = lo.clone();
                    }
                    if !source.matches('}')? {
                        sub.push(Item::Literal('{' as u32));
                        source.seek(here)?;
                        continue;
                    }
                    if !lo.is_empty() {
                        min = parse_digits(&lo);
                        if min >= MAXREPEAT {
                            return Err(PErr::Overflow(
                                "the repetition number is too large".to_string(),
                            ));
                        }
                    }
                    if !hi.is_empty() {
                        max = parse_digits(&hi);
                        if max >= MAXREPEAT {
                            return Err(PErr::Overflow(
                                "the repetition number is too large".to_string(),
                            ));
                        }
                        if max < min {
                            return Err(source
                                .error("min repeat greater than max repeat", source.tell() - here)
                                .into());
                        }
                    }
                    (min, max)
                }
            };
            let offset = source.tell() - here + 1;
            let body = match sub.last() {
                None | Some(Item::At(_)) => {
                    return Err(source.error("nothing to repeat", offset).into());
                }
                Some(Item::Repeat { .. }) => {
                    return Err(source.error("multiple repeat", offset).into());
                }
                Some(Item::Subpattern {
                    group: None,
                    add: 0,
                    del: 0,
                    body,
                }) => body.clone(),
                Some(other) => vec![other.clone()],
            };
            let kind = if source.matches('?')? {
                RepKind::Lazy
            } else if source.matches('+')? {
                RepKind::Possessive
            } else {
                RepKind::Greedy
            };
            sub.pop();
            sub.push(Item::Repeat {
                min,
                max,
                kind,
                body,
            });
        } else if c == '.' {
            sub.push(Item::Any);
        } else if c == '(' {
            if let Some(item) = parse_group(source, state, &mut verbose, nested, first, &sub)? {
                sub.push(item);
            }
        } else if c == '^' {
            sub.push(Item::At(AtCode::Beginning));
        } else if c == '$' {
            sub.push(Item::At(AtCode::End));
        }
    }

    // unpack non-capturing groups
    let mut out = Vec::with_capacity(sub.len());
    for item in sub {
        match item {
            Item::Subpattern {
                group: None,
                add: 0,
                del: 0,
                body,
            } => out.extend(body),
            other => out.push(other),
        }
    }
    Ok(out)
}

fn parse_class(source: &mut Source<'_>) -> PResult<Item> {
    let here = source.tell() - 1;
    let mut set: Vec<SetItem> = Vec::new();
    let negate = source.matches('^')?;
    let to_item = |code: ClassCode| match code {
        ClassCode::Lit(v) => SetItem::Lit(v),
        ClassCode::Cat(c) => SetItem::Cat(c),
    };
    loop {
        let Some(this) = source.get()? else {
            return Err(source
                .error("unterminated character set", source.tell() - here)
                .into());
        };
        if this.is(']') && !set.is_empty() {
            break;
        }
        let code1 = if this.esc {
            class_escape(source, this.c)?
        } else {
            ClassCode::Lit(this.c as u32)
        };
        if source.matches('-')? {
            let Some(that) = source.get()? else {
                return Err(source
                    .error("unterminated character set", source.tell() - here)
                    .into());
            };
            if that.is(']') {
                set.push(to_item(code1));
                set.push(SetItem::Lit('-' as u32));
                break;
            }
            let code2 = if that.esc {
                class_escape(source, that.c)?
            } else {
                ClassCode::Lit(that.c as u32)
            };
            let msg = || format!("bad character range {}-{}", this.text(), that.text());
            let off = this.len() + 1 + that.len();
            match (code1, code2) {
                (ClassCode::Lit(lo), ClassCode::Lit(hi)) => {
                    if hi < lo {
                        return Err(source.error(msg(), off).into());
                    }
                    set.push(SetItem::Range(lo, hi));
                }
                _ => return Err(source.error(msg(), off).into()),
            }
        } else {
            set.push(to_item(code1));
        }
    }
    let set = uniq(set);
    if let [SetItem::Lit(v)] = set.as_slice() {
        return Ok(if negate {
            Item::NotLiteral(*v)
        } else {
            Item::Literal(*v)
        });
    }
    Ok(Item::In { negate, items: set })
}

fn is_alpha_tok(t: Tok) -> bool {
    !t.esc && pystr::is_alpha(t.c)
}

fn parse_flags(
    source: &mut Source<'_>,
    state: &mut State,
    first: Tok,
) -> PResult<Option<(u32, u32)>> {
    let mut add = 0u32;
    let mut del = 0u32;
    let mut ch = first;
    if !ch.is('-') {
        loop {
            let flag = flag_value(ch.c).unwrap_or(0);
            if ch.c == 'L' {
                return Err(source
                    .error(
                        "bad inline flags: cannot use 'L' flag with a str pattern",
                        0,
                    )
                    .into());
            }
            add |= flag;
            if flag & TYPE_FLAGS != 0 && (add & TYPE_FLAGS) != flag {
                return Err(source
                    .error(
                        "bad inline flags: flags 'a', 'u' and 'L' are incompatible",
                        0,
                    )
                    .into());
            }
            let Some(next) = source.get()? else {
                return Err(source.error("missing -, : or )", 0).into());
            };
            ch = next;
            if ch.is(')') || ch.is('-') || ch.is(':') {
                break;
            }
            if ch.esc || flag_value(ch.c).is_none() {
                let msg = if is_alpha_tok(ch) {
                    "unknown flag"
                } else {
                    "missing -, : or )"
                };
                return Err(source.error(msg, ch.len()).into());
            }
        }
    }
    if ch.is(')') {
        state.flags |= add;
        return Ok(None);
    }
    if add & GLOBAL_FLAGS != 0 {
        return Err(source
            .error("bad inline flags: cannot turn on global flag", 1)
            .into());
    }
    if ch.is('-') {
        let Some(next) = source.get()? else {
            return Err(source.error("missing flag", 0).into());
        };
        ch = next;
        if ch.esc || flag_value(ch.c).is_none() {
            let msg = if is_alpha_tok(ch) {
                "unknown flag"
            } else {
                "missing flag"
            };
            return Err(source.error(msg, ch.len()).into());
        }
        loop {
            let flag = flag_value(ch.c).unwrap_or(0);
            if flag & TYPE_FLAGS != 0 {
                return Err(source
                    .error(
                        "bad inline flags: cannot turn off flags 'a', 'u' and 'L'",
                        0,
                    )
                    .into());
            }
            del |= flag;
            let Some(next) = source.get()? else {
                return Err(source.error("missing :", 0).into());
            };
            ch = next;
            if ch.is(':') {
                break;
            }
            if ch.esc || flag_value(ch.c).is_none() {
                let msg = if is_alpha_tok(ch) {
                    "unknown flag"
                } else {
                    "missing :"
                };
                return Err(source.error(msg, ch.len()).into());
            }
        }
    }
    if del & GLOBAL_FLAGS != 0 {
        return Err(source
            .error("bad inline flags: cannot turn off global flag", 1)
            .into());
    }
    if add & del != 0 {
        return Err(source
            .error("bad inline flags: flag turned on and off", 1)
            .into());
    }
    Ok(Some((add, del)))
}

fn parse_group(
    source: &mut Source<'_>,
    state: &mut State,
    verbose: &mut bool,
    nested: usize,
    first: bool,
    sub: &[Item],
) -> PResult<Option<Item>> {
    let start = source.tell() - 1;
    let mut capture = true;
    let mut atomic = false;
    let mut name: Option<String> = None;
    let mut add_flags = 0;
    let mut del_flags = 0;
    let unterminated = |s: &Source<'_>| -> PErr {
        s.error("missing ), unterminated subpattern", s.tell() - start)
            .into()
    };
    if source.matches('?')? {
        let Some(ch) = source.get()? else {
            return Err(source.error("unexpected end of pattern", 0).into());
        };
        if ch.is('P') {
            if source.matches('<')? {
                let n = source.getuntil('>', "group name")?;
                source.checkgroupname(&n, 1)?;
                name = Some(n);
            } else if source.matches('=')? {
                let n = source.getuntil(')', "group name")?;
                source.checkgroupname(&n, 1)?;
                let nlen = n.chars().count();
                let Some(&gid) = state.groupdict.get(&n) else {
                    return Err(source
                        .error(format!("unknown group name {}", pystr::repr(&n)), nlen + 1)
                        .into());
                };
                if !state.checkgroup(gid) {
                    return Err(source
                        .error("cannot refer to an open group", nlen + 1)
                        .into());
                }
                state.checklookbehindgroup(gid, source)?;
                return Ok(Some(Item::GroupRef(gid)));
            } else {
                let Some(c2) = source.get()? else {
                    return Err(source.error("unexpected end of pattern", 0).into());
                };
                return Err(source
                    .error(format!("unknown extension ?P{}", c2.text()), c2.len() + 2)
                    .into());
            }
        } else if ch.is(':') {
            capture = false;
        } else if ch.is('#') {
            loop {
                if source.next.is_none() {
                    return Err(source
                        .error("missing ), unterminated comment", source.tell() - start)
                        .into());
                }
                if source.get()?.is_some_and(|t| t.is(')')) {
                    break;
                }
            }
            return Ok(None);
        } else if ch.is('=') || ch.is('!') || ch.is('<') {
            let mut ch = ch;
            let mut behind = false;
            let mut saved_lb = None;
            if ch.is('<') {
                let Some(c2) = source.get()? else {
                    return Err(source.error("unexpected end of pattern", 0).into());
                };
                if !(c2.is('=') || c2.is('!')) {
                    return Err(source
                        .error(format!("unknown extension ?<{}", c2.text()), c2.len() + 2)
                        .into());
                }
                ch = c2;
                behind = true;
                saved_lb = Some(state.lookbehindgroups);
                if state.lookbehindgroups.is_none() {
                    state.lookbehindgroups = Some(state.groups());
                }
            }
            let p = parse_sub(source, state, *verbose, nested + 1)?;
            if behind && saved_lb == Some(None) {
                state.lookbehindgroups = None;
            }
            if !source.matches(')')? {
                return Err(unterminated(source));
            }
            return Ok(Some(Item::Assert {
                behind,
                negate: ch.is('!'),
                body: p,
            }));
        } else if ch.is('(') {
            let condname = source.getuntil(')', "group name")?;
            let clen = condname.chars().count();
            let condgroup: usize = if !(condname.chars().all(|c| c.is_ascii_digit())) {
                source.checkgroupname(&condname, 1)?;
                match state.groupdict.get(&condname) {
                    Some(&g) => g,
                    None => {
                        return Err(source
                            .error(
                                format!("unknown group name {}", pystr::repr(&condname)),
                                clen + 1,
                            )
                            .into())
                    }
                }
            } else {
                let g = parse_digits(&condname);
                if g == 0 {
                    return Err(source.error("bad group number", clen + 1).into());
                }
                if g >= MAXGROUPS {
                    return Err(source
                        .error(format!("invalid group reference {g}"), clen + 1)
                        .into());
                }
                let pos = source.tell() - clen - 1;
                state.grouprefpos.entry(g).or_insert(pos);
                usize::try_from(g).unwrap_or(usize::MAX)
            };
            state.checklookbehindgroup(condgroup, source)?;
            let yes = parse_seq(source, state, *verbose, nested + 1, false)?;
            let no = if source.matches('|')? {
                let no = parse_seq(source, state, *verbose, nested + 1, false)?;
                if source.next.is_some_and(|t| t.is('|')) {
                    return Err(source
                        .error("conditional backref with more than two branches", 0)
                        .into());
                }
                Some(no)
            } else {
                None
            };
            if !source.matches(')')? {
                return Err(unterminated(source));
            }
            return Ok(Some(Item::GroupRefExists {
                group: condgroup,
                yes,
                no,
            }));
        } else if ch.is('>') {
            capture = false;
            atomic = true;
        } else if !ch.esc && (flag_value(ch.c).is_some() || ch.c == '-') {
            match parse_flags(source, state, ch)? {
                None => {
                    if !first || !sub.is_empty() {
                        return Err(source
                            .error(
                                "global flags not at the start of the expression",
                                source.tell() - start,
                            )
                            .into());
                    }
                    *verbose = state.flags & FLAG_VERBOSE != 0;
                    return Ok(None);
                }
                Some((a, d)) => {
                    add_flags = a;
                    del_flags = d;
                    capture = false;
                }
            }
        } else {
            return Err(source
                .error(format!("unknown extension ?{}", ch.text()), ch.len() + 1)
                .into());
        }
    }

    let group = if capture {
        match state.opengroup(name.as_deref()) {
            Ok(g) => Some(g),
            Err(msg) => {
                let off = name.as_deref().map_or(0, |n| n.chars().count()) + 1;
                return Err(source.error(msg, off).into());
            }
        }
    } else {
        None
    };
    let sub_verbose = (*verbose || add_flags & FLAG_VERBOSE != 0) && del_flags & FLAG_VERBOSE == 0;
    let p = parse_sub(source, state, sub_verbose, nested + 1)?;
    if !source.matches(')')? {
        return Err(unterminated(source));
    }
    if let Some(g) = group {
        let w = state.width(&p);
        if let Some(slot) = state.groupwidths.get_mut(g) {
            *slot = Some(w);
        }
    }
    if atomic {
        return Ok(Some(Item::Atomic(p)));
    }
    Ok(Some(Item::Subpattern {
        group,
        add: add_flags,
        del: del_flags,
        body: p,
    }))
}

/// `_parser.parse(pattern, flags)` followed by `fix_flags`; nesting deeper
/// than `max_nested` is Python's `RecursionError`.
pub(crate) fn parse(pattern: &str, flags: u32, max_nested: usize) -> Result<Parsed, Error> {
    parse_inner(pattern, flags, max_nested).map_err(Error::from)
}

fn parse_inner(pattern: &str, flags: u32, max_nested: usize) -> PResult<Parsed> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut source = Source::new(pattern, &chars)?;
    let mut state = State {
        flags,
        groupdict: IndexMap::new(),
        groupwidths: vec![None],
        lookbehindgroups: None,
        grouprefpos: IndexMap::new(),
        max_nested,
    };
    let seq = parse_sub(&mut source, &mut state, flags & FLAG_VERBOSE != 0, 0)?;

    // fix_flags for a str pattern
    let mut f = state.flags;
    if f & FLAG_LOCALE != 0 {
        return Err(PErr::Value(
            "cannot use LOCALE flag with a str pattern".to_string(),
        ));
    }
    if f & FLAG_ASCII == 0 {
        f |= FLAG_UNICODE;
    } else if f & FLAG_UNICODE != 0 {
        return Err(PErr::Value(
            "ASCII and UNICODE flags are incompatible".to_string(),
        ));
    }
    state.flags = f;

    if source.next.is_some() {
        return Err(source.error("unbalanced parenthesis", 0).into());
    }
    for (&g, &pos) in &state.grouprefpos {
        if g >= state.groups() as u64 {
            return Err(RegexError::new(
                format!("invalid group reference {g}"),
                pattern,
                Some(pos),
            )
            .into());
        }
    }
    let groupwidths = state
        .groupwidths
        .iter()
        .map(|w| w.unwrap_or((0, 0)))
        .collect();
    Ok(Parsed {
        seq,
        flags: state.flags,
        groups: state.groups() - 1,
        groupdict: state.groupdict,
        groupwidths,
    })
}
