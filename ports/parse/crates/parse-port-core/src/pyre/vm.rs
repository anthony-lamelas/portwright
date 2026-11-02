//! Iterative backtracking matcher. All state lives on heap vectors, so deep
//! inputs never recurse on the Rust stack.

use super::charset::{ascii_lower, is_word, uni_lower, Fold};
use super::compile::{At, Inst, Program};
use super::parser::RepKind;
use std::cell::Cell;
use std::collections::HashMap;

const NONE: usize = usize::MAX;

/// Backtracks in one run before failure memoisation switches on. Cheap
/// matches never pay for it; pathological ones become polynomial.
const MEMO_AFTER: usize = 1 << 12;
/// Upper bound on remembered states, to bound memory.
const MEMO_CAP: usize = 1 << 20;

#[derive(Debug, Clone, Copy)]
enum BarrierKind {
    /// Atomic group or a possessive repeat's mandatory iteration: fail on unwind.
    Atomic,
    /// Optional possessive iteration: on unwind, leave the loop at `cont`.
    PossIter,
    LookPos,
    LookNeg,
}

#[derive(Debug, Clone, Copy)]
enum Bt {
    Alt {
        pc: usize,
        pos: usize,
    },
    RepOne {
        pc: usize,
        start: usize,
        count: usize,
    },
    LazyIter {
        pc: usize,
        pos: usize,
    },
    Slot {
        slot: usize,
        old: usize,
    },
    Reg {
        reg: usize,
        count: usize,
        last: usize,
    },
    Barrier {
        kind: BarrierKind,
        pos: usize,
        cont: usize,
    },
    /// Everything explored from memo state `id` has failed once this is
    /// popped by ordinary backtracking (cuts and look-around unwinds drop it
    /// without recording anything).
    Memo {
        id: usize,
    },
}

/// Options of one match attempt (sre `state` fields).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Mode {
    pub match_all: bool,
    pub must_advance: bool,
}

pub(crate) struct Vm<'p> {
    prog: &'p Program,
    slots: Vec<usize>,
    regs: Vec<(usize, usize)>,
    stack: Vec<Bt>,
    barriers: Vec<usize>,
    backtracks: usize,
    memo_ids: HashMap<Vec<usize>, usize>,
    memo_failed: Vec<bool>,
    scratch: Vec<usize>,
}

fn matched_span(slots: &[usize], group: usize) -> Option<(usize, usize)> {
    let s = *slots.get(group * 2)?;
    let e = *slots.get(group * 2 + 1)?;
    if s == NONE || e == NONE || e < s {
        None
    } else {
        Some((s, e))
    }
}

/// A finished match's vectors, kept per thread so the next match reuses
/// their allocations. Contents are always reset before use.
#[derive(Default)]
struct Spare {
    slots: Vec<usize>,
    regs: Vec<(usize, usize)>,
    stack: Vec<Bt>,
    barriers: Vec<usize>,
}

/// Larger stacks are freed rather than kept (bounds idle memory).
const SPARE_STACK_MAX: usize = 1 << 12;

thread_local! {
    static SPARE: Cell<Option<Spare>> = const { Cell::new(None) };
}

impl<'p> Vm<'p> {
    pub(crate) fn new(prog: &'p Program, ngroups: usize) -> Self {
        let Spare {
            mut slots,
            mut regs,
            mut stack,
            mut barriers,
        } = SPARE
            .try_with(Cell::take)
            .ok()
            .flatten()
            .unwrap_or_default();
        slots.clear();
        slots.resize(ngroups * 2, NONE);
        regs.clear();
        regs.resize(prog.nregs, (0, NONE));
        stack.clear();
        stack.reserve(64);
        barriers.clear();
        Vm {
            prog,
            slots,
            regs,
            stack,
            barriers,
            backtracks: 0,
            memo_ids: HashMap::new(),
            memo_failed: Vec::new(),
            scratch: Vec::new(),
        }
    }

    /// Called when backtracking resumes at `(pc, pos)` with the registers,
    /// slots and barriers already restored. Returns false if this exact
    /// state was fully explored before without a match, so it can be
    /// skipped: the outcome from a state depends only on `pc`, `pos`, the
    /// repeat registers, the barrier stack and (with backrefs or
    /// conditionals) the capture slots. Skipping a failed subtree keeps
    /// sre's match order, so results are unchanged.
    fn memo_admit(&mut self, pc: usize, pos: usize) -> bool {
        self.backtracks = self.backtracks.saturating_add(1);
        if self.backtracks < MEMO_AFTER {
            return true;
        }
        let mut key = std::mem::take(&mut self.scratch);
        key.clear();
        key.push(pc);
        key.push(pos);
        for (i, &(count, last)) in self.regs.iter().enumerate() {
            let count = match self.prog.reg_bounds.get(i) {
                Some(&(min, None)) => count.min(min),
                _ => count,
            };
            key.push(count);
            key.push(last);
        }
        if self.prog.reads_slots {
            key.extend_from_slice(&self.slots);
        }
        for &b in &self.barriers {
            if let Some(Bt::Barrier { kind, pos, cont }) = self.stack.get(b) {
                key.push(*kind as usize);
                key.push(*pos);
                key.push(*cont);
            }
        }
        let id = match self.memo_ids.get(key.as_slice()) {
            Some(&id) => {
                if self.memo_failed.get(id).copied().unwrap_or(false) {
                    self.scratch = key;
                    return false;
                }
                id
            }
            None => {
                if self.memo_ids.len() >= MEMO_CAP {
                    self.scratch = key;
                    return true;
                }
                let id = self.memo_failed.len();
                self.memo_failed.push(false);
                self.memo_ids.insert(key.clone(), id);
                id
            }
        };
        self.scratch = key;
        self.stack.push(Bt::Memo { id });
        true
    }

    fn set_slot(&mut self, slot: usize, v: usize) {
        if let Some(s) = self.slots.get_mut(slot) {
            let old = *s;
            *s = v;
            self.stack.push(Bt::Slot { slot, old });
        }
    }

    fn set_reg(&mut self, reg: usize, count: usize, last: usize) {
        if let Some(r) = self.regs.get_mut(reg) {
            let (oc, ol) = *r;
            *r = (count, last);
            self.stack.push(Bt::Reg {
                reg,
                count: oc,
                last: ol,
            });
        }
    }

    fn reg(&self, reg: usize) -> (usize, usize) {
        self.regs.get(reg).copied().unwrap_or((0, NONE))
    }

    fn push_barrier(&mut self, kind: BarrierKind, pos: usize, cont: usize) {
        self.barriers.push(self.stack.len());
        self.stack.push(Bt::Barrier { kind, pos, cont });
    }

    /// Remove the innermost barrier and the choice points above it, keeping
    /// the undo records so that later backtracking still restores state.
    fn cut(&mut self) -> Option<(BarrierKind, usize, usize)> {
        let b = self.barriers.pop()?;
        let tail: Vec<Bt> = self.stack.drain(b..).collect();
        let mut info = None;
        for e in tail {
            match e {
                Bt::Barrier { kind, pos, cont } if info.is_none() => info = Some((kind, pos, cont)),
                Bt::Slot { .. } | Bt::Reg { .. } => self.stack.push(e),
                _ => {}
            }
        }
        info
    }

    /// Undo everything above the innermost barrier and drop it.
    fn unwind_barrier(&mut self) {
        let Some(b) = self.barriers.pop() else { return };
        while self.stack.len() > b {
            match self.stack.pop() {
                Some(Bt::Slot { slot, old }) => {
                    if let Some(s) = self.slots.get_mut(slot) {
                        *s = old;
                    }
                }
                Some(Bt::Reg { reg, count, last }) => {
                    if let Some(r) = self.regs.get_mut(reg) {
                        *r = (count, last);
                    }
                }
                _ => {}
            }
        }
    }

    fn at(&self, at: At, text: &[char], pos: usize, end: usize) -> bool {
        let prev = || pos.checked_sub(1).and_then(|i| text.get(i)).copied();
        let cur = || {
            if pos < end {
                text.get(pos).copied()
            } else {
                None
            }
        };
        match at {
            At::Beginning | At::BeginningString => pos == 0,
            At::BeginningLine => pos == 0 || prev() == Some('\n'),
            At::End => pos == end || (pos + 1 == end && cur() == Some('\n')),
            // sre reads `ptr[0]` here even when `ptr > end`.
            At::EndLine => pos == end || text.get(pos) == Some(&'\n'),
            At::EndString => pos == end,
            At::Boundary { unicode } | At::NonBoundary { unicode } => {
                // sre tests `state->beginning == state->end`, i.e. endpos == 0.
                if end == 0 {
                    return false;
                }
                let that = pos > 0 && prev().is_some_and(|c| is_word(c, unicode));
                let this = cur().is_some_and(|c| is_word(c, unicode));
                if matches!(at, At::Boundary { .. }) {
                    this != that
                } else {
                    this == that
                }
            }
        }
    }

    /// Try to match at `start`. Returns the group-0 end and leaves the
    /// captures in `self.slots`.
    pub(crate) fn run(
        &mut self,
        text: &[char],
        start: usize,
        end: usize,
        mode: Mode,
    ) -> Option<usize> {
        self.slots.iter_mut().for_each(|s| *s = NONE);
        self.regs.iter_mut().for_each(|r| *r = (0, NONE));
        self.stack.clear();
        self.barriers.clear();
        self.backtracks = 0;
        self.memo_ids.clear();
        self.memo_failed.clear();
        let insts = &self.prog.insts;
        let mut pc = 0usize;
        let mut pos = start;
        loop {
            let ok = match insts.get(pc) {
                None => false,
                Some(Inst::Match) => {
                    if (mode.match_all && pos != end) || (mode.must_advance && pos == start) {
                        false
                    } else {
                        return Some(pos);
                    }
                }
                Some(Inst::Char(m)) => {
                    if pos < end && text.get(pos).is_some_and(|&c| m.matches(c)) {
                        pos += 1;
                        pc += 1;
                        true
                    } else {
                        false
                    }
                }
                Some(Inst::At(at)) => {
                    if self.at(*at, text, pos, end) {
                        pc += 1;
                        true
                    } else {
                        false
                    }
                }
                Some(Inst::Split(x, y)) => {
                    self.stack.push(Bt::Alt { pc: *y, pos });
                    pc = *x;
                    true
                }
                Some(Inst::Jmp(x)) => {
                    pc = *x;
                    true
                }
                Some(Inst::Save(slot)) => {
                    self.set_slot(*slot, pos);
                    pc += 1;
                    true
                }
                Some(Inst::Backref { group, fold }) => {
                    match matched_span(&self.slots, *group - 1) {
                        None => false,
                        Some((s, e)) => {
                            let mut p = pos;
                            let mut ok = true;
                            for i in s..e {
                                let a = text.get(i).copied();
                                let b = if p < end { text.get(p).copied() } else { None };
                                let same = match (a, b, fold) {
                                    (Some(a), Some(b), None) => a == b,
                                    (Some(a), Some(b), Some(Fold::Unicode)) => {
                                        uni_lower(a as u32) == uni_lower(b as u32)
                                    }
                                    (Some(a), Some(b), Some(Fold::Ascii)) => {
                                        ascii_lower(a as u32) == ascii_lower(b as u32)
                                    }
                                    _ => false,
                                };
                                if !same {
                                    ok = false;
                                    break;
                                }
                                p += 1;
                            }
                            if ok {
                                pos = p;
                                pc += 1;
                            }
                            ok
                        }
                    }
                }
                Some(Inst::RepOne { .. }) if pos > end => {
                    // sre: `if (min > end - ptr) RETURN_FAILURE;` (signed)
                    false
                }
                Some(Inst::RepOne { m, min, max, kind }) => {
                    let limit = max.unwrap_or(usize::MAX);
                    match kind {
                        RepKind::Greedy | RepKind::Possessive => {
                            let mut n = 0;
                            while n < limit
                                && pos + n < end
                                && text.get(pos + n).is_some_and(|&c| m.matches(c))
                            {
                                n += 1;
                            }
                            if n < *min {
                                false
                            } else {
                                if n > *min && *kind == RepKind::Greedy {
                                    self.stack.push(Bt::RepOne {
                                        pc,
                                        start: pos,
                                        count: n,
                                    });
                                }
                                pos += n;
                                pc += 1;
                                true
                            }
                        }
                        RepKind::Lazy => {
                            let mut n = 0;
                            while n < *min
                                && pos + n < end
                                && text.get(pos + n).is_some_and(|&c| m.matches(c))
                            {
                                n += 1;
                            }
                            if n < *min {
                                false
                            } else {
                                if n < limit {
                                    self.stack.push(Bt::RepOne {
                                        pc,
                                        start: pos,
                                        count: n,
                                    });
                                }
                                pos += n;
                                pc += 1;
                                true
                            }
                        }
                    }
                }
                Some(Inst::RepInit(reg)) => {
                    self.set_reg(*reg, 0, NONE);
                    pc += 1;
                    true
                }
                Some(Inst::RepLoop {
                    reg,
                    min,
                    max,
                    lazy,
                    body,
                    exit,
                }) => {
                    let (c, last) = self.reg(*reg);
                    if c < *min {
                        self.set_reg(*reg, c + 1, last);
                        pc = *body;
                    } else if *lazy {
                        self.stack.push(Bt::LazyIter { pc, pos });
                        pc = *exit;
                    } else if max.is_none_or(|mx| c < mx) && pos != last {
                        self.stack.push(Bt::Alt { pc: *exit, pos });
                        self.set_reg(*reg, c + 1, pos);
                        pc = *body;
                    } else {
                        pc = *exit;
                    }
                    true
                }
                Some(Inst::PossLoop {
                    reg,
                    min,
                    max,
                    body,
                    exit,
                }) => {
                    let (c, last) = self.reg(*reg);
                    if c < *min {
                        self.set_reg(*reg, c + 1, last);
                        self.push_barrier(BarrierKind::Atomic, pos, 0);
                        pc = *body;
                    } else if max.is_none_or(|mx| c < mx) && pos != last {
                        self.set_reg(*reg, c + 1, pos);
                        self.push_barrier(BarrierKind::PossIter, pos, *exit);
                        pc = *body;
                    } else {
                        pc = *exit;
                    }
                    true
                }
                Some(Inst::AtomicStart) => {
                    self.push_barrier(BarrierKind::Atomic, pos, 0);
                    pc += 1;
                    true
                }
                Some(Inst::LookStart {
                    behind,
                    negate,
                    cont,
                }) => {
                    if pos < *behind {
                        if *negate {
                            pc = *cont;
                            true
                        } else {
                            false
                        }
                    } else {
                        let kind = if *negate {
                            BarrierKind::LookNeg
                        } else {
                            BarrierKind::LookPos
                        };
                        self.push_barrier(kind, pos, *cont);
                        pos -= *behind;
                        pc += 1;
                        true
                    }
                }
                Some(Inst::SubEnd) => {
                    let top = self
                        .barriers
                        .last()
                        .and_then(|&b| self.stack.get(b))
                        .copied();
                    match top {
                        Some(Bt::Barrier {
                            kind: BarrierKind::LookNeg,
                            ..
                        }) => {
                            self.unwind_barrier();
                            false
                        }
                        Some(Bt::Barrier {
                            kind: BarrierKind::LookPos,
                            pos: p,
                            cont,
                        }) => {
                            self.cut();
                            pos = p;
                            pc = cont;
                            true
                        }
                        Some(Bt::Barrier { .. }) => {
                            self.cut();
                            pc += 1;
                            true
                        }
                        _ => false,
                    }
                }
                Some(Inst::CondGroup { group, no }) => {
                    if matched_span(&self.slots, *group - 1).is_some() {
                        pc += 1;
                    } else {
                        pc = *no;
                    }
                    true
                }
            };
            if ok {
                continue;
            }
            // backtrack
            loop {
                self.backtrack(text, end, &mut pc, &mut pos)?;
                if self.memo_admit(pc, pos) {
                    break;
                }
            }
        }
    }

    /// Pop choice points until one resumes; sets `pc`/`pos` to it. `None`
    /// when the stack is exhausted.
    fn backtrack(
        &mut self,
        text: &[char],
        end: usize,
        pc: &mut usize,
        pos: &mut usize,
    ) -> Option<()> {
        let prog = self.prog;
        let insts = &prog.insts;
        loop {
            let e = self.stack.pop()?;
            match e {
                Bt::Memo { id } => {
                    if let Some(f) = self.memo_failed.get_mut(id) {
                        *f = true;
                    }
                }
                Bt::Slot { slot, old } => {
                    if let Some(s) = self.slots.get_mut(slot) {
                        *s = old;
                    }
                }
                Bt::Reg { reg, count, last } => {
                    if let Some(r) = self.regs.get_mut(reg) {
                        *r = (count, last);
                    }
                }
                Bt::Alt { pc: p, pos: q } => {
                    *pc = p;
                    *pos = q;
                    return Some(());
                }
                Bt::RepOne {
                    pc: p,
                    start: s,
                    count,
                } => {
                    let Some(Inst::RepOne { m, min, max, kind }) = insts.get(p) else {
                        continue;
                    };
                    if *kind == RepKind::Lazy {
                        let at = s + count;
                        if max.is_none_or(|mx| count < mx)
                            && at < end
                            && text.get(at).is_some_and(|&c| m.matches(c))
                        {
                            let n = count + 1;
                            if max.is_none_or(|mx| n < mx) {
                                self.stack.push(Bt::RepOne {
                                    pc: p,
                                    start: s,
                                    count: n,
                                });
                            }
                            *pos = s + n;
                            *pc = p + 1;
                            return Some(());
                        }
                    } else {
                        let n = count - 1;
                        if n > *min {
                            self.stack.push(Bt::RepOne {
                                pc: p,
                                start: s,
                                count: n,
                            });
                        }
                        *pos = s + n;
                        *pc = p + 1;
                        return Some(());
                    }
                }
                Bt::LazyIter { pc: p, pos: q } => {
                    let Some(Inst::RepLoop { reg, max, body, .. }) = insts.get(p) else {
                        continue;
                    };
                    let (c, last) = self.reg(*reg);
                    if max.is_some_and(|mx| c >= mx) || q == last {
                        continue;
                    }
                    self.set_reg(*reg, c + 1, q);
                    *pc = *body;
                    *pos = q;
                    return Some(());
                }
                Bt::Barrier { kind, pos: q, cont } => {
                    self.barriers.pop();
                    match kind {
                        BarrierKind::Atomic | BarrierKind::LookPos => {}
                        BarrierKind::PossIter | BarrierKind::LookNeg => {
                            *pos = q;
                            *pc = cont;
                            return Some(());
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn span(&self, group: usize) -> Option<(usize, usize)> {
        matched_span(&self.slots, group)
    }
}

impl Drop for Vm<'_> {
    fn drop(&mut self) {
        if self.stack.capacity() > SPARE_STACK_MAX {
            return;
        }
        let spare = Spare {
            slots: std::mem::take(&mut self.slots),
            regs: std::mem::take(&mut self.regs),
            stack: std::mem::take(&mut self.stack),
            barriers: std::mem::take(&mut self.barriers),
        };
        // Fails only during thread teardown; the vectors are then just freed.
        let _ = SPARE.try_with(|s| s.set(Some(spare)));
    }
}
