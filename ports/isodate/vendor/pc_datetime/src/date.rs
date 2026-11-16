//! Task `date`: the proleptic Gregorian calendar of CPython's `_datetime` module, the
//! `date` type, and the shim's date functions.
//!
//! Calendar arithmetic mirrors `_datetimemodule.c`, including C's truncating `/` and `%`
//! (Rust's integer `/` and `%` truncate the same way). That matters only for the
//! unvalidated dates CPython's pickle fast path can build (see [`date_new`]).

use std::fmt::Write as _;

use num_traits::ToPrimitive;

use std::borrow::Borrow;

use crate::args::Param;
use crate::error::{PyError, PyResult};
use crate::timedelta::td_in;
use crate::{Kwargs, Value};

/// `datetime.MINYEAR`.
pub const MINYEAR: i64 = 1;
/// `datetime.MAXYEAR`.
pub const MAXYEAR: i64 = 9999;
/// `date.max.toordinal()`.
pub const MAX_ORDINAL: i64 = 3_652_059;

/// Days in 400, 100 and 4 Gregorian years.
const DI400Y: i64 = 146_097;
const DI100Y: i64 = 36_524;
const DI4Y: i64 = 1_461;

/// Days before the first of each month in a non-leap year; index 0 is unused.
const DAYS_BEFORE_MONTH: [i64; 13] = [0, 0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
/// Days in each month of a non-leap year; index 0 is unused.
const DAYS_IN_MONTH: [i64; 13] = [0, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

// ------------------------------------------------------------------ argument conversion

/// `PyLong_AsLong` behind the `l`/`i` argument formats: an int or bool, else TypeError;
/// OverflowError when it doesn't fit a C long.
///
/// # Errors
/// `TypeError("'T' object cannot be interpreted as an integer")` for non-integers,
/// `OverflowError("Python int too large to convert to C long")` outside `i64`.
pub fn as_c_long(value: &Value) -> PyResult<i64> {
    match value {
        Value::Bool(b) => Ok(i64::from(*b)),
        Value::Int(n) => n
            .to_i64()
            .ok_or_else(|| PyError::overflow_error("Python int too large to convert to C long")),
        other => Err(PyError::type_error(format!(
            "'{}' object cannot be interpreted as an integer",
            other.type_name()
        ))),
    }
}

/// The `i` argument format: [`as_c_long`], then a C int range check.
///
/// # Errors
/// [`as_c_long`]'s errors, then `OverflowError("signed integer is greater than maximum")`
/// or `OverflowError("signed integer is less than minimum")`.
pub fn as_c_int(value: &Value) -> PyResult<i32> {
    let n = as_c_long(value)?;
    i32::try_from(n).map_err(|_| {
        PyError::overflow_error(if n > 0 {
            "signed integer is greater than maximum"
        } else {
            "signed integer is less than minimum"
        })
    })
}

/// Appends `n` zero-padded to `width` digits, exactly as `format!("{n:0width$}")`.
pub(crate) fn push_padded(out: &mut String, n: impl Into<i64>, width: usize) {
    let n: i64 = n.into();
    let Ok(mut rest) = u64::try_from(n) else {
        // Negative fields don't occur in valid values; keep `format!`'s sign handling.
        let _ = write!(out, "{n:0width$}");
        return;
    };
    let mut digits = [b'0'; 20];
    let mut start = digits.len();
    while rest > 0 {
        start -= 1;
        digits[start] = b'0' + u8::try_from(rest % 10).unwrap_or(0);
        rest /= 10;
    }
    let len = digits.len() - start;
    for _ in len..width {
        out.push('0');
    }
    // A zero with width 0 still prints one digit.
    if len == 0 && width == 0 {
        out.push('0');
    }
    out.extend(digits[start..].iter().map(|&b| char::from(b)));
}

// ------------------------------------------------------------------ calendar

/// Whether `year` is a leap year, computed on the year as a C `unsigned int` like CPython.
pub fn is_leap(year: i64) -> bool {
    let y = year.rem_euclid(1 << 32);
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}

/// Days in `month` (1..=12) of `year`; 0 for an invalid month.
pub fn days_in_month(year: i64, month: i64) -> i64 {
    if month == 2 && is_leap(year) {
        29
    } else {
        usize::try_from(month)
            .ok()
            .and_then(|m| DAYS_IN_MONTH.get(m))
            .copied()
            .unwrap_or(0)
    }
}

/// Days in `year` before the first of `month` (1..=12).
pub(crate) fn days_before_month(year: i64, month: i64) -> i64 {
    let base = usize::try_from(month)
        .ok()
        .and_then(|m| DAYS_BEFORE_MONTH.get(m))
        .copied()
        .unwrap_or(0);
    base + i64::from(month > 2 && is_leap(year))
}

/// Days before January 1 of `year`, with C's truncating division (wrong below year 1,
/// exactly as in CPython).
pub(crate) fn days_before_year(year: i64) -> i64 {
    let y = year - 1;
    y * 365 + y / 4 - y / 100 + y / 400
}

/// Proleptic Gregorian ordinal of a date; 0001-01-01 is 1.
pub(crate) fn ymd_to_ord(year: i64, month: i64, day: i64) -> i64 {
    days_before_year(year) + days_before_month(year, month) + day
}

/// The (year, month, day) of an ordinal `>= 1`.
pub(crate) fn ord_to_ymd(ordinal: i64) -> (i64, i64, i64) {
    let n = ordinal - 1;
    let n400 = n.div_euclid(DI400Y);
    let n = n.rem_euclid(DI400Y);
    let n100 = n / DI100Y;
    let n = n % DI100Y;
    let n4 = n / DI4Y;
    let n = n % DI4Y;
    let n1 = n / 365;
    let mut n = n % 365;
    let year = n400 * 400 + 1 + n100 * 100 + n4 * 4 + n1;
    if n1 == 4 || n100 == 4 {
        return (year - 1, 12, 31);
    }
    let leap = n1 == 3 && (n4 != 24 || n100 == 3);
    let mut month = (n + 50) >> 5;
    let before = |m: i64| {
        let base = usize::try_from(m)
            .ok()
            .and_then(|m| DAYS_BEFORE_MONTH.get(m))
            .copied()
            .unwrap_or(0);
        base + i64::from(m > 2 && leap)
    };
    let mut preceding = before(month);
    if preceding > n {
        month -= 1;
        preceding = before(month);
    }
    n -= preceding;
    (year, month, n + 1)
}

/// Monday 0 .. Sunday 6, with C's `%` (negative for dates before year 1).
pub(crate) fn weekday_of(year: i64, month: i64, day: i64) -> i64 {
    (ymd_to_ord(year, month, day) + 6) % 7
}

/// Ordinal of the Monday that starts ISO week 1 of `year`.
pub(crate) fn iso_week1_monday(year: i64) -> i64 {
    let first_day = ymd_to_ord(year, 1, 1);
    let first_weekday = (first_day + 6) % 7;
    let monday = first_day - first_weekday;
    if first_weekday > 3 {
        monday + 7
    } else {
        monday
    }
}

/// CPython's `check_date_args`: the date constructor's range checks.
///
/// # Errors
/// ValueError `year N is out of range`, `month must be in 1..12` or
/// `day is out of range for month`.
pub fn check_date_args(year: i64, month: i64, day: i64) -> PyResult<()> {
    if !(MINYEAR..=MAXYEAR).contains(&year) {
        return Err(PyError::value_error(format!("year {year} is out of range")));
    }
    if !(1..=12).contains(&month) {
        return Err(PyError::value_error("month must be in 1..12"));
    }
    if day < 1 || day > days_in_month(year, month) {
        return Err(PyError::value_error("day is out of range for month"));
    }
    Ok(())
}

fn date_out_of_range() -> PyError {
    PyError::overflow_error("date value out of range")
}

/// CPython's `normalize_date`: brings an out-of-range `day` back into its month, carrying
/// into month and year, for a `month` in 1..=12.
///
/// # Errors
/// `OverflowError("date value out of range")` when the result leaves years 1..=9999.
pub fn normalize_date(year: i64, month: i64, day: i64) -> PyResult<Date> {
    let (mut y, mut m, mut d) = (year, month, day);
    let dim = days_in_month(y, m);
    if d < 1 || d > dim {
        if d == 0 {
            m -= 1;
            if m > 0 {
                d = days_in_month(y, m);
            } else {
                y -= 1;
                m = 12;
                d = 31;
            }
        } else if d == dim + 1 {
            m += 1;
            d = 1;
            if m > 12 {
                m = 1;
                y += 1;
            }
        } else {
            let ordinal = ymd_to_ord(y, m, 1) + d - 1;
            if !(1..=MAX_ORDINAL).contains(&ordinal) {
                return Err(date_out_of_range());
            }
            let (ny, nm, nd) = ord_to_ymd(ordinal);
            return Date::new(ny, nm, nd);
        }
    }
    if !(MINYEAR..=MAXYEAR).contains(&y) {
        return Err(date_out_of_range());
    }
    Date::new(y, m, d)
}

// ------------------------------------------------------------------ Date

/// A `datetime.date`.
///
/// Dates from [`Date::new`] are valid. CPython's pickle fast path also builds dates
/// without checks (year 0..=65535, day 0..=255), and every operation here handles those
/// the way CPython's C code does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Date {
    year: i32,
    month: i32,
    day: i32,
}

impl Date {
    /// `date(year, month, day)`.
    ///
    /// # Errors
    /// The ValueErrors of [`check_date_args`].
    pub fn new(year: i64, month: i64, day: i64) -> PyResult<Self> {
        check_date_args(year, month, day)?;
        let field = |n: i64| i32::try_from(n).map_err(|_| date_out_of_range());
        Ok(Self {
            year: field(year)?,
            month: field(month)?,
            day: field(day)?,
        })
    }

    /// CPython's `date_from_pickle`: four state bytes, not validated.
    fn from_pickle(state: [u8; 4]) -> Self {
        Self {
            year: i32::from(state[0]) << 8 | i32::from(state[1]),
            month: i32::from(state[2]),
            day: i32::from(state[3]),
        }
    }

    /// `date.fromordinal(ordinal)`.
    ///
    /// # Errors
    /// `ValueError("ordinal must be >= 1")`, then the constructor's range errors.
    pub fn from_ordinal(ordinal: i64) -> PyResult<Self> {
        if ordinal < 1 {
            return Err(PyError::value_error("ordinal must be >= 1"));
        }
        let (y, m, d) = ord_to_ymd(ordinal);
        Self::new(y, m, d)
    }

    /// `date.fromisocalendar(year, week, day)` after argument conversion.
    ///
    /// # Errors
    /// ValueError `Year is out of range: N`, `Invalid week: N`,
    /// `Invalid day: N (range is [1, 7])`, then the constructor's range errors.
    pub fn from_iso_calendar(year: i32, week: i32, day: i32) -> PyResult<Self> {
        let (y, w, d) = (i64::from(year), i64::from(week), i64::from(day));
        if !(MINYEAR..=MAXYEAR).contains(&y) {
            return Err(PyError::value_error(format!("Year is out of range: {y}")));
        }
        if w <= 0 || w >= 53 {
            let first_weekday = (ymd_to_ord(y, 1, 1) + 6) % 7;
            let long_year = first_weekday == 3 || (first_weekday == 2 && is_leap(y));
            if !(w == 53 && long_year) {
                return Err(PyError::value_error(format!("Invalid week: {w}")));
            }
        }
        if d <= 0 || d >= 8 {
            return Err(PyError::value_error(format!(
                "Invalid day: {d} (range is [1, 7])"
            )));
        }
        let ordinal = iso_week1_monday(y) + (w - 1) * 7 + d - 1;
        let (ny, nm, nd) = ord_to_ymd(ordinal);
        Self::new(ny, nm, nd)
    }

    /// The year.
    pub fn year(self) -> i32 {
        self.year
    }

    /// The month.
    pub fn month(self) -> i32 {
        self.month
    }

    /// The day.
    pub fn day(self) -> i32 {
        self.day
    }

    fn ymd(self) -> (i64, i64, i64) {
        (
            i64::from(self.year),
            i64::from(self.month),
            i64::from(self.day),
        )
    }

    /// `toordinal()`.
    pub fn to_ordinal(self) -> i64 {
        let (y, m, d) = self.ymd();
        ymd_to_ord(y, m, d)
    }

    /// `weekday()`: Monday 0 .. Sunday 6.
    pub fn weekday(self) -> i64 {
        let (y, m, d) = self.ymd();
        weekday_of(y, m, d)
    }

    /// `isoweekday()`: Monday 1 .. Sunday 7.
    pub fn isoweekday(self) -> i64 {
        self.weekday() + 1
    }

    /// `isocalendar()` as (ISO year, week, weekday).
    pub fn iso_calendar(self) -> (i64, i64, i64) {
        let mut year = i64::from(self.year);
        let today = self.to_ordinal();
        let mut monday = iso_week1_monday(year);
        let mut week = (today - monday).div_euclid(7);
        let mut day = (today - monday).rem_euclid(7);
        if week < 0 {
            year -= 1;
            monday = iso_week1_monday(year);
            week = (today - monday).div_euclid(7);
            day = (today - monday).rem_euclid(7);
        } else if week >= 52 && today >= iso_week1_monday(year + 1) {
            year += 1;
            week = 0;
        }
        (year, week + 1, day + 1)
    }

    /// `self + timedelta(days=days)`; the delta's seconds never reach a date.
    ///
    /// # Errors
    /// `OverflowError("date value out of range")`.
    pub fn add_days(self, days: i64) -> PyResult<Self> {
        let (y, m, d) = self.ymd();
        normalize_date(y, m, d + days)
    }

    /// `isoformat()`: C's `%04d-%02d-%02d`.
    pub fn isoformat(self) -> String {
        let mut out = String::with_capacity(10);
        self.write_isoformat(&mut out);
        out
    }

    /// Appends [`Self::isoformat`] to `out`.
    pub(crate) fn write_isoformat(self, out: &mut String) {
        push_padded(out, self.year, 4);
        out.push('-');
        push_padded(out, self.month, 2);
        out.push('-');
        push_padded(out, self.day, 2);
    }

    /// The shim's `_date_out`: `(year, month, day)`.
    pub fn to_value(self) -> Value {
        Value::tuple(vec![
            Value::int(self.year),
            Value::int(self.month),
            Value::int(self.day),
        ])
    }
}

/// The four state bytes of CPython's pickle fast path for a single `date()` argument,
/// `Ok(None)` when the argument doesn't take that path.
fn pickle_state(arg: &Value) -> PyResult<Option<[u8; 4]>> {
    let sane = |month: u32| (1..=12).contains(&month);
    match arg {
        Value::Bytes(b) => Ok(<[u8; 4]>::try_from(b.as_slice())
            .ok()
            .filter(|state| sane(u32::from(state[2])))),
        Value::Str(s) => {
            let points = s.code_points();
            if points.len() != 4 || !sane(points[2]) {
                return Ok(None);
            }
            let mut state = [0u8; 4];
            for (byte, &point) in state.iter_mut().zip(points) {
                *byte = u8::try_from(point).map_err(|_| {
                    PyError::value_error(
                        "Failed to encode latin1 string when unpickling a date object. \
                         pickle.load(data, encoding='latin1') is assumed.",
                    )
                })?;
            }
            Ok(Some(state))
        }
        _ => Ok(None),
    }
}

/// `datetime.date(*args)` with positional arguments only, as CPython's `date_new` does it:
/// the pickle fast path for one bytes/str argument, then `iii` argument parsing.
///
/// # Errors
/// CPython's binding TypeErrors, [`as_c_int`]'s errors, then [`Date::new`]'s.
pub fn date_new<V: Borrow<Value>>(args: &[V]) -> PyResult<Date> {
    if let [arg] = args {
        if let Some(state) = pickle_state(arg.borrow())? {
            return Ok(Date::from_pickle(state));
        }
    }
    const NAMES: [&str; 3] = ["year", "month", "day"];
    if args.len() > NAMES.len() {
        return Err(PyError::type_error(format!(
            "function takes at most 3 arguments ({} given)",
            args.len()
        )));
    }
    let mut fields = [0i64; 3];
    for (pos, (field, name)) in fields.iter_mut().zip(NAMES).enumerate() {
        let Some(arg) = args.get(pos) else {
            return Err(PyError::type_error(format!(
                "function missing required argument '{name}' (pos {})",
                pos + 1
            )));
        };
        *field = i64::from(as_c_int(arg.borrow())?);
    }
    Date::new(fields[0], fields[1], fields[2])
}

/// The shim's `_date_in(d)`: `datetime.date(*d)`.
///
/// # Errors
/// TypeError when `d` isn't iterable, then [`date_new`]'s errors.
pub fn date_in(d: &Value) -> PyResult<Date> {
    // Unpacking a list or tuple sees its items as they are; skip `iterate`'s copy.
    if let Value::List(items) | Value::Tuple(items) = d {
        return date_new(items);
    }
    let Some(items) = d.iterate() else {
        return Err(PyError::type_error(format!(
            "datetime.date() argument after * must be an iterable, not {}",
            d.type_name()
        )));
    };
    date_new(&items)
}

// ------------------------------------------------------------------ shim functions

const D: [Param; 1] = [Param::required("d")];
const YMD: [Param; 3] = [
    Param::required("year"),
    Param::required("month"),
    Param::required("day"),
];
const YWD: [Param; 3] = [
    Param::required("year"),
    Param::required("week"),
    Param::required("day"),
];

fn bind_date(name: &str, args: &[Value], kwargs: &Kwargs) -> PyResult<Date> {
    let v = crate::timedelta::bind_ref(name, &D, args, kwargs)?;
    date_in(&v[0])
}

/// `pc_datetime.make_date`.
///
/// # Errors
/// Binding errors, then those of `date(year, month, day)`.
pub fn make_date(args: &[Value], kwargs: &Kwargs) -> PyResult<Value> {
    let v = crate::timedelta::bind_ref("make_date", &YMD, args, kwargs)?;
    Ok(date_new(&v)?.to_value())
}

/// `pc_datetime.date_toordinal`.
///
/// # Errors
/// Binding errors, then those of `date(*d)`.
pub fn date_toordinal(args: &[Value], kwargs: &Kwargs) -> PyResult<Value> {
    Ok(Value::int(
        bind_date("date_toordinal", args, kwargs)?.to_ordinal(),
    ))
}

/// `pc_datetime.date_fromordinal`.
///
/// # Errors
/// Binding errors, C int conversion errors, then [`Date::from_ordinal`]'s.
pub fn date_fromordinal(args: &[Value], kwargs: &Kwargs) -> PyResult<Value> {
    let v = crate::timedelta::bind_ref("date_fromordinal", &[Param::required("n")], args, kwargs)?;
    let ordinal = as_c_int(&v[0])?;
    Ok(Date::from_ordinal(i64::from(ordinal))?.to_value())
}

/// `pc_datetime.weekday`.
///
/// # Errors
/// Binding errors, then those of `date(*d)`.
pub fn weekday(args: &[Value], kwargs: &Kwargs) -> PyResult<Value> {
    Ok(Value::int(bind_date("weekday", args, kwargs)?.weekday()))
}

/// `pc_datetime.isoweekday`.
///
/// # Errors
/// Binding errors, then those of `date(*d)`.
pub fn isoweekday(args: &[Value], kwargs: &Kwargs) -> PyResult<Value> {
    Ok(Value::int(
        bind_date("isoweekday", args, kwargs)?.isoweekday(),
    ))
}

/// `pc_datetime.isocalendar`.
///
/// # Errors
/// Binding errors, then those of `date(*d)`.
pub fn isocalendar(args: &[Value], kwargs: &Kwargs) -> PyResult<Value> {
    let (year, week, day) = bind_date("isocalendar", args, kwargs)?.iso_calendar();
    Ok(Value::tuple(vec![
        Value::int(year),
        Value::int(week),
        Value::int(day),
    ]))
}

/// `pc_datetime.date_fromisocalendar`.
///
/// # Errors
/// Binding errors, conversion errors (overflow becomes
/// `ValueError("ISO calendar component out of range")`), then
/// [`Date::from_iso_calendar`]'s.
pub fn date_fromisocalendar(args: &[Value], kwargs: &Kwargs) -> PyResult<Value> {
    let v = crate::timedelta::bind_ref("date_fromisocalendar", &YWD, args, kwargs)?;
    let convert = |value: &Value| {
        as_c_int(value).map_err(|e| match e {
            PyError::OverflowError(_) => {
                PyError::value_error("ISO calendar component out of range")
            }
            other => other,
        })
    };
    let year = convert(&v[0])?;
    let week = convert(&v[1])?;
    let day = convert(&v[2])?;
    Ok(Date::from_iso_calendar(year, week, day)?.to_value())
}

/// `pc_datetime.date_add`.
///
/// # Errors
/// Binding errors, `date(*d)`'s, `_td_in(td)`'s, then
/// `OverflowError("date value out of range")`.
pub fn date_add(args: &[Value], kwargs: &Kwargs) -> PyResult<Value> {
    let v = crate::timedelta::bind_ref(
        "date_add",
        &[Param::required("d"), Param::required("td")],
        args,
        kwargs,
    )?;
    let date = date_in(&v[0])?;
    let delta = td_in(&v[1])?;
    Ok(date.add_days(i64::from(delta.days()))?.to_value())
}

/// `pc_datetime.date_isoformat`.
///
/// # Errors
/// Binding errors, then those of `date(*d)`.
pub fn date_isoformat(args: &[Value], kwargs: &Kwargs) -> PyResult<Value> {
    Ok(Value::str(
        bind_date("date_isoformat", args, kwargs)?.isoformat(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_padded_matches_format() {
        let values = [
            0,
            1,
            5,
            9,
            10,
            99,
            100,
            999,
            1000,
            9999,
            123_456,
            999_999,
            -1,
            -5,
            -100,
            i64::MAX,
            i64::MIN,
        ];
        for n in values {
            for width in 0..8 {
                let mut out = String::from("x");
                push_padded(&mut out, n, width);
                assert_eq!(out, format!("x{n:0width$}"));
            }
        }
    }

    #[test]
    fn ordinal_round_trip() {
        for ordinal in [1, 59, 60, 365, 366, 730_120, 719_163, MAX_ORDINAL] {
            let (y, m, d) = ord_to_ymd(ordinal);
            assert_eq!(ymd_to_ord(y, m, d), ordinal, "ordinal {ordinal}");
        }
        assert_eq!(ord_to_ymd(MAX_ORDINAL), (9999, 12, 31));
    }

    #[test]
    fn iso_calendar_edges() {
        let d = Date::new(9999, 12, 31).map(Date::iso_calendar);
        assert_eq!(d, Ok((9999, 52, 5)));
        let d = Date::from_pickle([0, 0, 1, 1]);
        assert_eq!(d.to_ordinal(), -364);
        assert_eq!(d.weekday(), -1);
        assert_eq!(d.iso_calendar(), (-1, 52, 7));
    }
}
