//! The sre matcher: an iterative interpreter over the compiled code words with
//! `sre_lib.h` semantics (marks, `lastmark`/`lastindex`, repeat contexts,
//! the search optimizations driven by the INFO block).
//!
//! Recursion in the C engine (`DO_JUMP`) becomes an explicit heap stack of
//! [`Ctx`] frames, so deep backtracking never grows the Rust stack.

use crate::error::{Error, Result};
use crate::sre::{at, ch, op, INFO_CHARSET, INFO_LITERAL, INFO_PREFIX, MAXREPEAT};
use crate::unicode;

/// Where a child frame returns to in its parent (`JUMP_*` in sre).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Jump {
    None,
    MaxUntil1,
    MaxUntil2,
    MaxUntil3,
    MinUntil1,
    MinUntil2,
    MinUntil3,
    Repeat,
    RepeatOne1,
    RepeatOne2,
    MinRepeatOne,
    Branch,
    Assert,
    AssertNot,
    PossRepeat1,
    PossRepeat2,
    AtomicGroup,
}

/// One `SRE(match_context)`.
#[derive(Debug, Clone)]
struct Ctx {
    /// Saved `pattern` (index of the word after the current opcode).
    pc: usize,
    /// Saved `ptr`.
    ptr: usize,
    toplevel: bool,
    jump: Jump,
    count: isize,
    lastmark: isize,
    lastindex: isize,
    /// `u.rep`: index into the repeat arena.
    rep: usize,
    /// `u.chr`.
    chr: u32,
    /// The repeat's `last_ptr` saved by `DATA_PUSH`.
    saved_last_ptr: Option<usize>,
    /// The possessive repeat's position before its last iteration.
    poss_ptr: Option<usize>,
}

impl Ctx {
    fn new(pc: usize, toplevel: bool, jump: Jump) -> Self {
        Self {
            pc,
            ptr: 0,
            toplevel,
            jump,
            count: 0,
            lastmark: -1,
            lastindex: -1,
            rep: 0,
            chr: 0,
            saved_last_ptr: None,
            poss_ptr: None,
        }
    }
}

/// `SRE_REPEAT`.
#[derive(Debug, Clone, Copy)]
struct Repeat {
    count: isize,
    /// Index of the REPEAT's skip word (`pattern[1]` is min, `[2]` max,
    /// `+3` the item).
    pattern: usize,
    prev: Option<usize>,
    last_ptr: Option<usize>,
}

enum Step {
    Ret(bool),
    Call {
        jump: Jump,
        pc: usize,
        toplevel: bool,
    },
}

enum Flow {
    Step(Step),
    Continue(usize, usize),
}

fn w(code: &[u32], i: usize) -> u32 {
    code.get(i).copied().unwrap_or(op::FAILURE)
}

fn wu(code: &[u32], i: usize) -> usize {
    usize::try_from(w(code, i)).unwrap_or(usize::MAX)
}

fn wi(code: &[u32], i: usize) -> isize {
    isize::try_from(w(code, i)).unwrap_or(isize::MAX)
}

fn signed(x: usize) -> isize {
    isize::try_from(x).unwrap_or(isize::MAX)
}

const LF: u32 = 0x0A;

fn is_linebreak(c: u32) -> bool {
    c == LF
}

fn is_uni_linebreak(c: u32) -> bool {
    matches!(
        c,
        0x0A..=0x0D | 0x1C..=0x1E | 0x85 | 0x2028 | 0x2029
    )
}

fn is_loc_word(c: u32) -> bool {
    unicode::is_ascii_word(c)
}

// ASCII fast paths for the Unicode tables (which binary-search). For code
// points below 128 they agree with `crate::unicode` (see the tests below).

fn uni_word(c: u32) -> bool {
    if c < 128 {
        unicode::is_ascii_word(c)
    } else {
        unicode::is_word(c)
    }
}

fn uni_digit(c: u32) -> bool {
    if c < 128 {
        unicode::is_ascii_digit(c)
    } else {
        unicode::is_decimal(c)
    }
}

fn uni_space(c: u32) -> bool {
    if c < 128 {
        matches!(c, 0x09..=0x0D | 0x1C..=0x20)
    } else {
        unicode::is_space(c)
    }
}

fn uni_lower(c: u32) -> u32 {
    if c < 128 {
        unicode::ascii_lower(c)
    } else if let Some(&lower) = lower_table().get(c as usize) {
        lower
    } else {
        unicode::to_lower(c)
    }
}

/// Code points below this have their `unicode::to_lower` in [`lower_table`].
const LOWER_TABLE_LEN: u32 = 0x800;

/// `unicode::to_lower(c)` for every `c < LOWER_TABLE_LEN` (Latin, Greek,
/// Cyrillic, ...), built from it on first use; it binary-searches.
fn lower_table() -> &'static [u32] {
    static TABLE: std::sync::OnceLock<Vec<u32>> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| (0..LOWER_TABLE_LEN).map(unicode::to_lower).collect())
}

fn uni_upper(c: u32) -> u32 {
    if c < 128 {
        if (0x61..=0x7A).contains(&c) {
            c - 32
        } else {
            c
        }
    } else {
        unicode::to_upper(c)
    }
}

/// `sre_category`.
fn category(code: u32, c: u32) -> bool {
    match code {
        ch::DIGIT => unicode::is_ascii_digit(c),
        ch::NOT_DIGIT => !unicode::is_ascii_digit(c),
        ch::SPACE => unicode::is_ascii_space(c),
        ch::NOT_SPACE => !unicode::is_ascii_space(c),
        ch::WORD => unicode::is_ascii_word(c),
        ch::NOT_WORD => !unicode::is_ascii_word(c),
        ch::LINEBREAK => is_linebreak(c),
        ch::NOT_LINEBREAK => !is_linebreak(c),
        ch::LOC_WORD => is_loc_word(c),
        ch::LOC_NOT_WORD => !is_loc_word(c),
        ch::UNI_DIGIT => uni_digit(c),
        ch::UNI_NOT_DIGIT => !uni_digit(c),
        ch::UNI_SPACE => uni_space(c),
        ch::UNI_NOT_SPACE => !uni_space(c),
        ch::UNI_WORD => uni_word(c),
        ch::UNI_NOT_WORD => !uni_word(c),
        ch::UNI_LINEBREAK => is_uni_linebreak(c),
        ch::UNI_NOT_LINEBREAK => !is_uni_linebreak(c),
        _ => false,
    }
}

/// `sre_lower_ascii`.
fn lower_ascii(c: u32) -> u32 {
    unicode::ascii_lower(c)
}

/// `sre_lower_locale` in the C locale.
fn lower_locale(c: u32) -> u32 {
    if c < 256 {
        unicode::ascii_lower(c)
    } else {
        c
    }
}

/// `sre_upper_locale` in the C locale.
fn upper_locale(c: u32) -> u32 {
    if (0x61..=0x7A).contains(&c) {
        c - 32
    } else {
        c
    }
}

/// `char_loc_ignore`.
fn char_loc_ignore(pattern: u32, c: u32) -> bool {
    c == pattern || lower_locale(c) == pattern || upper_locale(c) == pattern
}

/// `SRE(charset)`: whether `c` is in the set starting at `set`.
fn charset(code: &[u32], mut set: usize, c: u32) -> bool {
    let mut ok = true;
    loop {
        let opcode = w(code, set);
        set += 1;
        match opcode {
            op::FAILURE => return !ok,
            op::LITERAL => {
                if c == w(code, set) {
                    return ok;
                }
                set += 1;
            }
            op::CATEGORY => {
                if category(w(code, set), c) {
                    return ok;
                }
                set += 1;
            }
            op::CHARSET => {
                if c < 256 && (w(code, set + (c / 32) as usize) & (1u32 << (c & 31))) != 0 {
                    return ok;
                }
                set += 256 / 32;
            }
            op::RANGE => {
                if w(code, set) <= c && c <= w(code, set + 1) {
                    return ok;
                }
                set += 2;
            }
            op::RANGE_UNI_IGNORE => {
                if w(code, set) <= c && c <= w(code, set + 1) {
                    return ok;
                }
                let u = uni_upper(c);
                if w(code, set) <= u && u <= w(code, set + 1) {
                    return ok;
                }
                set += 2;
            }
            op::NEGATE => ok = !ok,
            op::BIGCHARSET => {
                let count = wu(code, set);
                set += 1;
                let block = if c < 0x10000 {
                    let byte_index = (c >> 8) as usize;
                    let word = w(code, set + byte_index / 4);
                    Some(((word >> (8 * (byte_index % 4))) & 0xFF) as usize)
                } else {
                    None
                };
                set += 256 / 4;
                if let Some(block) = block {
                    let bit = block * 256 + (c & 255) as usize;
                    if (w(code, set + bit / 32) & (1u32 << (bit % 32))) != 0 {
                        return ok;
                    }
                }
                set = set.saturating_add(count.saturating_mul(256 / 32));
            }
            _ => return false,
        }
    }
}

/// `SRE(charset_loc_ignore)`.
fn charset_loc_ignore(code: &[u32], set: usize, c: u32) -> bool {
    let lo = lower_locale(c);
    if charset(code, set, c) {
        return true;
    }
    let up = upper_locale(lo);
    up != lo && charset(code, set, up)
}

/// Where a match must start, for skipping hopeless positions in a search.
///
/// Returns the opcode indices of single-character items such that every match
/// of `code` (after its INFO block) consumes, as its first character, one
/// accepted by at least one of them ([`State::unit`]). `None` when that can't
/// be shown (the pattern may match empty, or uses constructs not analysed).
/// Skipping a position this rules out never changes a result: the matcher
/// would fail there anyway.
pub(crate) fn first_units(code: &[u32]) -> Option<FirstSet> {
    let pc = if w(code, 0) == op::INFO {
        1 + wu(code, 1)
    } else {
        0
    };
    let mut units = Vec::new();
    if !first_units_from(code, pc, &mut units, 0) {
        return None;
    }
    let mut ascii = 0u128;
    for c in 0..128u32 {
        if units.iter().any(|&u| State::unit(code, u, c)) {
            ascii |= 1 << c;
        }
    }
    Some(FirstSet { units, ascii })
}

/// Every charset of a program (the sets of IN-family opcodes and the INFO
/// charset), each with its [`charset`] answers for the ASCII characters
/// precomputed. A lookup misses for positions not found by the walk, and the
/// matcher then runs [`charset`] itself, so coverage only affects speed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AsciiSets {
    /// For each code index, `1 +` the index in `maps` of the charset that
    /// starts there, or 0.
    index: Vec<u32>,
    /// Bit `c` is `charset(code, set, c)` for `c < 128`.
    maps: Vec<u128>,
}

impl AsciiSets {
    /// Deepest nesting the walk follows; deeper sets are just not indexed.
    const MAX_DEPTH: usize = 64;

    pub(crate) fn new(code: &[u32]) -> Self {
        let mut starts = Vec::new();
        Self::walk(code, 0, code.len(), &mut starts, 0);
        let mut sets = Self::default();
        if starts.is_empty() {
            return sets;
        }
        sets.index = vec![0; code.len()];
        for set in starts {
            let Some(slot) = sets.index.get_mut(set) else {
                continue;
            };
            if *slot != 0 {
                continue;
            }
            let mut map = 0u128;
            for c in 0..128u32 {
                if charset(code, set, c) {
                    map |= 1 << c;
                }
            }
            sets.maps.push(map);
            *slot = u32::try_from(sets.maps.len()).unwrap_or(0);
        }
        sets
    }

    /// Collects the charset starts in `code[start..end]`, following the
    /// layout `_compiler.dis` walks.
    fn walk(code: &[u32], start: usize, end: usize, out: &mut Vec<usize>, depth: usize) {
        if depth > Self::MAX_DEPTH {
            return;
        }
        let word = |i: usize| wu(code, i);
        let mut i = start;
        while i < end && i < code.len() {
            let o = w(code, i);
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
                op::IN | op::IN_IGNORE | op::IN_UNI_IGNORE | op::IN_LOC_IGNORE => {
                    out.push(i + 1);
                    i = i.saturating_add(word(i));
                }
                op::BRANCH => {
                    let mut skip = word(i);
                    while skip != 0 && i < end {
                        Self::walk(code, i + 1, i.saturating_add(skip), out, depth + 1);
                        i = i.saturating_add(skip);
                        skip = word(i);
                    }
                    i += 1;
                }
                op::REPEAT
                | op::REPEAT_ONE
                | op::MIN_REPEAT_ONE
                | op::POSSESSIVE_REPEAT
                | op::POSSESSIVE_REPEAT_ONE => {
                    let to = i.saturating_add(word(i));
                    Self::walk(code, i + 3, to, out, depth + 1);
                    i = to;
                }
                op::ASSERT | op::ASSERT_NOT => {
                    let to = i.saturating_add(word(i));
                    Self::walk(code, i + 2, to, out, depth + 1);
                    i = to;
                }
                op::ATOMIC_GROUP => {
                    let to = i.saturating_add(word(i));
                    Self::walk(code, i + 1, to, out, depth + 1);
                    i = to;
                }
                op::INFO => {
                    let to = i.saturating_add(word(i));
                    let flags = w(code, i + 1);
                    if flags & INFO_PREFIX == 0 && flags & INFO_CHARSET != 0 {
                        out.push(i + 4);
                    }
                    i = to;
                }
                _ => return,
            }
        }
    }

    /// The ASCII answers of the charset starting at `set`, if indexed.
    fn get(&self, set: usize) -> Option<u128> {
        let k = *self.index.get(set)?;
        let k = usize::try_from(k).ok()?.checked_sub(1)?;
        self.maps.get(k).copied()
    }
}

/// The result of [`first_units`]: the items, plus their answer for every
/// ASCII character precomputed (the items are pure functions of the code
/// and the character).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FirstSet {
    units: Vec<usize>,
    /// Bit `c` is set when some item accepts the ASCII character `c`.
    ascii: u128,
}

impl FirstSet {
    /// Whether some item accepts `c`.
    fn accepts(&self, code: &[u32], c: u32) -> bool {
        if c < 128 {
            (self.ascii >> c) & 1 != 0
        } else {
            self.units.iter().any(|&u| State::unit(code, u, c))
        }
    }
}

/// [`first_units`] from opcode index `pc`; `true` if every path from `pc`
/// starts by consuming a character tested by an item in `out`.
fn first_units_from(code: &[u32], mut pc: usize, out: &mut Vec<usize>, depth: usize) -> bool {
    if depth > 32 || out.len() > 16 {
        return false;
    }
    loop {
        match w(code, pc) {
            op::MARK | op::AT => pc += 2,
            op::LITERAL
            | op::NOT_LITERAL
            | op::LITERAL_IGNORE
            | op::NOT_LITERAL_IGNORE
            | op::LITERAL_UNI_IGNORE
            | op::NOT_LITERAL_UNI_IGNORE
            | op::LITERAL_LOC_IGNORE
            | op::NOT_LITERAL_LOC_IGNORE
            | op::CATEGORY
            | op::ANY
            | op::ANY_ALL
            | op::IN
            | op::IN_IGNORE
            | op::IN_UNI_IGNORE
            | op::IN_LOC_IGNORE => {
                out.push(pc);
                return true;
            }
            op::BRANCH => {
                let mut alt = pc + 1;
                while w(code, alt) != 0 {
                    if !first_units_from(code, alt + 1, out, depth + 1) {
                        return false;
                    }
                    alt += wu(code, alt);
                }
                return true;
            }
            op::REPEAT_ONE | op::MIN_REPEAT_ONE | op::POSSESSIVE_REPEAT_ONE => {
                let item = pc + 4;
                if !matches!(
                    w(code, item),
                    op::LITERAL
                        | op::NOT_LITERAL
                        | op::LITERAL_IGNORE
                        | op::NOT_LITERAL_IGNORE
                        | op::LITERAL_UNI_IGNORE
                        | op::NOT_LITERAL_UNI_IGNORE
                        | op::LITERAL_LOC_IGNORE
                        | op::NOT_LITERAL_LOC_IGNORE
                        | op::CATEGORY
                        | op::ANY
                        | op::ANY_ALL
                        | op::IN
                        | op::IN_IGNORE
                        | op::IN_UNI_IGNORE
                        | op::IN_LOC_IGNORE
                ) {
                    return false;
                }
                out.push(item);
                if w(code, pc + 2) >= 1 {
                    return true;
                }
                let skip = wu(code, pc + 1);
                if skip == 0 {
                    return false;
                }
                pc += 1 + skip;
            }
            op::JUMP => {
                let skip = wu(code, pc + 1);
                if skip == 0 {
                    return false;
                }
                pc += 1 + skip;
            }
            _ => return false,
        }
    }
}

/// The matcher state (`SRE_STATE`) for one subject.
#[derive(Debug, Clone)]
pub(crate) struct State<'s> {
    /// The whole subject (`beginning` is index 0).
    s: &'s [u32],
    /// `state->start`: where the current attempt began.
    pub(crate) start: usize,
    /// `state->end`: the clamped `endpos`.
    pub(crate) end: usize,
    /// `state->ptr`: the end of the match after success.
    pub(crate) ptr: usize,
    /// Group marks (`mark[2 * (g - 1)]`, `mark[2 * (g - 1) + 1]`).
    pub(crate) marks: Vec<Option<usize>>,
    pub(crate) lastmark: isize,
    pub(crate) lastindex: isize,
    /// `state->match_all` (fullmatch).
    pub(crate) match_all: bool,
    /// `state->must_advance` (iteration after an empty match).
    pub(crate) must_advance: bool,
    repeat: Option<usize>,
    repeats: Vec<Repeat>,
    free_repeats: Vec<usize>,
    mark_stack: Vec<Option<usize>>,
    /// The frame stack of [`State::run`], kept between calls so a search
    /// doesn't allocate at every position.
    frames: Vec<Ctx>,
    /// The program's charsets with their ASCII answers.
    sets: &'s AsciiSets,
    /// Set when growing a matcher stack failed (`SRE_ERROR_MEMORY`); the
    /// match is abandoned and the caller raises `MemoryError`.
    out_of_memory: bool,
}

/// Makes room for `extra` more items without aborting on allocation failure.
fn try_grow<T>(v: &mut Vec<T>, extra: usize) -> bool {
    v.try_reserve(extra).is_ok()
}

/// The matcher's growable buffers, kept per thread between matches so a
/// call doesn't allocate them again. Only their capacity is reused: they are
/// handed out empty.
#[derive(Debug, Default)]
struct Buffers {
    marks: Vec<Option<usize>>,
    repeats: Vec<Repeat>,
    free_repeats: Vec<usize>,
    mark_stack: Vec<Option<usize>>,
    frames: Vec<Ctx>,
}

thread_local! {
    static BUFFERS: std::cell::Cell<Option<Buffers>> = const { std::cell::Cell::new(None) };
}

impl Buffers {
    /// Buffers larger than this many items are freed, not kept.
    const KEEP_MAX: usize = 256;

    /// Empty buffers, reusing the thread's spare ones if there are any.
    fn take() -> Self {
        BUFFERS
            .try_with(std::cell::Cell::take)
            .ok()
            .flatten()
            .unwrap_or_default()
    }

    /// Keeps `self` (emptied) for the thread's next match, unless it grew large.
    fn give_back(mut self) {
        if self.marks.capacity() > Self::KEEP_MAX
            || self.repeats.capacity() > Self::KEEP_MAX
            || self.free_repeats.capacity() > Self::KEEP_MAX
            || self.mark_stack.capacity() > Self::KEEP_MAX
            || self.frames.capacity() > Self::KEEP_MAX
        {
            return;
        }
        self.marks.clear();
        self.repeats.clear();
        self.free_repeats.clear();
        self.mark_stack.clear();
        self.frames.clear();
        // Fails only while the thread is being torn down; then just free them.
        let _ = BUFFERS.try_with(|cell| cell.set(Some(self)));
    }
}

impl Drop for State<'_> {
    fn drop(&mut self) {
        Buffers {
            marks: std::mem::take(&mut self.marks),
            repeats: std::mem::take(&mut self.repeats),
            free_repeats: std::mem::take(&mut self.free_repeats),
            mark_stack: std::mem::take(&mut self.mark_stack),
            frames: std::mem::take(&mut self.frames),
        }
        .give_back();
    }
}

impl<'s> State<'s> {
    /// `state_init` with already clamped `start` and `end`.
    pub(crate) fn new(
        s: &'s [u32],
        start: usize,
        end: usize,
        groups: usize,
        sets: &'s AsciiSets,
    ) -> Self {
        let mut buffers = Buffers::take();
        buffers.marks.resize(groups * 2, None);
        Self {
            s,
            sets,
            start,
            end,
            ptr: start,
            marks: buffers.marks,
            lastmark: -1,
            lastindex: -1,
            match_all: false,
            must_advance: false,
            repeat: None,
            repeats: buffers.repeats,
            free_repeats: buffers.free_repeats,
            mark_stack: buffers.mark_stack,
            frames: buffers.frames,
            out_of_memory: false,
        }
    }

    /// `state_reset`: forget captures and repeats before the next scan step.
    pub(crate) fn reset(&mut self) {
        self.lastmark = -1;
        self.lastindex = -1;
        self.repeat = None;
        self.repeats.clear();
        self.free_repeats.clear();
        self.mark_stack.clear();
        self.out_of_memory = false;
        self.marks.iter_mut().for_each(|m| *m = None);
    }

    /// `RESET_CAPTURE_GROUP`.
    fn reset_capture(&mut self) {
        self.lastmark = -1;
        self.lastindex = -1;
    }

    /// The span of group `g` (1-based) as `pattern_new_match` reads it.
    pub(crate) fn group_span(&self, g: usize) -> Option<(usize, usize)> {
        let j = 2 * (g - 1);
        if signed(j + 1) > self.lastmark {
            return None;
        }
        match (self.marks.get(j).copied()?, self.marks.get(j + 1).copied()?) {
            (Some(a), Some(b)) => Some((a, b)),
            _ => None,
        }
    }

    fn c(&self, i: usize) -> u32 {
        self.s.get(i).copied().unwrap_or(0)
    }

    fn alloc_repeat(&mut self, r: Repeat) -> usize {
        if let Some(i) = self.free_repeats.pop() {
            if let Some(slot) = self.repeats.get_mut(i) {
                *slot = r;
                return i;
            }
        }
        if !try_grow(&mut self.repeats, 1) {
            self.out_of_memory = true;
        }
        self.repeats.push(r);
        self.repeats.len() - 1
    }

    fn free_repeat(&mut self, i: usize) {
        self.free_repeats.push(i);
    }

    fn rep(&self, i: usize) -> Repeat {
        self.repeats.get(i).copied().unwrap_or(Repeat {
            count: 0,
            pattern: 0,
            prev: None,
            last_ptr: None,
        })
    }

    fn rep_mut(&mut self, i: usize) -> Option<&mut Repeat> {
        self.repeats.get_mut(i)
    }

    fn set_mark(&mut self, i: usize, v: Option<usize>) {
        if i >= self.marks.len() {
            self.marks.resize(i + 1, None);
        }
        if let Some(m) = self.marks.get_mut(i) {
            *m = v;
        }
    }

    fn mark_push(&mut self, lastmark: isize) {
        if lastmark >= 0 {
            let n = lastmark.unsigned_abs() + 1;
            if !try_grow(&mut self.mark_stack, n) {
                self.out_of_memory = true;
                return;
            }
            for i in 0..n {
                let v = self.marks.get(i).copied().flatten();
                self.mark_stack.push(v);
            }
        }
    }

    fn mark_restore(&mut self, lastmark: isize, discard: bool) {
        if lastmark >= 0 {
            let n = lastmark.unsigned_abs() + 1;
            let base = self.mark_stack.len().saturating_sub(n);
            for i in 0..n {
                let v = self.mark_stack.get(base + i).copied().flatten();
                self.set_mark(i, v);
            }
            if discard {
                self.mark_stack.truncate(base);
            }
        }
    }

    /// `MARK_POP`.
    fn mark_pop(&mut self, lastmark: isize) {
        self.mark_restore(lastmark, true);
    }

    /// `MARK_POP_KEEP`.
    fn mark_pop_keep(&mut self, lastmark: isize) {
        self.mark_restore(lastmark, false);
    }

    /// `MARK_POP_DISCARD`.
    fn mark_pop_discard(&mut self, lastmark: isize) {
        if lastmark >= 0 {
            let n = lastmark.unsigned_abs() + 1;
            let base = self.mark_stack.len().saturating_sub(n);
            self.mark_stack.truncate(base);
        }
    }

    fn lastmark_save(&self, ctx: &mut Ctx) {
        ctx.lastmark = self.lastmark;
        ctx.lastindex = self.lastindex;
    }

    fn lastmark_restore(&mut self, ctx: &Ctx) {
        self.lastmark = ctx.lastmark;
        self.lastindex = ctx.lastindex;
    }

    /// `SRE(at)`.
    fn at(&self, ptr: usize, code: u32) -> bool {
        let end = self.end;
        match code {
            at::BEGINNING | at::BEGINNING_STRING => ptr == 0,
            at::BEGINNING_LINE => ptr == 0 || is_linebreak(self.c(ptr.wrapping_sub(1))),
            at::END => (ptr + 1 == end && is_linebreak(self.c(ptr))) || ptr == end,
            at::END_LINE => ptr == end || is_linebreak(self.c(ptr)),
            at::END_STRING => ptr == end,
            at::BOUNDARY | at::NON_BOUNDARY => {
                self.boundary(ptr, unicode::is_ascii_word, code == at::BOUNDARY)
            }
            at::LOC_BOUNDARY | at::LOC_NON_BOUNDARY => {
                self.boundary(ptr, is_loc_word, code == at::LOC_BOUNDARY)
            }
            at::UNI_BOUNDARY | at::UNI_NON_BOUNDARY => {
                self.boundary(ptr, uni_word, code == at::UNI_BOUNDARY)
            }
            _ => false,
        }
    }

    fn boundary(&self, ptr: usize, is_word: fn(u32) -> bool, want_boundary: bool) -> bool {
        if self.end == 0 {
            return false;
        }
        let thatp = ptr > 0 && is_word(self.c(ptr - 1));
        let thisp = ptr < self.end && is_word(self.c(ptr));
        (thisp != thatp) == want_boundary
    }

    /// Whether the single-character item at `pc` (an opcode) accepts `c`.
    fn unit(code: &[u32], pc: usize, c: u32) -> bool {
        let arg = w(code, pc + 1);
        match w(code, pc) {
            op::ANY => !is_linebreak(c),
            op::ANY_ALL => true,
            op::LITERAL => c == arg,
            op::NOT_LITERAL => c != arg,
            op::LITERAL_IGNORE => lower_ascii(c) == arg,
            op::NOT_LITERAL_IGNORE => lower_ascii(c) != arg,
            op::LITERAL_UNI_IGNORE => uni_lower(c) == arg,
            op::NOT_LITERAL_UNI_IGNORE => uni_lower(c) != arg,
            op::LITERAL_LOC_IGNORE => char_loc_ignore(arg, c),
            op::NOT_LITERAL_LOC_IGNORE => !char_loc_ignore(arg, c),
            op::CATEGORY => category(arg, c),
            op::IN => charset(code, pc + 2, c),
            op::IN_IGNORE => charset(code, pc + 2, lower_ascii(c)),
            op::IN_UNI_IGNORE => charset(code, pc + 2, uni_lower(c)),
            op::IN_LOC_IGNORE => charset_loc_ignore(code, pc + 2, c),
            _ => false,
        }
    }

    /// `charset(code, set, c)`, from the precomputed answers for ASCII.
    fn in_set(&self, code: &[u32], set: usize, c: u32) -> bool {
        if c < 128 {
            if let Some(map) = self.sets.get(set) {
                return (map >> c) & 1 != 0;
            }
        }
        charset(code, set, c)
    }

    /// [`Self::unit`], with charsets answered through [`Self::in_set`].
    fn unit_fast(&self, code: &[u32], pc: usize, c: u32) -> bool {
        match w(code, pc) {
            op::IN => self.in_set(code, pc + 2, c),
            op::IN_IGNORE => self.in_set(code, pc + 2, lower_ascii(c)),
            op::IN_UNI_IGNORE => self.in_set(code, pc + 2, uni_lower(c)),
            _ => Self::unit(code, pc, c),
        }
    }

    /// `SRE(count)`: how many times the single-character item at `pc`
    /// matches from `self.ptr`, at most `maxcount` times.
    fn count(&self, code: &[u32], pc: usize, maxcount: u32) -> isize {
        let ptr = self.ptr;
        let mut end = self.end;
        let maxcount_s = isize::try_from(maxcount).unwrap_or(isize::MAX);
        if maxcount_s < signed(end) - signed(ptr) && maxcount != MAXREPEAT {
            end = ptr + maxcount as usize;
        }
        if w(code, pc) == op::ANY_ALL {
            return signed(end) - signed(ptr);
        }
        let Some(s) = self.s.get(ptr..end) else {
            let mut p = ptr;
            while p < end && self.unit_fast(code, pc, self.c(p)) {
                p += 1;
            }
            return signed(p) - signed(ptr);
        };
        // The opcode is fixed for the whole run, so pick the test once.
        let arg = w(code, pc + 1);
        let n = match w(code, pc) {
            op::LITERAL => s.iter().take_while(|&&c| c == arg).count(),
            op::NOT_LITERAL => s.iter().take_while(|&&c| c != arg).count(),
            op::CATEGORY => s.iter().take_while(|&&c| category(arg, c)).count(),
            op::IN => match self.sets.get(pc + 2) {
                Some(map) => s
                    .iter()
                    .take_while(|&&c| {
                        if c < 128 {
                            (map >> c) & 1 != 0
                        } else {
                            charset(code, pc + 2, c)
                        }
                    })
                    .count(),
                None => s.iter().take_while(|&&c| charset(code, pc + 2, c)).count(),
            },
            _ => s
                .iter()
                .take_while(|&&c| self.unit_fast(code, pc, c))
                .count(),
        };
        signed(n)
    }

    /// `SRE(match)`: runs the program from `pc` at `self.ptr`.
    pub(crate) fn run(&mut self, code: &[u32], pc: usize, toplevel: bool) -> bool {
        let mut stack = std::mem::take(&mut self.frames);
        stack.clear();
        let found = self.run_with(code, pc, toplevel, &mut stack);
        self.frames = stack;
        found
    }

    fn run_with(&mut self, code: &[u32], pc: usize, toplevel: bool, stack: &mut Vec<Ctx>) -> bool {
        if self.out_of_memory || !try_grow(stack, 1) {
            self.out_of_memory = true;
            return false;
        }
        stack.push(Ctx::new(pc, toplevel, Jump::None));
        let mut resume: Option<(Jump, bool)> = None;
        loop {
            if self.out_of_memory {
                return false;
            }
            let Some(ctx) = stack.last_mut() else {
                return false;
            };
            let flow = match resume.take() {
                None => self.enter(code, ctx),
                Some((jump, ret)) => self.resume(code, ctx, jump, ret),
            };
            let step = match flow {
                Flow::Step(step) => step,
                Flow::Continue(pc, ptr) => self.dispatch(code, ctx, pc, ptr),
            };
            match step {
                Step::Call { jump, pc, toplevel } => {
                    if !try_grow(stack, 1) {
                        self.out_of_memory = true;
                        return false;
                    }
                    stack.push(Ctx::new(pc, toplevel, jump));
                }
                Step::Ret(ret) => {
                    let done = stack.pop();
                    if stack.is_empty() {
                        return ret;
                    }
                    resume = Some((done.map_or(Jump::None, |c| c.jump), ret));
                }
            }
        }
    }

    /// The `entrance` label: the INFO check, then dispatch.
    fn enter(&mut self, code: &[u32], ctx: &mut Ctx) -> Flow {
        let ptr = self.ptr;
        let mut pc = ctx.pc;
        if w(code, pc) == op::INFO {
            let min = wu(code, pc + 3);
            if min != 0 && self.end.wrapping_sub(ptr) < min {
                return Flow::Step(Step::Ret(false));
            }
            pc += wu(code, pc + 1) + 1;
        }
        Flow::Continue(pc, ptr)
    }

    fn call(ctx: &mut Ctx, pc: usize, ptr: usize, jump: Jump, next: usize, top: bool) -> Step {
        ctx.pc = pc;
        ctx.ptr = ptr;
        Step::Call {
            jump,
            pc: next,
            toplevel: top,
        }
    }

    /// The opcode loop, from `pc` (an opcode index) at `ptr`.
    fn dispatch(&mut self, code: &[u32], ctx: &mut Ctx, mut pc: usize, mut ptr: usize) -> Step {
        let end = self.end;
        loop {
            let opcode = w(code, pc);
            pc += 1;
            match opcode {
                op::MARK => {
                    let i = wu(code, pc);
                    if i & 1 == 1 {
                        self.lastindex = signed(i / 2 + 1);
                    }
                    if signed(i) > self.lastmark {
                        let first = usize::try_from(self.lastmark + 1).unwrap_or(0);
                        for j in first..i {
                            self.set_mark(j, None);
                        }
                        self.lastmark = signed(i);
                    }
                    self.set_mark(i, Some(ptr));
                    pc += 1;
                }
                op::LITERAL
                | op::NOT_LITERAL
                | op::LITERAL_IGNORE
                | op::NOT_LITERAL_IGNORE
                | op::LITERAL_UNI_IGNORE
                | op::NOT_LITERAL_UNI_IGNORE
                | op::LITERAL_LOC_IGNORE
                | op::NOT_LITERAL_LOC_IGNORE
                | op::CATEGORY => {
                    if ptr >= end || !self.unit_fast(code, pc - 1, self.c(ptr)) {
                        return Step::Ret(false);
                    }
                    pc += 1;
                    ptr += 1;
                }
                op::ANY | op::ANY_ALL => {
                    if ptr >= end || !self.unit_fast(code, pc - 1, self.c(ptr)) {
                        return Step::Ret(false);
                    }
                    ptr += 1;
                }
                op::IN | op::IN_IGNORE | op::IN_UNI_IGNORE | op::IN_LOC_IGNORE => {
                    if ptr >= end || !self.unit_fast(code, pc - 1, self.c(ptr)) {
                        return Step::Ret(false);
                    }
                    pc += wu(code, pc);
                    ptr += 1;
                }
                op::SUCCESS => {
                    if ctx.toplevel
                        && ((self.match_all && ptr != end)
                            || (self.must_advance && ptr == self.start))
                    {
                        return Step::Ret(false);
                    }
                    self.ptr = ptr;
                    return Step::Ret(true);
                }
                op::AT => {
                    if !self.at(ptr, w(code, pc)) {
                        return Step::Ret(false);
                    }
                    pc += 1;
                }
                op::JUMP | op::INFO => {
                    pc += wu(code, pc);
                }
                op::BRANCH => {
                    self.lastmark_save(ctx);
                    if self.repeat.is_some() {
                        self.mark_push(ctx.lastmark);
                    }
                    return self.branch_loop(code, ctx, pc, ptr);
                }
                op::REPEAT_ONE => {
                    let min = wi(code, pc + 1);
                    if min > signed(end) - signed(ptr) {
                        return Step::Ret(false);
                    }
                    self.ptr = ptr;
                    ctx.count = self.count(code, pc + 3, w(code, pc + 2));
                    ptr = ptr.wrapping_add_signed(ctx.count);
                    if ctx.count < min {
                        return Step::Ret(false);
                    }
                    let tail = pc + wu(code, pc);
                    if w(code, tail) == op::SUCCESS
                        && ptr == self.end
                        && !(ctx.toplevel && self.must_advance && ptr == self.start)
                    {
                        self.ptr = ptr;
                        return Step::Ret(true);
                    }
                    // Exactly `min` items: sre tries the tail once, and if it
                    // fails only restores `lastmark`/`lastindex` (no repeat,
                    // so no mark stack) before failing. Every frame that a
                    // failure reaches and that reads them restores its own
                    // saved values first, so running the tail in this frame
                    // gives the same result without a child frame.
                    if ctx.count == min && self.repeat.is_none() {
                        pc = tail;
                        continue;
                    }
                    // A SUCCESS tail with no repeat: the first child sre
                    // calls (with all `count` items) runs only SUCCESS, and
                    // when that succeeds so does this frame, with nothing
                    // else changed. Answer it here, without the child.
                    if w(code, tail) == op::SUCCESS
                        && self.repeat.is_none()
                        && !(ctx.toplevel
                            && ((self.match_all && ptr != end)
                                || (self.must_advance && ptr == self.start)))
                    {
                        self.ptr = ptr;
                        return Step::Ret(true);
                    }
                    self.lastmark_save(ctx);
                    if self.repeat.is_some() {
                        self.mark_push(ctx.lastmark);
                    }
                    if w(code, tail) == op::LITERAL {
                        ctx.chr = w(code, tail + 1);
                        return self.repeat_one_literal(code, ctx, pc, ptr);
                    }
                    return self.repeat_one_general(code, ctx, pc, ptr);
                }
                op::MIN_REPEAT_ONE => {
                    let min = wi(code, pc + 1);
                    if min > signed(end) - signed(ptr) {
                        return Step::Ret(false);
                    }
                    self.ptr = ptr;
                    if min == 0 {
                        ctx.count = 0;
                    } else {
                        let n = self.count(code, pc + 3, w(code, pc + 1));
                        if n < min {
                            return Step::Ret(false);
                        }
                        ctx.count = n;
                        ptr = ptr.wrapping_add_signed(n);
                    }
                    let tail = pc + wu(code, pc);
                    if w(code, tail) == op::SUCCESS
                        && !(ctx.toplevel
                            && ((self.match_all && ptr != self.end)
                                || (self.must_advance && ptr == self.start)))
                    {
                        self.ptr = ptr;
                        return Step::Ret(true);
                    }
                    self.lastmark_save(ctx);
                    if self.repeat.is_some() {
                        self.mark_push(ctx.lastmark);
                    }
                    return self.min_repeat_one_loop(code, ctx, pc, ptr);
                }
                op::POSSESSIVE_REPEAT_ONE => {
                    let min = wi(code, pc + 1);
                    if min > signed(end) - signed(ptr) {
                        return Step::Ret(false);
                    }
                    self.ptr = ptr;
                    ctx.count = self.count(code, pc + 3, w(code, pc + 2));
                    ptr = ptr.wrapping_add_signed(ctx.count);
                    if ctx.count < min {
                        return Step::Ret(false);
                    }
                    pc += wu(code, pc);
                }
                op::REPEAT => {
                    let rep = self.alloc_repeat(Repeat {
                        count: -1,
                        pattern: pc,
                        prev: self.repeat,
                        last_ptr: None,
                    });
                    ctx.rep = rep;
                    self.repeat = Some(rep);
                    self.ptr = ptr;
                    let next = pc + wu(code, pc);
                    return Self::call(ctx, pc, ptr, Jump::Repeat, next, ctx.toplevel);
                }
                op::MAX_UNTIL => {
                    let Some(rep) = self.repeat else {
                        return Step::Ret(false);
                    };
                    ctx.rep = rep;
                    self.ptr = ptr;
                    let r = self.rep(rep);
                    ctx.count = r.count + 1;
                    if ctx.count < wi(code, r.pattern + 1) {
                        if let Some(r) = self.rep_mut(rep) {
                            r.count = ctx.count;
                        }
                        return Self::call(
                            ctx,
                            pc,
                            ptr,
                            Jump::MaxUntil1,
                            r.pattern + 3,
                            ctx.toplevel,
                        );
                    }
                    let max = w(code, r.pattern + 2);
                    if (ctx.count < signed(max as usize) || max == MAXREPEAT)
                        && Some(self.ptr) != r.last_ptr
                    {
                        self.lastmark_save(ctx);
                        self.mark_push(ctx.lastmark);
                        ctx.saved_last_ptr = r.last_ptr;
                        let sptr = self.ptr;
                        if let Some(r) = self.rep_mut(rep) {
                            r.count = ctx.count;
                            r.last_ptr = Some(sptr);
                        }
                        return Self::call(
                            ctx,
                            pc,
                            ptr,
                            Jump::MaxUntil2,
                            r.pattern + 3,
                            ctx.toplevel,
                        );
                    }
                    self.repeat = r.prev;
                    return Self::call(ctx, pc, ptr, Jump::MaxUntil3, pc, ctx.toplevel);
                }
                op::MIN_UNTIL => {
                    let Some(rep) = self.repeat else {
                        return Step::Ret(false);
                    };
                    ctx.rep = rep;
                    self.ptr = ptr;
                    let r = self.rep(rep);
                    ctx.count = r.count + 1;
                    if ctx.count < wi(code, r.pattern + 1) {
                        if let Some(r) = self.rep_mut(rep) {
                            r.count = ctx.count;
                        }
                        return Self::call(
                            ctx,
                            pc,
                            ptr,
                            Jump::MinUntil1,
                            r.pattern + 3,
                            ctx.toplevel,
                        );
                    }
                    self.repeat = r.prev;
                    self.lastmark_save(ctx);
                    if self.repeat.is_some() {
                        self.mark_push(ctx.lastmark);
                    }
                    return Self::call(ctx, pc, ptr, Jump::MinUntil2, pc, ctx.toplevel);
                }
                op::POSSESSIVE_REPEAT => {
                    self.ptr = ptr;
                    let rep = self.alloc_repeat(Repeat {
                        count: -1,
                        pattern: 0,
                        prev: self.repeat,
                        last_ptr: None,
                    });
                    ctx.rep = rep;
                    self.repeat = Some(rep);
                    ctx.count = 0;
                    return self.poss_min_loop(code, ctx, pc, ptr);
                }
                op::ATOMIC_GROUP => {
                    self.ptr = ptr;
                    return Self::call(ctx, pc, ptr, Jump::AtomicGroup, pc + 1, false);
                }
                op::GROUPREF
                | op::GROUPREF_IGNORE
                | op::GROUPREF_UNI_IGNORE
                | op::GROUPREF_LOC_IGNORE => {
                    let groupref = wu(code, pc).saturating_mul(2);
                    if signed(groupref) >= self.lastmark {
                        return Step::Ret(false);
                    }
                    let (Some(mut p), Some(e)) = (
                        self.marks.get(groupref).copied().flatten(),
                        self.marks.get(groupref + 1).copied().flatten(),
                    ) else {
                        return Step::Ret(false);
                    };
                    if e < p {
                        return Step::Ret(false);
                    }
                    let fold: fn(u32) -> u32 = match opcode {
                        op::GROUPREF_IGNORE => lower_ascii,
                        op::GROUPREF_UNI_IGNORE => uni_lower,
                        op::GROUPREF_LOC_IGNORE => lower_locale,
                        _ => |c| c,
                    };
                    while p < e {
                        if ptr >= end || fold(self.c(ptr)) != fold(self.c(p)) {
                            return Step::Ret(false);
                        }
                        p += 1;
                        ptr += 1;
                    }
                    pc += 1;
                }
                op::GROUPREF_EXISTS => {
                    let groupref = wu(code, pc).saturating_mul(2);
                    let exists = signed(groupref) < self.lastmark
                        && matches!(
                            (
                                self.marks.get(groupref).copied().flatten(),
                                self.marks.get(groupref + 1).copied().flatten(),
                            ),
                            (Some(p), Some(e)) if e >= p
                        );
                    if exists {
                        pc += 2;
                    } else {
                        pc += wu(code, pc + 1);
                    }
                }
                op::ASSERT => {
                    let back = wu(code, pc + 1);
                    if ptr < back {
                        return Step::Ret(false);
                    }
                    self.ptr = ptr - back;
                    return Self::call(ctx, pc, ptr, Jump::Assert, pc + 2, false);
                }
                op::ASSERT_NOT => {
                    let back = wu(code, pc + 1);
                    if ptr >= back {
                        self.ptr = ptr - back;
                        self.lastmark_save(ctx);
                        if self.repeat.is_some() {
                            self.mark_push(ctx.lastmark);
                        }
                        return Self::call(ctx, pc, ptr, Jump::AssertNot, pc + 2, false);
                    }
                    pc += wu(code, pc);
                }
                _ => return Step::Ret(false),
            }
        }
    }

    fn branch_loop(&mut self, code: &[u32], ctx: &mut Ctx, mut pc: usize, ptr: usize) -> Step {
        let end = self.end;
        while w(code, pc) != 0 {
            let first = w(code, pc + 1);
            let skip = (first == op::LITERAL && (ptr >= end || self.c(ptr) != w(code, pc + 2)))
                || (first == op::IN && (ptr >= end || !self.in_set(code, pc + 3, self.c(ptr))));
            if skip {
                pc += wu(code, pc);
                continue;
            }
            self.ptr = ptr;
            return Self::call(ctx, pc, ptr, Jump::Branch, pc + 1, ctx.toplevel);
        }
        if self.repeat.is_some() {
            self.mark_pop_discard(ctx.lastmark);
        }
        Step::Ret(false)
    }

    fn repeat_one_literal(
        &mut self,
        code: &[u32],
        ctx: &mut Ctx,
        pc: usize,
        mut ptr: usize,
    ) -> Step {
        let min = wi(code, pc + 1);
        let end = self.end;
        while ctx.count >= min && (ptr >= end || self.c(ptr) != ctx.chr) {
            ptr = ptr.wrapping_sub(1);
            ctx.count -= 1;
        }
        if ctx.count >= min {
            self.ptr = ptr;
            let next = pc + wu(code, pc);
            return Self::call(ctx, pc, ptr, Jump::RepeatOne1, next, ctx.toplevel);
        }
        if self.repeat.is_some() {
            self.mark_pop_discard(ctx.lastmark);
        }
        Step::Ret(false)
    }

    fn repeat_one_general(&mut self, code: &[u32], ctx: &mut Ctx, pc: usize, ptr: usize) -> Step {
        let min = wi(code, pc + 1);
        if ctx.count >= min {
            self.ptr = ptr;
            let next = pc + wu(code, pc);
            return Self::call(ctx, pc, ptr, Jump::RepeatOne2, next, ctx.toplevel);
        }
        if self.repeat.is_some() {
            self.mark_pop_discard(ctx.lastmark);
        }
        Step::Ret(false)
    }

    fn min_repeat_one_loop(&mut self, code: &[u32], ctx: &mut Ctx, pc: usize, ptr: usize) -> Step {
        let max = w(code, pc + 2);
        if max == MAXREPEAT || ctx.count <= signed(max as usize) {
            self.ptr = ptr;
            let next = pc + wu(code, pc);
            return Self::call(ctx, pc, ptr, Jump::MinRepeatOne, next, ctx.toplevel);
        }
        if self.repeat.is_some() {
            self.mark_pop_discard(ctx.lastmark);
        }
        Step::Ret(false)
    }

    fn poss_min_loop(&mut self, code: &[u32], ctx: &mut Ctx, pc: usize, ptr: usize) -> Step {
        if ctx.count < wi(code, pc + 1) {
            return Self::call(ctx, pc, ptr, Jump::PossRepeat1, pc + 3, false);
        }
        ctx.poss_ptr = None;
        self.poss_max_loop(code, ctx, pc, ptr)
    }

    fn poss_max_loop(&mut self, code: &[u32], ctx: &mut Ctx, pc: usize, ptr: usize) -> Step {
        let max = w(code, pc + 2);
        if (ctx.count < signed(max as usize) || max == MAXREPEAT) && Some(self.ptr) != ctx.poss_ptr
        {
            self.lastmark_save(ctx);
            self.mark_push(ctx.lastmark);
            ctx.poss_ptr = Some(self.ptr);
            return Self::call(ctx, pc, ptr, Jump::PossRepeat2, pc + 3, false);
        }
        self.poss_finish(code, ctx, pc)
    }

    fn poss_finish(&mut self, code: &[u32], ctx: &mut Ctx, pc: usize) -> Step {
        self.repeat = self.rep(ctx.rep).prev;
        self.free_repeat(ctx.rep);
        // The skip lands on the body's SUCCESS; the tail follows it.
        let tail = pc + wu(code, pc) + 1;
        let ptr = self.ptr;
        self.dispatch(code, ctx, tail, ptr)
    }

    /// The code after each `jump_*` label.
    fn resume(&mut self, code: &[u32], ctx: &mut Ctx, jump: Jump, ret: bool) -> Flow {
        let pc = ctx.pc;
        let ptr = ctx.ptr;
        let step = match jump {
            Jump::None => Step::Ret(ret),
            Jump::Branch => {
                if ret {
                    if self.repeat.is_some() {
                        self.mark_pop_discard(ctx.lastmark);
                    }
                    return Flow::Step(Step::Ret(true));
                }
                if self.repeat.is_some() {
                    self.mark_pop_keep(ctx.lastmark);
                }
                self.lastmark_restore(ctx);
                let next = pc + wu(code, pc);
                self.branch_loop(code, ctx, next, ptr)
            }
            Jump::RepeatOne1 | Jump::RepeatOne2 | Jump::MinRepeatOne => {
                if ret {
                    if self.repeat.is_some() {
                        self.mark_pop_discard(ctx.lastmark);
                    }
                    return Flow::Step(Step::Ret(true));
                }
                if self.repeat.is_some() {
                    self.mark_pop_keep(ctx.lastmark);
                }
                self.lastmark_restore(ctx);
                match jump {
                    Jump::RepeatOne1 => {
                        ctx.count -= 1;
                        self.repeat_one_literal(code, ctx, pc, ptr.wrapping_sub(1))
                    }
                    Jump::RepeatOne2 => {
                        ctx.count -= 1;
                        self.repeat_one_general(code, ctx, pc, ptr.wrapping_sub(1))
                    }
                    _ => {
                        self.ptr = ptr;
                        if self.count(code, pc + 3, 1) == 0 {
                            if self.repeat.is_some() {
                                self.mark_pop_discard(ctx.lastmark);
                            }
                            Step::Ret(false)
                        } else {
                            ctx.count += 1;
                            self.min_repeat_one_loop(code, ctx, pc, ptr + 1)
                        }
                    }
                }
            }
            Jump::Repeat => {
                self.repeat = self.rep(ctx.rep).prev;
                self.free_repeat(ctx.rep);
                Step::Ret(ret)
            }
            Jump::MaxUntil1 | Jump::MinUntil1 => {
                if ret {
                    return Flow::Step(Step::Ret(true));
                }
                let count = ctx.count - 1;
                if let Some(r) = self.rep_mut(ctx.rep) {
                    r.count = count;
                }
                self.ptr = ptr;
                Step::Ret(false)
            }
            Jump::MaxUntil2 => {
                let saved = ctx.saved_last_ptr;
                if let Some(r) = self.rep_mut(ctx.rep) {
                    r.last_ptr = saved;
                }
                if ret {
                    self.mark_pop_discard(ctx.lastmark);
                    return Flow::Step(Step::Ret(true));
                }
                self.mark_pop(ctx.lastmark);
                self.lastmark_restore(ctx);
                let count = ctx.count - 1;
                if let Some(r) = self.rep_mut(ctx.rep) {
                    r.count = count;
                }
                self.ptr = ptr;
                self.repeat = self.rep(ctx.rep).prev;
                Self::call(ctx, pc, ptr, Jump::MaxUntil3, pc, ctx.toplevel)
            }
            Jump::MaxUntil3 => {
                self.repeat = Some(ctx.rep);
                if ret {
                    return Flow::Step(Step::Ret(true));
                }
                self.ptr = ptr;
                Step::Ret(false)
            }
            Jump::MinUntil2 => {
                let repeat_of_tail = self.repeat;
                self.repeat = Some(ctx.rep);
                if ret {
                    if repeat_of_tail.is_some() {
                        self.mark_pop_discard(ctx.lastmark);
                    }
                    return Flow::Step(Step::Ret(true));
                }
                if repeat_of_tail.is_some() {
                    self.mark_pop(ctx.lastmark);
                }
                self.lastmark_restore(ctx);
                self.ptr = ptr;
                let r = self.rep(ctx.rep);
                let max = w(code, r.pattern + 2);
                if (ctx.count >= signed(max as usize) && max != MAXREPEAT)
                    || Some(self.ptr) == r.last_ptr
                {
                    return Flow::Step(Step::Ret(false));
                }
                ctx.saved_last_ptr = r.last_ptr;
                let (count, sptr) = (ctx.count, self.ptr);
                if let Some(r) = self.rep_mut(ctx.rep) {
                    r.count = count;
                    r.last_ptr = Some(sptr);
                }
                Self::call(ctx, pc, ptr, Jump::MinUntil3, r.pattern + 3, ctx.toplevel)
            }
            Jump::MinUntil3 => {
                let saved = ctx.saved_last_ptr;
                if let Some(r) = self.rep_mut(ctx.rep) {
                    r.last_ptr = saved;
                }
                if ret {
                    return Flow::Step(Step::Ret(true));
                }
                let count = ctx.count - 1;
                if let Some(r) = self.rep_mut(ctx.rep) {
                    r.count = count;
                }
                self.ptr = ptr;
                Step::Ret(false)
            }
            Jump::Assert => {
                if !ret {
                    return Flow::Step(Step::Ret(false));
                }
                return Flow::Continue(pc + wu(code, pc), ptr);
            }
            Jump::AssertNot => {
                if ret {
                    if self.repeat.is_some() {
                        self.mark_pop_discard(ctx.lastmark);
                    }
                    return Flow::Step(Step::Ret(false));
                }
                if self.repeat.is_some() {
                    self.mark_pop(ctx.lastmark);
                }
                self.lastmark_restore(ctx);
                return Flow::Continue(pc + wu(code, pc), ptr);
            }
            Jump::PossRepeat1 => {
                if ret {
                    ctx.count += 1;
                    self.poss_min_loop(code, ctx, pc, ptr)
                } else {
                    self.ptr = ptr;
                    self.repeat = self.rep(ctx.rep).prev;
                    self.free_repeat(ctx.rep);
                    Step::Ret(false)
                }
            }
            Jump::PossRepeat2 => {
                if ret {
                    self.mark_pop_discard(ctx.lastmark);
                    ctx.count += 1;
                    self.poss_max_loop(code, ctx, pc, ptr)
                } else {
                    self.mark_pop(ctx.lastmark);
                    self.lastmark_restore(ctx);
                    self.ptr = ctx.poss_ptr.unwrap_or(ptr);
                    self.poss_finish(code, ctx, pc)
                }
            }
            Jump::AtomicGroup => {
                if !ret {
                    self.ptr = ptr;
                    return Flow::Step(Step::Ret(false));
                }
                return Flow::Continue(pc + wu(code, pc), self.ptr);
            }
        };
        Flow::Step(step)
    }

    /// `sre_match`: a match anchored at `self.start`.
    ///
    /// # Errors
    /// `MemoryError` when the matcher's stacks can't grow, as sre raises it.
    pub(crate) fn sre_match(&mut self, code: &[u32]) -> Result<bool> {
        self.ptr = self.start;
        let found = self.run(code, 0, true);
        self.status(found)
    }

    /// `SRE(search)`.
    ///
    /// # Errors
    /// `MemoryError` when the matcher's stacks can't grow, as sre raises it.
    pub(crate) fn search(&mut self, code: &[u32], first: Option<&FirstSet>) -> Result<bool> {
        let found = self.search_inner(code, first);
        self.status(found)
    }

    /// Turns `SRE_ERROR_MEMORY` into `MemoryError` (`PyErr_NoMemory`).
    fn status(&self, found: bool) -> Result<bool> {
        if self.out_of_memory {
            Err(Error::Memory(String::new()))
        } else {
            Ok(found)
        }
    }

    fn search_inner(&mut self, code: &[u32], first: Option<&FirstSet>) -> bool {
        let mut ptr = self.start;
        let mut end = self.end;
        if ptr > end {
            return false;
        }
        let mut pc = 0;
        let mut flags = 0;
        let mut prefix: Option<(usize, usize, usize)> = None; // (len, skip, index)
        let mut charset_at: Option<usize> = None;
        if w(code, 0) == op::INFO {
            flags = w(code, 2);
            let min = wu(code, 3);
            if min != 0 && end - ptr < min {
                return false;
            }
            if min > 1 {
                end = end.saturating_sub(min - 1);
                if end <= ptr {
                    end = ptr;
                }
            }
            if flags & INFO_PREFIX != 0 {
                prefix = Some((wu(code, 5), wu(code, 6), 7));
            } else if flags & INFO_CHARSET != 0 {
                charset_at = Some(5);
            }
            pc = 1 + wu(code, 1);
        }
        let literal = flags & INFO_LITERAL != 0;
        if let Some((len, skip, at)) = prefix {
            if len == 1 {
                let c = w(code, at);
                let end = self.end;
                self.must_advance = false;
                while ptr < end {
                    while self.c(ptr) != c {
                        ptr += 1;
                        if ptr >= end {
                            return false;
                        }
                    }
                    self.start = ptr;
                    self.ptr = ptr + skip;
                    if literal {
                        return true;
                    }
                    if self.run(code, pc + 2 * skip, false) {
                        return true;
                    }
                    ptr += 1;
                    self.reset_capture();
                }
                return false;
            }
            if len > 1 {
                return self.search_prefix(code, pc, ptr, (len, skip, at), literal);
            }
        }
        if let Some(set) = charset_at {
            let end = self.end;
            self.must_advance = false;
            loop {
                while ptr < end && !self.in_set(code, set, self.c(ptr)) {
                    ptr += 1;
                }
                if ptr >= end {
                    return false;
                }
                self.start = ptr;
                self.ptr = ptr;
                if self.run(code, pc, false) {
                    return true;
                }
                ptr += 1;
                self.reset_capture();
            }
        }
        self.start = ptr;
        self.ptr = ptr;
        let mut status = self.may_start(code, first, ptr) && self.run(code, pc, true);
        self.must_advance = false;
        if !status
            && w(code, pc) == op::AT
            && matches!(w(code, pc + 1), at::BEGINNING | at::BEGINNING_STRING)
        {
            self.start = end;
            self.ptr = end;
            return false;
        }
        while !status && ptr < end {
            ptr += 1;
            // Skip the positions `may_start` rules out in one tight loop.
            if let Some(set) = first {
                while ptr < end && !set.accepts(code, self.c(ptr)) {
                    ptr += 1;
                }
            }
            self.reset_capture();
            self.start = ptr;
            self.ptr = ptr;
            status = self.may_start(code, first, ptr) && self.run(code, pc, false);
        }
        status
    }

    /// Whether a match can start at `ptr` given the pattern's [`first_units`].
    /// When this is `false` the matcher would fail there, so a search skips it.
    fn may_start(&self, code: &[u32], first: Option<&FirstSet>, ptr: usize) -> bool {
        match first {
            None => true,
            Some(set) => ptr < self.end && set.accepts(code, self.c(ptr)),
        }
    }

    /// The overlap-table scan of `SRE(search)` for prefixes longer than one.
    fn search_prefix(
        &mut self,
        code: &[u32],
        pc: usize,
        mut ptr: usize,
        (len, skip, at): (usize, usize, usize),
        literal: bool,
    ) -> bool {
        let end = self.end;
        if signed(len) > signed(end) - signed(ptr) {
            return false;
        }
        let overlap = at + len - 1;
        while ptr < end {
            let c = w(code, at);
            loop {
                let cur = self.c(ptr);
                ptr += 1;
                if cur == c {
                    break;
                }
                if ptr >= end {
                    return false;
                }
            }
            if ptr >= end {
                return false;
            }
            let mut i = 1;
            self.must_advance = false;
            loop {
                if self.c(ptr) == w(code, at + i) {
                    i += 1;
                    if i != len {
                        ptr += 1;
                        if ptr >= end {
                            return false;
                        }
                        continue;
                    }
                    self.start = ptr + 1 - len;
                    self.ptr = ptr + 1 + skip - len;
                    if literal {
                        return true;
                    }
                    if self.run(code, pc + 2 * skip, false) {
                        return true;
                    }
                    ptr += 1;
                    if ptr >= end {
                        return false;
                    }
                    self.reset_capture();
                }
                i = wu(code, overlap + i);
                if i == 0 {
                    break;
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod ascii_fast_path_tests {
    use super::*;

    #[test]
    fn ascii_fast_paths_match_the_tables() {
        for c in 0..256 {
            assert_eq!(uni_word(c), unicode::is_word(c), "{c}");
            assert_eq!(uni_digit(c), unicode::is_decimal(c), "{c}");
            assert_eq!(uni_space(c), unicode::is_space(c), "{c}");
            assert_eq!(uni_lower(c), unicode::to_lower(c), "{c}");
            assert_eq!(uni_upper(c), unicode::to_upper(c), "{c}");
        }
        for c in 0..LOWER_TABLE_LEN + 0x100 {
            assert_eq!(uni_lower(c), unicode::to_lower(c), "{c}");
        }
    }

    #[test]
    fn ascii_sets_match_charset() {
        use num_bigint::BigInt;
        let patterns = [
            r"[a-c]x|\w+[^\d,]",
            "(?i)[k\u{3c3}]+|[^\\s]",
            r"(?:[ab]|c(?=[\d.])|(?![x-z])[^q])*+",
            "[\u{100}-\u{10000}a]+\\b",
        ];
        for pattern in patterns {
            let p = crate::pattern::Pattern::compile(&crate::text::cps(pattern), &BigInt::from(0));
            let Ok(p) = p else {
                panic!("{pattern} doesn't compile");
            };
            assert!(!p.sets.maps.is_empty(), "{pattern}");
            for (set, &k) in p.sets.index.iter().enumerate() {
                if k == 0 {
                    continue;
                }
                for c in 0..128 {
                    assert_eq!(
                        p.sets.get(set).map(|m| (m >> c) & 1 != 0),
                        Some(charset(&p.code, set, c)),
                        "{pattern} {set} {c}"
                    );
                }
            }
        }
    }
}
