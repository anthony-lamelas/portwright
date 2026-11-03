//! Per-character `unicodedata` properties (Unicode 15.0, CPython 3.12).

use crate::args::bad_arg_type_name;
use crate::error::{Error, Result};
use crate::tables::unidata as t;
use crate::value::Value;

pub use crate::tables::UNIDATA_VERSION;

/// How CPython's argument parser words the bad-character error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArgLabel {
    /// Single-argument functions: `f() argument must be ...`.
    Only,
    /// Functions taking a default: `f() argument 1 must be ...`.
    First,
}

/// Extracts the single code point of `ch`, or raises CPython's `TypeError`.
fn single_char(func: &str, label: ArgLabel, ch: &Value) -> Result<u32> {
    if let Value::Str(s) = ch {
        if let [cp] = s.as_slice() {
            return Ok(*cp);
        }
    }
    let label = match label {
        ArgLabel::Only => "argument",
        ArgLabel::First => "argument 1",
    };
    Err(Error::type_error(format!(
        "{func}() {label} must be a unicode character, not {}",
        bad_arg_type_name(ch)
    )))
}

/// The `(category, bidi, combining, east_asian_width, mirrored)` record of `cp`.
fn record(cp: u32) -> (u8, u8, u8, u8, u8) {
    let index = usize::from(t::record_index(cp));
    t::RECORDS.get(index).copied().unwrap_or((0, 0, 0, 0, 0))
}

fn name_at(names: &'static [&'static str], index: u8) -> &'static str {
    names.get(usize::from(index)).copied().unwrap_or("")
}

/// `unicodedata.unidata_version`.
#[must_use]
pub fn unidata_version() -> &'static str {
    UNIDATA_VERSION
}

/// `unicodedata.category(ch)`.
///
/// # Errors
/// `TypeError` unless `ch` is a one-character `str`.
pub fn category(ch: &Value) -> Result<&'static str> {
    let cp = single_char("category", ArgLabel::Only, ch)?;
    Ok(name_at(&t::CATEGORY_NAMES, record(cp).0))
}

/// `unicodedata.bidirectional(ch)`.
///
/// # Errors
/// `TypeError` unless `ch` is a one-character `str`.
pub fn bidirectional(ch: &Value) -> Result<&'static str> {
    let cp = single_char("bidirectional", ArgLabel::Only, ch)?;
    Ok(name_at(&t::BIDI_NAMES, record(cp).1))
}

/// `unicodedata.combining(ch)`: the canonical combining class.
///
/// # Errors
/// `TypeError` unless `ch` is a one-character `str`.
pub fn combining(ch: &Value) -> Result<u8> {
    let cp = single_char("combining", ArgLabel::Only, ch)?;
    Ok(record(cp).2)
}

/// `unicodedata.east_asian_width(ch)`.
///
/// # Errors
/// `TypeError` unless `ch` is a one-character `str`.
pub fn east_asian_width(ch: &Value) -> Result<&'static str> {
    let cp = single_char("east_asian_width", ArgLabel::Only, ch)?;
    Ok(name_at(&t::EAW_NAMES, record(cp).3))
}

/// `unicodedata.mirrored(ch)`: `1` if mirrored in bidi text, else `0`.
///
/// # Errors
/// `TypeError` unless `ch` is a one-character `str`.
pub fn mirrored(ch: &Value) -> Result<u8> {
    let cp = single_char("mirrored", ArgLabel::Only, ch)?;
    Ok(record(cp).4)
}

/// `unicodedata.decomposition(ch)`: the raw UCD mapping, or `""`.
///
/// # Errors
/// `TypeError` unless `ch` is a one-character `str`.
pub fn decomposition(ch: &Value) -> Result<&'static str> {
    let cp = single_char("decomposition", ArgLabel::Only, ch)?;
    Ok(decomposition_of(cp).unwrap_or(""))
}

/// The UCD decomposition mapping of `cp`, if any (Hangul syllables have none here).
pub(crate) fn decomposition_of(cp: u32) -> Option<&'static str> {
    t::DECOMPOSITIONS
        .binary_search_by_key(&cp, |&(c, _)| c)
        .ok()
        .and_then(|i| t::DECOMPOSITIONS.get(i))
        .map(|&(_, d)| d)
}

/// Which of CPython's three numeric properties a lookup wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NumericKind {
    Numeric = 0,
    Digit = 1,
    Decimal = 2,
}

/// The value of `cp` if it has at least property `kind`.
fn numeric_of(cp: u32, kind: NumericKind) -> Option<f64> {
    let i = t::NUMERIC_RUNS.partition_point(|&(first, _, _, _)| first <= cp);
    let &(first, last, base, run_kind) = t::NUMERIC_RUNS.get(i.checked_sub(1)?)?;
    (cp <= last && run_kind >= kind as u8).then(|| base + f64::from(cp - first))
}

/// Shared shape of `numeric`/`decimal`/`digit`: `Ok(None)` means "return `default`".
fn lookup_numeric(
    func: &str,
    kind: NumericKind,
    missing: &str,
    ch: &Value,
    default: Option<&Value>,
) -> Result<Option<f64>> {
    let cp = single_char(func, ArgLabel::First, ch)?;
    match numeric_of(cp, kind) {
        Some(v) => Ok(Some(v)),
        None if default.is_some_and(|d| *d != Value::None) => Ok(None),
        None => Err(Error::value_error(missing)),
    }
}

/// Upstream `numeric_value(ch, default=None)` = `unicodedata.numeric`.
/// `Ok(None)` means the caller must return `default` unchanged.
///
/// # Errors
/// `TypeError` for a bad `ch`; `ValueError('not a numeric character')` when `ch` has no
/// value and `default` is missing or `None`.
pub fn numeric_value(ch: &Value, default: Option<&Value>) -> Result<Option<f64>> {
    lookup_numeric(
        "numeric",
        NumericKind::Numeric,
        "not a numeric character",
        ch,
        default,
    )
}

/// Upstream `decimal_value(ch, default=None)` = `unicodedata.decimal`.
/// `Ok(None)` means the caller must return `default` unchanged.
///
/// # Errors
/// `TypeError` for a bad `ch`; `ValueError('not a decimal')` when `ch` has no value and
/// `default` is missing or `None`.
pub fn decimal_value(ch: &Value, default: Option<&Value>) -> Result<Option<i64>> {
    let v = lookup_numeric(
        "decimal",
        NumericKind::Decimal,
        "not a decimal",
        ch,
        default,
    )?;
    Ok(v.map(to_int))
}

/// Upstream `digit_value(ch, default=None)` = `unicodedata.digit`.
/// `Ok(None)` means the caller must return `default` unchanged.
///
/// # Errors
/// `TypeError` for a bad `ch`; `ValueError('not a digit')` when `ch` has no value and
/// `default` is missing or `None`.
pub fn digit_value(ch: &Value, default: Option<&Value>) -> Result<Option<i64>> {
    let v = lookup_numeric("digit", NumericKind::Digit, "not a digit", ch, default)?;
    Ok(v.map(to_int))
}

/// Digit and decimal values are small non-negative integers (0..=9) in the tables.
#[expect(
    clippy::cast_possible_truncation,
    reason = "digit values are exact integers 0..=9"
)]
fn to_int(v: f64) -> i64 {
    v as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(c: u32) -> Value {
        Value::Str(vec![c])
    }

    #[test]
    fn properties() {
        assert_eq!(category(&ch(0x41)), Ok("Lu"));
        assert_eq!(category(&ch(0xD800)), Ok("Cs"));
        assert_eq!(bidirectional(&ch(0x378)), Ok(""));
        assert_eq!(combining(&ch(0x301)), Ok(230));
        assert_eq!(east_asian_width(&ch(0x3000)), Ok("F"));
        assert_eq!(mirrored(&ch(0x28)), Ok(1));
        assert_eq!(decomposition(&ch(0xC0)), Ok("0041 0300"));
        assert_eq!(decomposition(&ch(0xAC00)), Ok(""));
    }

    #[test]
    fn numbers() {
        assert_eq!(numeric_value(&ch(0xBD), None), Ok(Some(0.5)));
        assert_eq!(digit_value(&ch(0xB2), None), Ok(Some(2)));
        assert_eq!(
            decimal_value(&ch(0xB2), None).map_err(|e| e.message),
            Err("not a decimal".to_owned())
        );
        assert_eq!(
            decimal_value(&ch(0xB2), Some(&Value::Bool(false))),
            Ok(None)
        );
        assert_eq!(numeric_value(&ch(0x4E07), None), Ok(Some(10000.0)));
    }

    #[test]
    fn bad_args() {
        let e = numeric_value(&Value::None, None).map_err(|e| e.message);
        assert_eq!(
            e,
            Err("numeric() argument 1 must be a unicode character, not None".to_owned())
        );
        let e = category(&Value::Str(vec![])).map_err(|e| e.message);
        assert_eq!(
            e,
            Err("category() argument must be a unicode character, not str".to_owned())
        );
    }
}
