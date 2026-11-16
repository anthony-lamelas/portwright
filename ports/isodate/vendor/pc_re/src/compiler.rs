//! Port of CPython 3.12 `re._compiler`: the parse tree to sre code words.

use crate::casefix;
use crate::error::{Error, ReError, Result};
use crate::flags;
use crate::frames::frame;
use crate::parser::{getwidth, getwidth_depth, Node, SetItem, SubPattern};
use crate::sre::{self, op, INFO_CHARSET, INFO_LITERAL, INFO_PREFIX, MAXCODE};
use crate::text::cps;
use crate::unicode;

const TYPE_FLAGS: u32 = flags::ASCII | flags::LOCALE | flags::UNICODE;

/// Group widths from the parser, needed for `GROUPREF` widths.
type Widths<'a> = &'a [Option<(u128, u128)>];

/// `_combine_flags`.
fn combine_flags(mut f: u32, add_flags: u32, del_flags: u32) -> u32 {
    if add_flags & TYPE_FLAGS != 0 {
        f &= !TYPE_FLAGS;
    }
    (f | add_flags) & !del_flags
}

/// Case handling in effect (`iscased`, `tolower` and `fixes` of `_compile`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fold {
    /// `IGNORECASE` with `ASCII`.
    Ascii,
    /// `IGNORECASE` with `UNICODE` (uses `_EXTRA_CASES`).
    Unicode,
}

impl Fold {
    fn of(f: u32) -> Option<Self> {
        if f & flags::IGNORECASE != 0 && f & flags::LOCALE == 0 {
            Some(if f & flags::UNICODE != 0 {
                Fold::Unicode
            } else {
                Fold::Ascii
            })
        } else {
            None
        }
    }

    fn iscased(self, c: u32) -> bool {
        match self {
            Fold::Ascii => unicode::ascii_is_cased(c),
            Fold::Unicode => unicode::is_cased(c),
        }
    }

    fn lower(self, c: u32) -> u32 {
        match self {
            Fold::Ascii => unicode::ascii_lower(c),
            Fold::Unicode => unicode::to_lower(c),
        }
    }

    fn fixes(self, c: u32) -> Option<&'static [u32]> {
        match self {
            Fold::Ascii => None,
            Fold::Unicode => casefix::extra_cases(c),
        }
    }
}

/// `_get_iscased(flags)`.
fn get_iscased(f: u32) -> Option<fn(u32) -> bool> {
    if f & flags::IGNORECASE == 0 {
        None
    } else if f & flags::UNICODE != 0 {
        Some(unicode::is_cased)
    } else {
        Some(unicode::ascii_is_cased)
    }
}

fn internal(msg: &str) -> Error {
    ReError::bare(cps(msg)).into()
}

/// Patches the skip word at `skip` to the distance to the current end.
fn patch(code: &mut [u32], skip: usize, extra: usize) {
    let value = code.len() - skip + extra;
    if let Some(slot) = code.get_mut(skip) {
        *slot = u32::try_from(value).unwrap_or(u32::MAX);
    }
}

fn emit_usize(code: &mut Vec<u32>, v: usize) {
    code.push(u32::try_from(v).unwrap_or(u32::MAX));
}

/// `_compile(code, pattern, flags)`.
#[expect(
    clippy::too_many_lines,
    reason = "mirrors _compiler._compile, one branch per opcode"
)]
///
/// `depth` is the Python frame depth of this `_compile` call (see
/// [`crate::frames`]). Nested calls get a `SubPattern`, which the loop
/// iterates through `SubPattern.__getitem__`, one frame deeper.
fn compile_into(
    code: &mut Vec<u32>,
    pattern: &[Node],
    f: u32,
    widths: Widths<'_>,
    depth: usize,
) -> Result<()> {
    if depth > 0 {
        frame(depth + 1)?;
    }
    let fold = Fold::of(f);
    let inner = depth + 1;
    for node in pattern {
        match node {
            Node::Literal(av) | Node::NotLiteral(av) => {
                let not = matches!(node, Node::NotLiteral(_));
                let av = *av;
                let base = if not { op::NOT_LITERAL } else { op::LITERAL };
                if f & flags::IGNORECASE == 0 {
                    code.extend([base, av]);
                } else if f & flags::LOCALE != 0 {
                    let o = if not {
                        op::NOT_LITERAL_LOC_IGNORE
                    } else {
                        op::LITERAL_LOC_IGNORE
                    };
                    code.extend([o, av]);
                } else if let Some(fold) = fold.filter(|fd| fd.iscased(av)) {
                    let lo = fold.lower(av);
                    match fold {
                        Fold::Ascii => {
                            let o = if not {
                                op::NOT_LITERAL_IGNORE
                            } else {
                                op::LITERAL_IGNORE
                            };
                            code.extend([o, lo]);
                        }
                        Fold::Unicode => match fold.fixes(lo) {
                            None => {
                                let o = if not {
                                    op::NOT_LITERAL_UNI_IGNORE
                                } else {
                                    op::LITERAL_UNI_IGNORE
                                };
                                code.extend([o, lo]);
                            }
                            Some(extra) => {
                                code.push(op::IN_UNI_IGNORE);
                                let skip = code.len();
                                code.push(0);
                                if not {
                                    code.push(op::NEGATE);
                                }
                                for &k in std::iter::once(&lo).chain(extra) {
                                    code.extend([op::LITERAL, k]);
                                }
                                code.push(op::FAILURE);
                                patch(code, skip, 0);
                            }
                        },
                    }
                } else {
                    code.extend([base, av]);
                }
            }
            Node::In(av) => {
                let (charset, hascased) = optimize_charset(av, fold);
                // `_optimize_charset` calls `_mk_bitmap` / `_bytes_to_codes`
                // when it builds a bitmap.
                let bitmap = charset
                    .iter()
                    .any(|i| matches!(i, SetItem::Charset(_) | SetItem::BigCharset(_)));
                frame(if bitmap { inner + 1 } else { inner })?;
                let o = if f & flags::IGNORECASE != 0 && f & flags::LOCALE != 0 {
                    op::IN_LOC_IGNORE
                } else if !hascased {
                    op::IN
                } else if fold == Some(Fold::Unicode) {
                    op::IN_UNI_IGNORE
                } else {
                    op::IN_IGNORE
                };
                code.push(o);
                let skip = code.len();
                code.push(0);
                compile_charset(&charset, f, code);
                patch(code, skip, 0);
            }
            Node::Any => code.push(if f & flags::DOTALL != 0 {
                op::ANY_ALL
            } else {
                op::ANY
            }),
            Node::Repeat {
                kind,
                min,
                max,
                item,
            } => {
                if f & flags::TEMPLATE != 0 {
                    frame(inner)?;
                    return Err(internal(&format!(
                        "internal: unsupported template operator {}",
                        sre::name(&sre::OPCODE_NAMES, *kind)
                    )));
                }
                let (multi, until, one) = match *kind {
                    op::MIN_REPEAT => (op::REPEAT, op::MIN_UNTIL, op::MIN_REPEAT_ONE),
                    op::POSSESSIVE_REPEAT => (
                        op::POSSESSIVE_REPEAT,
                        op::SUCCESS,
                        op::POSSESSIVE_REPEAT_ONE,
                    ),
                    _ => (op::REPEAT, op::MAX_UNTIL, op::REPEAT_ONE),
                };
                frame(simple_depth(item, inner))?;
                if simple(item) {
                    code.push(one);
                    let skip = code.len();
                    code.extend([0, *min, *max]);
                    compile_into(code, item, f, widths, inner)?;
                    code.push(op::SUCCESS);
                    patch(code, skip, 0);
                } else {
                    code.push(multi);
                    let skip = code.len();
                    code.extend([0, *min, *max]);
                    compile_into(code, item, f, widths, inner)?;
                    patch(code, skip, 0);
                    code.push(until);
                }
            }
            Node::Subpattern {
                group,
                add_flags,
                del_flags,
                p,
            } => {
                if let Some(g) = group {
                    code.push(op::MARK);
                    emit_usize(code, (g - 1) * 2);
                }
                frame(inner)?; // `_combine_flags`
                compile_into(
                    code,
                    p,
                    combine_flags(f, *add_flags, *del_flags),
                    widths,
                    inner,
                )?;
                if let Some(g) = group {
                    code.push(op::MARK);
                    emit_usize(code, (g - 1) * 2 + 1);
                }
            }
            Node::AtomicGroup(p) => {
                code.push(op::ATOMIC_GROUP);
                let skip = code.len();
                code.push(0);
                compile_into(code, p, f, widths, inner)?;
                code.push(op::SUCCESS);
                patch(code, skip, 0);
            }
            Node::Assert { negate, dir, p } => {
                code.push(if *negate { op::ASSERT_NOT } else { op::ASSERT });
                let skip = code.len();
                code.push(0);
                if *dir >= 0 {
                    code.push(0);
                } else {
                    frame(getwidth_depth(p, inner))?;
                    let (lo, hi) = getwidth(p, widths);
                    if lo != hi || lo > u128::from(MAXCODE) {
                        frame(inner)?; // `error.__init__`
                    }
                    if lo > u128::from(MAXCODE) {
                        return Err(internal("looks too much behind"));
                    }
                    if lo != hi {
                        return Err(internal("look-behind requires fixed-width pattern"));
                    }
                    code.push(u32::try_from(lo).unwrap_or(MAXCODE));
                }
                compile_into(code, p, f, widths, inner)?;
                code.push(op::SUCCESS);
                patch(code, skip, 0);
            }
            Node::At(av) => {
                code.push(op::AT);
                let mut av = *av;
                if f & flags::MULTILINE != 0 {
                    av = match av {
                        sre::at::BEGINNING => sre::at::BEGINNING_LINE,
                        sre::at::END => sre::at::END_LINE,
                        other => other,
                    };
                }
                if f & flags::LOCALE != 0 {
                    av = match av {
                        sre::at::BOUNDARY => sre::at::LOC_BOUNDARY,
                        sre::at::NON_BOUNDARY => sre::at::LOC_NON_BOUNDARY,
                        other => other,
                    };
                } else if f & flags::UNICODE != 0 {
                    av = match av {
                        sre::at::BOUNDARY => sre::at::UNI_BOUNDARY,
                        sre::at::NON_BOUNDARY => sre::at::UNI_NON_BOUNDARY,
                        other => other,
                    };
                }
                code.push(av);
            }
            Node::Branch(items) => {
                code.push(op::BRANCH);
                let mut tails = Vec::with_capacity(items.len());
                for item in items {
                    let skip = code.len();
                    code.push(0);
                    compile_into(code, item, f, widths, inner)?;
                    code.push(op::JUMP);
                    tails.push(code.len());
                    code.push(0);
                    patch(code, skip, 0);
                }
                code.push(op::FAILURE);
                for tail in tails {
                    patch(code, tail, 0);
                }
            }
            Node::GroupRef(g) => {
                code.push(if f & flags::IGNORECASE == 0 {
                    op::GROUPREF
                } else if f & flags::LOCALE != 0 {
                    op::GROUPREF_LOC_IGNORE
                } else if fold == Some(Fold::Unicode) {
                    op::GROUPREF_UNI_IGNORE
                } else {
                    op::GROUPREF_IGNORE
                });
                emit_usize(code, g - 1);
            }
            Node::GroupRefExists { group, yes, no } => {
                code.push(op::GROUPREF_EXISTS);
                emit_usize(code, group - 1);
                let skipyes = code.len();
                code.push(0);
                compile_into(code, yes, f, widths, inner)?;
                if no.is_some() {
                    frame(inner)?; // `if av[2]:` calls `SubPattern.__len__`
                }
                match no {
                    Some(no) if !no.is_empty() => {
                        code.push(op::JUMP);
                        let skipno = code.len();
                        code.push(0);
                        patch(code, skipyes, 1);
                        compile_into(code, no, f, widths, inner)?;
                        patch(code, skipno, 0);
                    }
                    _ => patch(code, skipyes, 1),
                }
            }
        }
    }
    Ok(())
}

/// `_compile_charset`.
fn compile_charset(charset: &[SetItem], f: u32, code: &mut Vec<u32>) {
    for item in charset {
        match item {
            SetItem::Negate => code.push(op::NEGATE),
            SetItem::Literal(c) => code.extend([op::LITERAL, *c]),
            SetItem::Range(lo, hi) => code.extend([op::RANGE, *lo, *hi]),
            SetItem::RangeUniIgnore(lo, hi) => code.extend([op::RANGE_UNI_IGNORE, *lo, *hi]),
            SetItem::Charset(words) => {
                code.push(op::CHARSET);
                code.extend_from_slice(words);
            }
            SetItem::BigCharset(words) => {
                code.push(op::BIGCHARSET);
                code.extend_from_slice(words);
            }
            SetItem::Category(c) => {
                code.push(op::CATEGORY);
                code.push(if f & flags::LOCALE != 0 {
                    sre::ch_locale(*c)
                } else if f & flags::UNICODE != 0 {
                    sre::ch_unicode(*c)
                } else {
                    *c
                });
            }
        }
    }
    code.push(op::FAILURE);
}

/// `charmap[i] = 1`, or `None` where Python raises `IndexError` (past the end).
fn mark(charmap: &mut [u8], i: u32) -> Option<()> {
    *usize::try_from(i).ok().and_then(|i| charmap.get_mut(i))? = 1;
    Some(())
}

/// `_optimize_charset(charset, iscased, fixup, fixes)`.
fn optimize_charset(charset: &[SetItem], fold: Option<Fold>) -> (Vec<SetItem>, bool) {
    let mut out: Vec<SetItem> = Vec::new();
    let mut tail: Vec<SetItem> = Vec::new();
    let mut charmap = vec![0u8; 256];
    let mut hascased = false;
    for item in charset {
        let mut item = item.clone();
        loop {
            let attempt = match &mut item {
                SetItem::Literal(av) => match fold {
                    Some(fd) => {
                        *av = fd.lower(*av);
                        let av = *av;
                        let r = mark(&mut charmap, av).and_then(|()| {
                            fd.fixes(av)
                                .unwrap_or(&[])
                                .iter()
                                .try_for_each(|&k| mark(&mut charmap, k))
                        });
                        if r.is_some() && !hascased && fd.iscased(av) {
                            hascased = true;
                        }
                        r
                    }
                    None => mark(&mut charmap, *av),
                },
                SetItem::Range(a, b) => {
                    let (a, b) = (*a, *b);
                    match fold {
                        Some(fd) => {
                            let r = (a..=b).try_for_each(|c| {
                                let i = fd.lower(c);
                                mark(&mut charmap, i)?;
                                fd.fixes(i)
                                    .unwrap_or(&[])
                                    .iter()
                                    .try_for_each(|&k| mark(&mut charmap, k))
                            });
                            if r.is_some() && !hascased {
                                hascased = (a..=b).any(|c| fd.iscased(c));
                            }
                            r
                        }
                        None => (a..=b).try_for_each(|c| mark(&mut charmap, c)),
                    }
                }
                SetItem::Negate => {
                    out.push(item.clone());
                    Some(())
                }
                _ => {
                    tail.push(item.clone());
                    Some(())
                }
            };
            if attempt.is_none() {
                if charmap.len() == 256 {
                    charmap.resize(65536, 0);
                    continue;
                }
                if let Some(fd) = fold {
                    match item {
                        SetItem::Range(a, b) => {
                            if fd == Fold::Unicode {
                                item = SetItem::RangeUniIgnore(a, b);
                            }
                            hascased = true;
                        }
                        SetItem::Literal(av) if !hascased && fd.iscased(av) => {
                            hascased = true;
                        }
                        _ => {}
                    }
                }
                tail.push(item);
            }
            break;
        }
    }

    // compress character map
    let find = |map: &[u8], v: u8, from: usize| -> Option<usize> {
        map.get(from..)
            .and_then(|s| s.iter().position(|&x| x == v))
            .map(|i| i + from)
    };
    let mut runs: Option<Vec<(usize, usize)>> = Some(Vec::new());
    let mut q = 0;
    while let Some(p) = find(&charmap, 1, q) {
        let Some(r) = runs.as_mut() else { break };
        if r.len() >= 2 {
            runs = None;
            break;
        }
        match find(&charmap, 0, p) {
            None => {
                r.push((p, charmap.len()));
                break;
            }
            Some(next) => {
                q = next;
                r.push((p, q));
            }
        }
    }
    if let Some(runs) = runs {
        for (p, q) in runs {
            let (pp, qq) = (u32::try_from(p).unwrap_or(0), u32::try_from(q).unwrap_or(0));
            if q - p == 1 {
                out.push(SetItem::Literal(pp));
            } else {
                out.push(SetItem::Range(pp, qq - 1));
            }
        }
        out.extend(tail);
        if hascased || out.len() < charset.len() {
            return (out, hascased);
        }
        return (charset.to_vec(), hascased);
    }

    if charmap.len() == 256 {
        out.push(SetItem::Charset(mk_bitmap(&charmap)));
        out.extend(tail);
        return (out, hascased);
    }

    // BIGCHARSET: unique 256-entry chunks, a 256-byte chunk map, and the chunks.
    let mut comps: Vec<&[u8]> = Vec::new();
    let mut mapping = [0u8; 256];
    let mut data: Vec<u8> = Vec::new();
    for (i, chunk) in charmap.chunks(256).enumerate() {
        let index = match comps.iter().position(|c| *c == chunk) {
            Some(k) => k,
            None => {
                comps.push(chunk);
                data.extend_from_slice(chunk);
                comps.len() - 1
            }
        };
        if let Some(slot) = mapping.get_mut(i) {
            *slot = u8::try_from(index).unwrap_or(u8::MAX);
        }
    }
    let mut words = vec![u32::try_from(comps.len()).unwrap_or(0)];
    words.extend(mapping.chunks(4).map(|b| {
        b.iter()
            .rev()
            .fold(0u32, |acc, &x| (acc << 8) | u32::from(x))
    }));
    words.extend(mk_bitmap(&data));
    out.push(SetItem::BigCharset(words));
    out.extend(tail);
    (out, hascased)
}

/// `_mk_bitmap`: 32 flags per word, least significant bit first.
fn mk_bitmap(bits: &[u8]) -> Vec<u32> {
    bits.chunks(32)
        .map(|chunk| {
            chunk.iter().enumerate().fold(
                0u32,
                |acc, (j, &b)| if b != 0 { acc | (1 << j) } else { acc },
            )
        })
        .collect()
}

/// Deepest frame `_simple(p)` reaches when it runs at depth `depth`: it
/// calls `SubPattern.__len__` and `__getitem__`, and recurses into
/// non-capturing groups.
fn simple_depth(p: &[Node], depth: usize) -> usize {
    match p {
        [Node::Subpattern { group: None, p, .. }] => simple_depth(p, depth + 1),
        _ => depth + 1,
    }
}

/// `_simple(p)`.
fn simple(p: &[Node]) -> bool {
    match p {
        [Node::Subpattern { group: None, p, .. }] => simple(p),
        [Node::Literal(_) | Node::NotLiteral(_) | Node::Any | Node::In(_)] => true,
        _ => false,
    }
}

/// `_generate_overlap_table(prefix)`.
fn overlap_table(prefix: &[u32]) -> Vec<u32> {
    let mut table = vec![0usize; prefix.len()];
    for i in 1..prefix.len() {
        let mut idx = table.get(i - 1).copied().unwrap_or(0);
        loop {
            if prefix.get(i) == prefix.get(idx) {
                if let Some(t) = table.get_mut(i) {
                    *t = idx + 1;
                }
                break;
            }
            if idx == 0 {
                if let Some(t) = table.get_mut(i) {
                    *t = 0;
                }
                break;
            }
            idx = table.get(idx - 1).copied().unwrap_or(0);
        }
    }
    table
        .into_iter()
        .map(|v| u32::try_from(v).unwrap_or(u32::MAX))
        .collect()
}

/// `_get_literal_prefix`: (prefix, prefix_skip, got_all).
///
/// `depth` is this call's Python frame depth; `deepest` records the deepest
/// frame it reaches (`_get_iscased`, `_combine_flags`, recursion).
fn literal_prefix(
    pattern: &[Node],
    f: u32,
    depth: usize,
    deepest: &mut usize,
) -> (Vec<u32>, Option<usize>, bool) {
    *deepest = (*deepest).max(depth + 1);
    let mut prefix = Vec::new();
    let mut prefix_skip = None;
    let iscased = get_iscased(f);
    for node in pattern {
        match node {
            Node::Literal(av) => {
                if iscased.is_some_and(|ic| ic(*av)) {
                    return (prefix, prefix_skip, false);
                }
                prefix.push(*av);
            }
            Node::Subpattern {
                group,
                add_flags,
                del_flags,
                p,
            } => {
                let flags1 = combine_flags(f, *add_flags, *del_flags);
                if flags1 & flags::IGNORECASE != 0 && flags1 & flags::LOCALE != 0 {
                    return (prefix, prefix_skip, false);
                }
                let (prefix1, skip1, got_all) = literal_prefix(p, flags1, depth + 1, deepest);
                if prefix_skip.is_none() {
                    if group.is_some() {
                        prefix_skip = Some(prefix.len());
                    } else if let Some(s1) = skip1 {
                        prefix_skip = Some(prefix.len() + s1);
                    }
                }
                prefix.extend(prefix1);
                if !got_all {
                    return (prefix, prefix_skip, false);
                }
            }
            _ => return (prefix, prefix_skip, false),
        }
    }
    (prefix, prefix_skip, true)
}

/// `_get_charset_prefix`.
fn charset_prefix(pattern: &[Node], f: u32) -> Option<Vec<SetItem>> {
    let mut pattern = pattern;
    let mut f = f;
    let first = loop {
        let node = pattern.first()?;
        let Node::Subpattern {
            add_flags,
            del_flags,
            p,
            ..
        } = node
        else {
            break node;
        };
        f = combine_flags(f, *add_flags, *del_flags);
        pattern = p;
        if f & flags::IGNORECASE != 0 && f & flags::LOCALE != 0 {
            return None;
        }
    };
    let iscased = get_iscased(f);
    let cased = |c: u32| iscased.is_some_and(|ic| ic(c));
    match first {
        Node::Literal(av) => {
            if cased(*av) {
                None
            } else {
                Some(vec![SetItem::Literal(*av)])
            }
        }
        Node::Branch(items) => {
            let mut charset = Vec::new();
            for p in items {
                match p.first()? {
                    Node::Literal(av) if !cased(*av) => charset.push(SetItem::Literal(*av)),
                    _ => return None,
                }
            }
            Some(charset)
        }
        Node::In(av) => {
            if let Some(ic) = iscased {
                for item in av {
                    match item {
                        SetItem::Literal(c) if ic(*c) => return None,
                        SetItem::Range(lo, hi) if *hi > 0xFFFF || (*lo..=*hi).any(ic) => {
                            return None;
                        }
                        _ => {}
                    }
                }
            }
            Some(av.clone())
        }
        _ => None,
    }
}

/// `_compile_info`.
///
/// Runs at depth 0 (see [`crate::frames`]).
fn compile_info(
    code: &mut Vec<u32>,
    pattern: &SubPattern,
    f: u32,
    widths: Widths<'_>,
) -> Result<()> {
    frame(getwidth_depth(pattern, 1))?;
    let (lo, hi) = getwidth(pattern, widths);
    let hi = u32::try_from(hi.min(u128::from(MAXCODE))).unwrap_or(MAXCODE);
    if lo == 0 {
        code.extend([op::INFO, 4, 0, 0, hi]);
        return Ok(());
    }
    let mut prefix = Vec::new();
    let mut prefix_skip = None;
    let mut got_all = false;
    let mut charset = None;
    if !(f & flags::IGNORECASE != 0 && f & flags::LOCALE != 0) {
        let mut deepest = 0;
        (prefix, prefix_skip, got_all) = literal_prefix(pattern, f, 1, &mut deepest);
        frame(deepest)?;
        if prefix.is_empty() {
            charset = charset_prefix(pattern, f);
        }
    }
    code.push(op::INFO);
    let skip = code.len();
    code.push(0);
    let mut mask = 0;
    if !prefix.is_empty() {
        mask = INFO_PREFIX;
        if prefix_skip.is_none() && got_all {
            mask |= INFO_LITERAL;
        }
    } else if charset.as_ref().is_some_and(|c| !c.is_empty()) {
        mask |= INFO_CHARSET;
    }
    code.push(mask);
    if lo < u128::from(MAXCODE) {
        code.push(u32::try_from(lo).unwrap_or(MAXCODE));
    } else {
        code.push(MAXCODE);
        prefix.truncate(MAXCODE as usize);
    }
    code.push(hi);
    if !prefix.is_empty() {
        emit_usize(code, prefix.len());
        emit_usize(code, prefix_skip.unwrap_or(prefix.len()));
        code.extend_from_slice(&prefix);
        code.extend(overlap_table(&prefix));
    } else if let Some(cs) = charset.filter(|c| !c.is_empty()) {
        let (cs, _) = optimize_charset(&cs, None);
        compile_charset(&cs, f, code);
    }
    patch(code, skip, 0);
    Ok(())
}

/// `_code(p, flags)`: the complete program for a parsed pattern, where `f`
/// holds the low bits of `p.state.flags | flags`.
///
/// # Errors
/// `re.error` for look-behinds of variable width and for repeats under
/// `TEMPLATE`.
pub fn code(pattern: &SubPattern, f: u32, widths: Widths<'_>) -> Result<Vec<u32>> {
    let mut code = Vec::new();
    compile_info(&mut code, pattern, f, widths)?;
    compile_into(&mut code, pattern, f, widths, 0)?;
    code.push(op::SUCCESS);
    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_pattern;

    fn compile(p: &str, f: u32) -> Vec<u32> {
        let parsed = parse_pattern(&cps(p), f).ok();
        parsed
            .and_then(|p| code(&p.data, p.state.flags, &p.state.groupwidths).ok())
            .unwrap_or_default()
    }

    #[test]
    fn simple_codes() {
        // re._compiler._code(re._parser.parse('a'), 0)
        assert_eq!(
            compile("a", 0),
            vec![14, 8, 3, 1, 1, 1, 1, 97, 0, 16, 97, 1]
        );
        assert_eq!(compile("(?i)a", 0), vec![14, 4, 0, 1, 1, 40, 97, 1]);
    }
}
