//! `str` splitting, stripping, padding and tab expansion with CPython 3.12 semantics.
//!
//! Argument checks follow the order CPython's Argument Clinic wrappers use: the
//! `self` descriptor check, then each converted argument in order, then checks in
//! the method body.

use num_bigint::BigInt;
use num_traits::ToPrimitive;

use crate::args::self_text;
use crate::casing::is_whitespace;
use crate::error::{Error, Result};
use crate::value::{Text, Value};

/// `PY_SSIZE_T_MAX` on 64-bit platforms.
const SSIZE_MAX: i64 = i64::MAX;

/// Largest length `PyUnicode_New` accepts for 4-byte strings before `MemoryError`.
/// Any longer result is rejected up front instead of attempting the allocation.
const MAX_TEXT_LEN: i64 = (SSIZE_MAX - 56) / 4 - 1;

/// `operator.index(v)` as a `BigInt` (CPython `_PyNumber_Index`).
fn index_of(v: &Value) -> Result<BigInt> {
    match v {
        Value::Int(i) => Ok(i.clone()),
        Value::Bool(b) => Ok(BigInt::from(u8::from(*b))),
        other => Err(Error::type_error(format!(
            "'{}' object cannot be interpreted as an integer",
            other.type_name()
        ))),
    }
}

/// Argument Clinic's `Py_ssize_t` converter.
fn ssize_arg(v: &Value) -> Result<i64> {
    index_of(v)?
        .to_i64()
        .ok_or_else(|| Error::overflow_error("Python int too large to convert to C ssize_t"))
}

/// Argument Clinic's `int` converter.
fn c_int_arg(v: &Value) -> Result<i32> {
    index_of(v)?
        .to_i32()
        .ok_or_else(|| Error::overflow_error("Python int too large to convert to C int"))
}

/// Argument Clinic's `Py_UCS4` converter used for `fillchar`.
fn fill_arg(v: Option<&Value>) -> Result<u32> {
    match v {
        None => Ok(u32::from(b' ')),
        Some(Value::Str(t)) => match t.as_slice() {
            [c] => Ok(*c),
            _ => Err(Error::type_error(
                "The fill character must be exactly one character long",
            )),
        },
        Some(other) => Err(Error::type_error(format!(
            "The fill character must be a unicode character, not {}",
            other.type_name()
        ))),
    }
}

/// Allocates room for a result of `len` code points, failing like `PyUnicode_New`.
fn new_text(len: i64) -> Result<Text> {
    if len > MAX_TEXT_LEN {
        return Err(Error::memory_error());
    }
    let len = usize::try_from(len).map_err(|_| Error::memory_error())?;
    let mut out = Text::new();
    out.try_reserve_exact(len)
        .map_err(|_| Error::memory_error())?;
    Ok(out)
}

/// Reads `sep` for split/rsplit: `None` means whitespace splitting.
fn sep_arg(sep: Option<&Value>) -> Result<Option<&[u32]>> {
    match sep {
        None | Some(Value::None) => Ok(None),
        Some(Value::Str(t)) if t.is_empty() => Err(Error::value_error("empty separator")),
        Some(Value::Str(t)) => Ok(Some(t)),
        Some(other) => Err(Error::type_error(format!(
            "must be str or None, not {}",
            other.type_name()
        ))),
    }
}

/// Checked split/rsplit arguments.
struct SplitArgs<'a> {
    text: &'a [u32],
    /// `None` splits on whitespace runs.
    sep: Option<&'a [u32]>,
    /// Maximum number of splits.
    max: usize,
}

/// Shared prologue of split/rsplit.
fn split_args<'a>(
    name: &str,
    s: &'a Value,
    sep: Option<&'a Value>,
    maxsplit: Option<&Value>,
) -> Result<SplitArgs<'a>> {
    let s = self_text(name, s)?;
    let maxsplit = maxsplit.map(ssize_arg).transpose()?.unwrap_or(-1);
    let sep = sep_arg(sep)?;
    // Negative means unlimited (CPython uses PY_SSIZE_T_MAX).
    let max = usize::try_from(maxsplit).unwrap_or(usize::MAX);
    Ok(SplitArgs { text: s, sep, max })
}

/// KMP failure table: `fail[i]` is the length of the longest proper border of `needle[..=i]`.
fn failure(needle: &[u32]) -> Vec<usize> {
    let mut fail = vec![0; needle.len()];
    let mut k = 0;
    for i in 1..needle.len() {
        while k > 0 && needle[i] != needle[k] {
            k = fail[k - 1];
        }
        if needle[i] == needle[k] {
            k += 1;
        }
        fail[i] = k;
    }
    fail
}

/// Linear-time (KMP) search for the first occurrence of the non-empty `needle`
/// in `hay`, given its [`failure`] table. Returns the number of items consumed
/// up to the end of the match.
fn match_end(hay: impl Iterator<Item = u32>, needle: &[u32], fail: &[usize]) -> Option<usize> {
    let mut k = 0;
    for (i, c) in hay.enumerate() {
        while k > 0 && c != needle[k] {
            k = fail[k - 1];
        }
        if c == needle[k] {
            k += 1;
        }
        if k == needle.len() {
            return Some(i + 1);
        }
    }
    None
}

/// `str.split(s, sep, maxsplit)`.
///
/// # Errors
/// `TypeError`/`OverflowError`/`ValueError` for bad arguments, as CPython raises them.
pub fn split(s: &Value, sep: Option<&Value>, maxsplit: Option<&Value>) -> Result<Vec<Text>> {
    Ok(split_slices(s, sep, maxsplit)?
        .into_iter()
        .map(<[u32]>::to_vec)
        .collect())
}

/// [`split`] returning the pieces as slices of `s` (no copies).
///
/// # Errors
/// As [`split`].
pub fn split_slices<'a>(
    s: &'a Value,
    sep: Option<&'a Value>,
    maxsplit: Option<&Value>,
) -> Result<Vec<&'a [u32]>> {
    let SplitArgs { text: s, sep, max } = split_args("split", s, sep, maxsplit)?;
    let mut out = Vec::new();
    match sep {
        None => {
            let n = s.len();
            let mut i = 0;
            let mut count = max;
            while count > 0 {
                count -= 1;
                while i < n && is_whitespace(s[i]) {
                    i += 1;
                }
                if i == n {
                    break;
                }
                let j = i;
                i += 1;
                while i < n && !is_whitespace(s[i]) {
                    i += 1;
                }
                out.push(&s[j..i]);
            }
            while i < n && is_whitespace(s[i]) {
                i += 1;
            }
            if i < n {
                out.push(&s[i..]);
            }
        }
        Some(sep) => {
            let fail = failure(sep);
            let mut start = 0;
            let mut count = max;
            while count > 0 {
                let Some(len) = match_end(s[start..].iter().copied(), sep, &fail) else {
                    break;
                };
                count -= 1;
                out.push(&s[start..start + len - sep.len()]);
                start += len;
            }
            out.push(&s[start..]);
        }
    }
    Ok(out)
}

/// `str.rsplit(s, sep, maxsplit)`.
///
/// # Errors
/// `TypeError`/`OverflowError`/`ValueError` for bad arguments, as CPython raises them.
pub fn rsplit(s: &Value, sep: Option<&Value>, maxsplit: Option<&Value>) -> Result<Vec<Text>> {
    Ok(rsplit_slices(s, sep, maxsplit)?
        .into_iter()
        .map(<[u32]>::to_vec)
        .collect())
}

/// [`rsplit`] returning the pieces as slices of `s` (no copies).
///
/// # Errors
/// As [`rsplit`].
pub fn rsplit_slices<'a>(
    s: &'a Value,
    sep: Option<&'a Value>,
    maxsplit: Option<&Value>,
) -> Result<Vec<&'a [u32]>> {
    let SplitArgs { text: s, sep, max } = split_args("rsplit", s, sep, maxsplit)?;
    let mut out = Vec::new();
    match sep {
        None => {
            // `i` is one past the next unscanned position from the right.
            let mut i = s.len();
            let mut count = max;
            while count > 0 {
                count -= 1;
                while i > 0 && is_whitespace(s[i - 1]) {
                    i -= 1;
                }
                if i == 0 {
                    break;
                }
                let end = i;
                i -= 1;
                while i > 0 && !is_whitespace(s[i - 1]) {
                    i -= 1;
                }
                out.push(&s[i..end]);
            }
            while i > 0 && is_whitespace(s[i - 1]) {
                i -= 1;
            }
            if i > 0 {
                out.push(&s[..i]);
            }
        }
        Some(sep) => {
            // Search the reversed text for the reversed separator.
            let rev_sep: Text = sep.iter().rev().copied().collect();
            let fail = failure(&rev_sep);
            let mut end = s.len();
            let mut count = max;
            while count > 0 {
                let Some(len) = match_end(s[..end].iter().rev().copied(), &rev_sep, &fail) else {
                    break;
                };
                let pos = end - len;
                count -= 1;
                out.push(&s[pos + sep.len()..end]);
                end = pos;
            }
            out.push(&s[..end]);
        }
    }
    out.reverse();
    Ok(out)
}

fn is_linebreak(c: u32) -> bool {
    matches!(
        c,
        0x0A | 0x0B | 0x0C | 0x0D | 0x1C | 0x1D | 0x1E | 0x85 | 0x2028 | 0x2029
    )
}

/// `str.splitlines(s, keepends)`; `keepends` is taken by truthiness.
///
/// # Errors
/// `TypeError` when `s` is not a `str`.
pub fn splitlines(s: &Value, keepends: Option<&Value>) -> Result<Vec<Text>> {
    Ok(splitlines_slices(s, keepends)?
        .into_iter()
        .map(<[u32]>::to_vec)
        .collect())
}

/// [`splitlines`] returning the pieces as slices of `s` (no copies).
///
/// # Errors
/// As [`splitlines`].
pub fn splitlines_slices<'a>(s: &'a Value, keepends: Option<&Value>) -> Result<Vec<&'a [u32]>> {
    let s = self_text("splitlines", s)?;
    let keepends = keepends.is_some_and(Value::truthy);
    let n = s.len();
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        let j = i;
        while i < n && !is_linebreak(s[i]) {
            i += 1;
        }
        let mut eol = i;
        if i < n {
            i += if s[i] == 0x0D && s.get(i + 1) == Some(&0x0A) {
                2
            } else {
                1
            };
            if keepends {
                eol = i;
            }
        }
        out.push(&s[j..eol]);
    }
    Ok(out)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Left,
    Right,
    Both,
}

fn do_strip(name: &str, s: &Value, chars: Option<&Value>, side: Side) -> Result<Text> {
    let s = self_text(name, s)?;
    let strip_set: Option<&[u32]> = match chars {
        None | Some(Value::None) => None,
        Some(Value::Str(t)) => Some(t),
        Some(_) => return Err(Error::type_error(format!("{name} arg must be None or str"))),
    };
    let strippable = |c: u32| match strip_set {
        None => is_whitespace(c),
        Some(set) => set.contains(&c),
    };
    let mut start = 0;
    let mut end = s.len();
    if side != Side::Right {
        while start < end && strippable(s[start]) {
            start += 1;
        }
    }
    if side != Side::Left {
        while end > start && strippable(s[end - 1]) {
            end -= 1;
        }
    }
    Ok(s[start..end].to_vec())
}

/// `str.strip(s, chars)`.
///
/// # Errors
/// `TypeError` for a non-`str` `s` or `chars` that is neither `str` nor `None`.
pub fn strip(s: &Value, chars: Option<&Value>) -> Result<Text> {
    do_strip("strip", s, chars, Side::Both)
}

/// `str.lstrip(s, chars)`.
///
/// # Errors
/// `TypeError` for a non-`str` `s` or `chars` that is neither `str` nor `None`.
pub fn lstrip(s: &Value, chars: Option<&Value>) -> Result<Text> {
    do_strip("lstrip", s, chars, Side::Left)
}

/// `str.rstrip(s, chars)`.
///
/// # Errors
/// `TypeError` for a non-`str` `s` or `chars` that is neither `str` nor `None`.
pub fn rstrip(s: &Value, chars: Option<&Value>) -> Result<Text> {
    do_strip("rstrip", s, chars, Side::Right)
}

/// CPython `pad()`: `left` and `right` copies of `fill` around `s`.
fn pad(s: &[u32], left: i64, right: i64, fill: u32) -> Result<Text> {
    let total = i64::try_from(s.len())
        .ok()
        .and_then(|n| n.checked_add(left)?.checked_add(right))
        .ok_or_else(|| Error::overflow_error("padded string is too long"))?;
    let mut out = new_text(total)?;
    let left = usize::try_from(left).map_err(|_| Error::memory_error())?;
    let right = usize::try_from(right).map_err(|_| Error::memory_error())?;
    out.resize(left, fill);
    out.extend_from_slice(s);
    out.resize(out.len() + right, fill);
    Ok(out)
}

/// Shared prologue of center/ljust/rjust: `(text, width, fill)`, or the text
/// unchanged when no padding is needed.
fn justify_args<'a>(
    name: &str,
    s: &'a Value,
    width: &Value,
    fillchar: Option<&Value>,
) -> Result<(&'a [u32], i64, u32)> {
    let s = self_text(name, s)?;
    let width = ssize_arg(width)?;
    let fill = fill_arg(fillchar)?;
    Ok((s, width, fill))
}

/// Padding still needed to reach `width`, if any.
fn margin(s: &[u32], width: i64) -> Option<i64> {
    let len = i64::try_from(s.len()).ok()?;
    (width > len).then(|| width - len)
}

/// `str.center(s, width, fillchar)`.
///
/// # Errors
/// `TypeError`/`OverflowError` for bad arguments, `MemoryError` for huge widths.
pub fn center(s: &Value, width: &Value, fillchar: Option<&Value>) -> Result<Text> {
    let (s, width, fill) = justify_args("center", s, width, fillchar)?;
    let Some(marg) = margin(s, width) else {
        return Ok(s.to_vec());
    };
    let left = marg / 2 + (marg & width & 1);
    pad(s, left, marg - left, fill)
}

/// `str.ljust(s, width, fillchar)`.
///
/// # Errors
/// `TypeError`/`OverflowError` for bad arguments, `MemoryError` for huge widths.
pub fn ljust(s: &Value, width: &Value, fillchar: Option<&Value>) -> Result<Text> {
    let (s, width, fill) = justify_args("ljust", s, width, fillchar)?;
    match margin(s, width) {
        Some(marg) => pad(s, 0, marg, fill),
        None => Ok(s.to_vec()),
    }
}

/// `str.rjust(s, width, fillchar)`.
///
/// # Errors
/// `TypeError`/`OverflowError` for bad arguments, `MemoryError` for huge widths.
pub fn rjust(s: &Value, width: &Value, fillchar: Option<&Value>) -> Result<Text> {
    let (s, width, fill) = justify_args("rjust", s, width, fillchar)?;
    match margin(s, width) {
        Some(marg) => pad(s, marg, 0, fill),
        None => Ok(s.to_vec()),
    }
}

/// `str.zfill(s, width)`.
///
/// # Errors
/// `TypeError`/`OverflowError` for bad arguments, `MemoryError` for huge widths.
pub fn zfill(s: &Value, width: &Value) -> Result<Text> {
    let s = self_text("zfill", s)?;
    let width = ssize_arg(width)?;
    let Some(fill) = margin(s, width) else {
        return Ok(s.to_vec());
    };
    let mut out = pad(s, fill, 0, u32::from(b'0'))?;
    let fill = usize::try_from(fill).map_err(|_| Error::memory_error())?;
    if let Some(&c) = out.get(fill) {
        if c == u32::from(b'+') || c == u32::from(b'-') {
            out[0] = c;
            out[fill] = u32::from(b'0');
        }
    }
    Ok(out)
}

/// `str.expandtabs(s, tabsize)`.
///
/// # Errors
/// `TypeError`/`OverflowError` for bad arguments; `OverflowError("new string is too
/// long")` or `MemoryError` when the result cannot exist.
pub fn expandtabs(s: &Value, tabsize: Option<&Value>) -> Result<Text> {
    let s = self_text("expandtabs", s)?;
    let tabsize = i64::from(tabsize.map(c_int_arg).transpose()?.unwrap_or(8));
    let too_long = || Error::overflow_error("new string is too long");
    // First pass: the output length, with CPython's overflow checks.
    let mut total: i64 = 0;
    let mut line_pos: i64 = 0;
    let mut found = false;
    for &c in s {
        if c == u32::from(b'\t') {
            found = true;
            if tabsize > 0 {
                let incr = tabsize - line_pos % tabsize;
                total = total.checked_add(incr).ok_or_else(too_long)?;
                line_pos += incr;
            }
        } else {
            total = total.checked_add(1).ok_or_else(too_long)?;
            line_pos += 1;
            if c == u32::from(b'\n') || c == u32::from(b'\r') {
                line_pos = 0;
            }
        }
    }
    if !found {
        return Ok(s.to_vec());
    }
    let mut out = new_text(total)?;
    let mut line_pos: i64 = 0;
    for &c in s {
        if c == u32::from(b'\t') {
            if tabsize > 0 {
                let incr = tabsize - line_pos % tabsize;
                line_pos += incr;
                let incr = usize::try_from(incr).map_err(|_| Error::memory_error())?;
                out.resize(out.len() + incr, u32::from(b' '));
            }
        } else {
            line_pos += 1;
            out.push(c);
            if c == u32::from(b'\n') || c == u32::from(b'\r') {
                line_pos = 0;
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> Value {
        Value::Str(s.chars().map(u32::from).collect())
    }

    fn txt(s: &str) -> Text {
        s.chars().map(u32::from).collect()
    }

    #[test]
    fn split_matches_cpython() {
        assert_eq!(
            split(&t(" a\x1cb  "), None, None),
            Ok(vec![txt("a"), txt("b")])
        );
        assert_eq!(
            rsplit(&t("aaa"), Some(&t("aa")), None),
            Ok(vec![txt("a"), txt("")])
        );
        assert_eq!(
            split(&t("aaa"), Some(&t("aa")), None),
            Ok(vec![txt(""), txt("a")])
        );
        assert_eq!(
            rsplit(&t(" a b c "), None, Some(&Value::Int(1.into()))),
            Ok(vec![txt(" a b"), txt("c")])
        );
    }

    #[test]
    fn padding_matches_cpython() {
        let w = |n: i64| Value::Int(n.into());
        assert_eq!(center(&t("abc"), &w(6), Some(&t("*"))), Ok(txt("*abc**")));
        assert_eq!(center(&t("ab"), &w(5), Some(&t("*"))), Ok(txt("**ab*")));
        assert_eq!(zfill(&t("-12"), &w(6)), Ok(txt("-00012")));
        assert_eq!(
            center(&t("a"), &w(1 << 62), None).map_err(|e| e.kind),
            Err(crate::ErrorKind::Memory)
        );
    }

    #[test]
    fn expandtabs_matches_cpython() {
        assert_eq!(
            expandtabs(&t("ab\tc\r\td\n\te"), Some(&Value::Int(3.into()))),
            Ok(txt("ab c\r   d\n   e"))
        );
    }
}
