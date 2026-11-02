//! Lowers the parsed [`Item`] tree into a program for the backtracking VM,
//! resolving flags the way `_compiler._compile` does.

use super::charset::{CharSet, Fold, Matcher};
use super::parser::{
    width_of, AtCode, Item, RepKind, FLAG_ASCII, FLAG_DOTALL, FLAG_IGNORECASE, FLAG_LOCALE,
    FLAG_MULTILINE, FLAG_UNICODE, MAXCODE, MAXREPEAT,
};
use crate::error::RegexError;

/// Assertion kinds after flag resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum At {
    Beginning,
    BeginningLine,
    BeginningString,
    End,
    EndLine,
    EndString,
    Boundary { unicode: bool },
    NonBoundary { unicode: bool },
}

/// `None` repeat bound means unbounded (`MAXREPEAT`).
pub(crate) type Max = Option<usize>;

#[derive(Debug, Clone)]
pub(crate) enum Inst {
    Char(Matcher),
    At(At),
    /// Try `.0` first; on failure resume at `.1`.
    Split(usize, usize),
    Jmp(usize),
    Save(usize),
    Backref {
        group: usize,
        fold: Option<Fold>,
    },
    RepOne {
        m: Matcher,
        min: usize,
        max: Max,
        kind: RepKind,
    },
    RepInit(usize),
    RepLoop {
        reg: usize,
        min: usize,
        max: Max,
        lazy: bool,
        body: usize,
        exit: usize,
    },
    PossLoop {
        reg: usize,
        min: usize,
        max: Max,
        body: usize,
        exit: usize,
    },
    AtomicStart,
    LookStart {
        behind: usize,
        negate: bool,
        cont: usize,
    },
    SubEnd,
    CondGroup {
        group: usize,
        no: usize,
    },
    Match,
}

pub(crate) struct Program {
    pub insts: Vec<Inst>,
    pub nregs: usize,
    /// `(min, max)` of each repeat register, used to normalise counts in
    /// the VM's failure-memo keys.
    pub reg_bounds: Vec<(usize, Max)>,
    /// Whether captured spans can steer matching (backrefs, `(?(n)...)`).
    pub reads_slots: bool,
}

impl Program {
    /// A matcher every match must start with: the first consuming
    /// instruction, reached through captures only, when it is a single
    /// character or a repeat needing at least one. A run from a position
    /// whose character fails it fails at once, so a search may skip it.
    pub(crate) fn first_matcher(&self) -> Option<&Matcher> {
        for inst in &self.insts {
            match inst {
                Inst::Save(_) => {}
                Inst::Char(m) => return Some(m),
                Inst::RepOne { m, min, .. } if *min >= 1 => return Some(m),
                _ => return None,
            }
        }
        None
    }
}

struct Compiler<'a> {
    insts: Vec<Inst>,
    nregs: usize,
    reg_bounds: Vec<(usize, Max)>,
    reads_slots: bool,
    groupwidths: &'a [(u128, u128)],
}

fn fold_for(flags: u32) -> Option<Fold> {
    if flags & FLAG_IGNORECASE == 0 || flags & FLAG_LOCALE != 0 {
        None
    } else if flags & FLAG_UNICODE != 0 {
        Some(Fold::Unicode)
    } else {
        Some(Fold::Ascii)
    }
}

fn combine_flags(flags: u32, add: u32, del: u32) -> u32 {
    let type_flags = FLAG_ASCII | FLAG_LOCALE | FLAG_UNICODE;
    let mut f = flags;
    if add & type_flags != 0 {
        f &= !type_flags;
    }
    (f | add) & !del
}

fn bound(n: u64) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX)
}

/// The single-character matcher for a "simple" repeat body (`_simple`).
fn simple_matcher(body: &[Item], flags: u32) -> Option<Matcher> {
    match body {
        [Item::Subpattern {
            group: None,
            add,
            del,
            body,
        }] => simple_matcher(body, combine_flags(flags, *add, *del)),
        [item] => unit_matcher(item, flags),
        _ => None,
    }
}

fn unit_matcher(item: &Item, flags: u32) -> Option<Matcher> {
    let fold = fold_for(flags);
    let unicode = flags & FLAG_UNICODE != 0;
    Some(match item {
        Item::Literal(c) => Matcher::literal(*c, false, fold),
        Item::NotLiteral(c) => Matcher::literal(*c, true, fold),
        Item::Any => {
            if flags & FLAG_DOTALL != 0 {
                Matcher::AnyAll
            } else {
                Matcher::Any
            }
        }
        Item::In { negate, items } => {
            Matcher::Set(Box::new(CharSet::new(*negate, items, fold, unicode)))
        }
        _ => return None,
    })
}

impl Compiler<'_> {
    fn emit(&mut self, i: Inst) -> usize {
        self.insts.push(i);
        self.insts.len() - 1
    }

    fn here(&self) -> usize {
        self.insts.len()
    }

    fn patch(&mut self, at: usize, target: usize) {
        match self.insts.get_mut(at) {
            Some(Inst::Split(_, y)) => *y = target,
            Some(Inst::Jmp(x)) => *x = target,
            Some(Inst::RepLoop { exit, .. } | Inst::PossLoop { exit, .. }) => *exit = target,
            Some(Inst::LookStart { cont, .. }) => *cont = target,
            Some(Inst::CondGroup { no, .. }) => *no = target,
            _ => {}
        }
    }

    fn seq(&mut self, seq: &[Item], flags: u32) -> Result<(), RegexError> {
        for item in seq {
            self.item(item, flags)?;
        }
        Ok(())
    }

    fn item(&mut self, item: &Item, flags: u32) -> Result<(), RegexError> {
        if let Some(m) = unit_matcher(item, flags) {
            self.emit(Inst::Char(m));
            return Ok(());
        }
        match item {
            Item::Literal(_) | Item::NotLiteral(_) | Item::Any | Item::In { .. } => {}
            Item::At(code) => {
                let multiline = flags & FLAG_MULTILINE != 0;
                let unicode = flags & FLAG_UNICODE != 0;
                let at = match code {
                    AtCode::Beginning if multiline => At::BeginningLine,
                    AtCode::Beginning => At::Beginning,
                    AtCode::End if multiline => At::EndLine,
                    AtCode::End => At::End,
                    AtCode::BeginningString => At::BeginningString,
                    AtCode::EndString => At::EndString,
                    AtCode::Boundary => At::Boundary { unicode },
                    AtCode::NonBoundary => At::NonBoundary { unicode },
                };
                self.emit(Inst::At(at));
            }
            Item::Branch(alts) => {
                let mut jumps = Vec::new();
                let n = alts.len();
                for (i, alt) in alts.iter().enumerate() {
                    if i + 1 < n {
                        let split = self.emit(Inst::Split(0, 0));
                        if let Some(Inst::Split(x, _)) = self.insts.get_mut(split) {
                            *x = split + 1;
                        }
                        self.seq(alt, flags)?;
                        jumps.push(self.emit(Inst::Jmp(0)));
                        let next = self.here();
                        self.patch(split, next);
                    } else {
                        self.seq(alt, flags)?;
                    }
                }
                let end = self.here();
                for j in jumps {
                    self.patch(j, end);
                }
            }
            Item::Subpattern {
                group,
                add,
                del,
                body,
            } => {
                if let Some(g) = group {
                    self.emit(Inst::Save((g - 1) * 2));
                }
                self.seq(body, combine_flags(flags, *add, *del))?;
                if let Some(g) = group {
                    self.emit(Inst::Save((g - 1) * 2 + 1));
                }
            }
            Item::Atomic(body) => {
                self.emit(Inst::AtomicStart);
                self.seq(body, flags)?;
                self.emit(Inst::SubEnd);
            }
            Item::Repeat {
                min,
                max,
                kind,
                body,
            } => {
                let min = bound(*min);
                let max = if *max == MAXREPEAT {
                    None
                } else {
                    Some(bound(*max))
                };
                if let Some(m) = simple_matcher(body, flags) {
                    self.emit(Inst::RepOne {
                        m,
                        min,
                        max,
                        kind: *kind,
                    });
                    return Ok(());
                }
                let reg = self.nregs;
                self.nregs += 1;
                self.reg_bounds.push((min, max));
                self.emit(Inst::RepInit(reg));
                let lp = self.here();
                if *kind == RepKind::Possessive {
                    self.emit(Inst::PossLoop {
                        reg,
                        min,
                        max,
                        body: lp + 1,
                        exit: 0,
                    });
                    self.seq(body, flags)?;
                    self.emit(Inst::SubEnd);
                } else {
                    self.emit(Inst::RepLoop {
                        reg,
                        min,
                        max,
                        lazy: *kind == RepKind::Lazy,
                        body: lp + 1,
                        exit: 0,
                    });
                    self.seq(body, flags)?;
                }
                self.emit(Inst::Jmp(lp));
                let exit = self.here();
                self.patch(lp, exit);
            }
            Item::GroupRef(g) => {
                self.reads_slots = true;
                self.emit(Inst::Backref {
                    group: *g,
                    fold: fold_for(flags),
                });
            }
            Item::GroupRefExists { group, yes, no } => {
                self.reads_slots = true;
                let cond = self.emit(Inst::CondGroup {
                    group: *group,
                    no: 0,
                });
                self.seq(yes, flags)?;
                match no {
                    Some(no) => {
                        let j = self.emit(Inst::Jmp(0));
                        let target = self.here();
                        self.patch(cond, target);
                        self.seq(no, flags)?;
                        let end = self.here();
                        self.patch(j, end);
                    }
                    None => {
                        let end = self.here();
                        self.patch(cond, end);
                    }
                }
            }
            Item::Assert {
                behind,
                negate,
                body,
            } => {
                let mut w = 0;
                if *behind {
                    let gw = self.groupwidths;
                    let (lo, hi) = width_of(body, &|g| gw.get(g).copied().unwrap_or((0, 0)));
                    if lo > MAXCODE {
                        return Err(RegexError::new("looks too much behind", "", None));
                    }
                    if lo != hi {
                        return Err(RegexError::new(
                            "look-behind requires fixed-width pattern",
                            "",
                            None,
                        ));
                    }
                    w = usize::try_from(lo).unwrap_or(usize::MAX);
                }
                let start = self.emit(Inst::LookStart {
                    behind: w,
                    negate: *negate,
                    cont: 0,
                });
                self.seq(body, flags)?;
                self.emit(Inst::SubEnd);
                let end = self.here();
                self.patch(start, end);
            }
        }
        Ok(())
    }
}

/// Compile a parsed pattern. Errors are `re.error`s without a position.
pub(crate) fn compile(
    seq: &[Item],
    flags: u32,
    groupwidths: &[(u128, u128)],
) -> Result<Program, RegexError> {
    let mut c = Compiler {
        insts: Vec::new(),
        nregs: 0,
        reg_bounds: Vec::new(),
        reads_slots: false,
        groupwidths,
    };
    c.seq(seq, flags)?;
    c.emit(Inst::Match);
    Ok(Program {
        insts: c.insts,
        nregs: c.nregs,
        reg_bounds: c.reg_bounds,
        reads_slots: c.reads_slots,
    })
}
