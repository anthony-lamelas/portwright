//! Task `datetime`: the `time` and `datetime` types of CPython's `_datetime` module with
//! fixed-offset zones, their arithmetic, comparison, `str`/`repr` and `isoformat`.
//!
//! The shim's zones are always `datetime.timezone` instances, so a zone is just a
//! [`FixedOffset`]. Two distinct zone objects with equal offsets behave exactly like one
//! shared object in every operation here, so object identity needs no modelling.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::fmt::Write as _;

use num_bigint::BigInt;

use crate::args::{Default, Param};
use crate::date::{as_c_int, check_date_args, push_padded, Date};
use crate::error::{PyError, PyResult};
use crate::pystr::PyStr;
use crate::timedelta::{item, offset_in, td_in, FixedOffset, TimeDelta};
use crate::{Kwargs, Value};

const US_PER_SECOND: i64 = 1_000_000;
const US_PER_DAY: i64 = 86_400 * US_PER_SECOND;

// ------------------------------------------------------------------ Time

/// A `datetime.time` with an optional fixed-offset zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Time {
    hour: i32,
    minute: i32,
    second: i32,
    microsecond: i32,
    tz: Option<FixedOffset>,
    fold: i32,
}

/// CPython's `check_time_args`.
fn check_time_args(hour: i32, minute: i32, second: i32, us: i32, fold: i32) -> PyResult<()> {
    let checks = [
        (hour, 23, "hour must be in 0..23"),
        (minute, 59, "minute must be in 0..59"),
        (second, 59, "second must be in 0..59"),
        (us, 999_999, "microsecond must be in 0..999999"),
    ];
    for (value, max, message) in checks {
        if !(0..=max).contains(&value) {
            return Err(PyError::value_error(message));
        }
    }
    if fold != 0 && fold != 1 {
        return Err(PyError::value_error("fold must be either 0 or 1"));
    }
    Ok(())
}

/// The time-of-day arguments as Python values, before `i` conversion.
struct TimeArgs<'a> {
    hour: &'a Value,
    minute: &'a Value,
    second: &'a Value,
    microsecond: &'a Value,
    tz: Option<FixedOffset>,
    fold: &'a Value,
}

impl TimeArgs<'_> {
    /// Converts the fields with the `i` format, in CPython's order, then validates them.
    fn convert(&self) -> PyResult<Time> {
        let hour = as_c_int(self.hour)?;
        let minute = as_c_int(self.minute)?;
        let second = as_c_int(self.second)?;
        let microsecond = as_c_int(self.microsecond)?;
        let fold = as_c_int(self.fold)?;
        check_time_args(hour, minute, second, microsecond, fold)?;
        Ok(Time {
            hour,
            minute,
            second,
            microsecond,
            tz: self.tz,
            fold,
        })
    }
}

impl Time {
    /// The hour, `0..24`.
    pub fn hour(self) -> i32 {
        self.hour
    }

    /// The minute, `0..60`.
    pub fn minute(self) -> i32 {
        self.minute
    }

    /// The second, `0..60`.
    pub fn second(self) -> i32 {
        self.second
    }

    /// The microsecond, `0..1_000_000`.
    pub fn microsecond(self) -> i32 {
        self.microsecond
    }

    /// The zone, or None when naive.
    pub fn tz(self) -> Option<FixedOffset> {
        self.tz
    }

    /// Microseconds since midnight.
    fn micros(self) -> i64 {
        ((i64::from(self.hour) * 60 + i64::from(self.minute)) * 60 + i64::from(self.second))
            * US_PER_SECOND
            + i64::from(self.microsecond)
    }

    /// A naive or aware time from microseconds since midnight, `0..US_PER_DAY`.
    fn from_micros(us: i64, tz: Option<FixedOffset>) -> PyResult<Self> {
        let field = |n: i64| i32::try_from(n).map_err(|_| date_out_of_range());
        let seconds = us.div_euclid(US_PER_SECOND);
        Ok(Self {
            hour: field(seconds / 3600)?,
            minute: field(seconds / 60 % 60)?,
            second: field(seconds % 60)?,
            microsecond: field(us.rem_euclid(US_PER_SECOND))?,
            tz,
            fold: 0,
        })
    }

    /// Appends `isoformat(timespec)` without the offset.
    fn write_clock(self, out: &mut String, spec: TimeSpec) {
        let (h, m, s, us) = (self.hour, self.minute, self.second, self.microsecond);
        push_padded(out, h, 2);
        if spec == TimeSpec::Hours {
            return;
        }
        out.push(':');
        push_padded(out, m, 2);
        if spec == TimeSpec::Minutes {
            return;
        }
        out.push(':');
        push_padded(out, s, 2);
        match spec {
            TimeSpec::Milliseconds => {
                out.push('.');
                push_padded(out, us / 1000, 3);
            }
            TimeSpec::Microseconds => {
                out.push('.');
                push_padded(out, us, 6);
            }
            TimeSpec::Auto if us != 0 => {
                out.push('.');
                push_padded(out, us, 6);
            }
            _ => {}
        }
    }

    /// Appends `t.isoformat(timespec)`.
    fn write_isoformat(self, out: &mut String, spec: TimeSpec) {
        self.write_clock(out, spec);
        if let Some(tz) = self.tz {
            write_utcoffset(out, tz.utcoffset());
        }
    }

    /// `t.isoformat(timespec)`.
    pub fn isoformat(self, spec: TimeSpec) -> String {
        let mut out = String::with_capacity(32);
        self.write_isoformat(&mut out, spec);
        out
    }

    /// The shim's `_time_out`: `(hour, minute, second, microsecond, offset, fold)`.
    pub fn to_value(self) -> Value {
        Value::tuple(vec![
            Value::int(self.hour),
            Value::int(self.minute),
            Value::int(self.second),
            Value::int(self.microsecond),
            offset_out(self.tz),
            Value::int(self.fold),
        ])
    }
}

/// The shim's `_offset`: whole seconds of the offset (microseconds dropped), or None.
fn offset_out(tz: Option<FixedOffset>) -> Value {
    tz.map_or(Value::None, |tz| {
        let off = tz.utcoffset();
        Value::int(i64::from(off.days()) * 86_400 + i64::from(off.seconds()))
    })
}

/// `(total % 10**6, total / 10**6)` as `i64`s (`i64::MAX` if a part doesn't fit), in
/// `i64` arithmetic when `total` fits one, since `i128` division is a slow library call.
pub(crate) fn split_micros(total: i128) -> (i64, i64) {
    if let Ok(total) = i64::try_from(total) {
        return (total % 1_000_000, total / 1_000_000);
    }
    let part = |n: i128| i64::try_from(n).unwrap_or(i64::MAX);
    (part(total % 1_000_000), part(total / 1_000_000))
}

/// CPython's `format_utcoffset` with `:` separators, appended: `+HH:MM[:SS[.ffffff]]`.
fn write_utcoffset(out: &mut String, offset: TimeDelta) {
    // |offset| < 1 day, so every part fits an i64.
    let total = offset.total_microseconds();
    let (sign, total) = if offset.days() < 0 {
        ('-', -total)
    } else {
        ('+', total)
    };
    let (us, seconds) = split_micros(total);
    let (h, m, s) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    out.push(sign);
    push_padded(out, h, 2);
    out.push(':');
    push_padded(out, m, 2);
    if us != 0 || s != 0 {
        out.push(':');
        push_padded(out, s, 2);
    }
    if us != 0 {
        out.push('.');
        push_padded(out, us, 6);
    }
}

/// The `timespec` argument of `isoformat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimeSpec {
    /// `auto`: microseconds only when nonzero.
    Auto,
    /// `hours`.
    Hours,
    /// `minutes`.
    Minutes,
    /// `seconds`.
    Seconds,
    /// `milliseconds`.
    Milliseconds,
    /// `microseconds`.
    Microseconds,
}

impl TimeSpec {
    /// Parses a timespec the way `isoformat` does: case-sensitive.
    ///
    /// # Errors
    /// `ValueError("Unknown timespec value")`.
    pub fn parse(name: &str) -> PyResult<Self> {
        Ok(match name {
            "auto" => Self::Auto,
            "hours" => Self::Hours,
            "minutes" => Self::Minutes,
            "seconds" => Self::Seconds,
            "milliseconds" => Self::Milliseconds,
            "microseconds" => Self::Microseconds,
            _ => return Err(PyError::value_error("Unknown timespec value")),
        })
    }
}

/// The `s` argument format of `isoformat`'s timespec, then [`TimeSpec::parse`].
fn timespec_arg(value: &Value, position: usize) -> PyResult<TimeSpec> {
    let Value::Str(s) = value else {
        return Err(PyError::type_error(format!(
            "isoformat() argument {position} must be str, not {}",
            arg_type_name(value)
        )));
    };
    let Some(text) = s.to_str() else {
        return Err(surrogate_encode_error(s));
    };
    if text.contains('\0') {
        return Err(PyError::value_error("embedded null character"));
    }
    TimeSpec::parse(&text)
}

/// The type name CPython's argument-format errors print: `None` for None.
fn arg_type_name(value: &Value) -> &str {
    match value {
        Value::None => "None",
        other => other.type_name(),
    }
}

/// The UnicodeEncodeError `s` raises for a lone surrogate. No `PyError` variant exists
/// for it, so it surfaces as its base class, ValueError, with CPython's message.
fn surrogate_encode_error(s: &PyStr) -> PyError {
    let points = s.code_points();
    let position = points
        .iter()
        .position(|&c| (0xD800..=0xDFFF).contains(&c))
        .unwrap_or(0);
    let c = points.get(position).copied().unwrap_or(0);
    PyError::value_error(format!(
        "'utf-8' codec can't encode character '\\u{c:x}' in position {position}: \
         surrogates not allowed"
    ))
}

// ------------------------------------------------------------------ DateTime

/// A `datetime.datetime` with an optional fixed-offset zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DateTime {
    date: Date,
    time: Time,
}

fn date_out_of_range() -> PyError {
    PyError::overflow_error("date value out of range")
}

impl DateTime {
    /// A datetime from a valid date, microseconds since midnight (`0..86_400_000_000`)
    /// and a zone; fold is 0.
    ///
    /// # Errors
    /// `OverflowError("date value out of range")` if `us` is outside a day.
    pub fn from_date_micros(date: Date, us: i64, tz: Option<FixedOffset>) -> PyResult<Self> {
        if !(0..US_PER_DAY).contains(&us) {
            return Err(date_out_of_range());
        }
        Ok(Self {
            date,
            time: Time::from_micros(us, tz)?,
        })
    }

    /// The same fields with `tzinfo` replaced.
    #[must_use]
    pub fn with_tz(self, tz: Option<FixedOffset>) -> Self {
        Self {
            date: self.date,
            time: Time { tz, ..self.time },
        }
    }

    /// The date part.
    pub fn date(self) -> Date {
        self.date
    }

    /// The time part, with the zone and fold.
    pub fn time(self) -> Time {
        self.time
    }

    /// `datetime(year, month, day, ...)` after argument evaluation: `i` conversion of
    /// every field in order, then `check_date_args` and `check_time_args`.
    fn convert(year: &Value, month: &Value, day: &Value, rest: &TimeArgs<'_>) -> PyResult<Self> {
        let y = as_c_int(year)?;
        let m = as_c_int(month)?;
        let d = as_c_int(day)?;
        let hour = as_c_int(rest.hour)?;
        let minute = as_c_int(rest.minute)?;
        let second = as_c_int(rest.second)?;
        let microsecond = as_c_int(rest.microsecond)?;
        let fold = as_c_int(rest.fold)?;
        check_date_args(i64::from(y), i64::from(m), i64::from(d))?;
        check_time_args(hour, minute, second, microsecond, fold)?;
        Ok(Self {
            date: Date::new(i64::from(y), i64::from(m), i64::from(d))?,
            time: Time {
                hour,
                minute,
                second,
                microsecond,
                tz: rest.tz,
                fold,
            },
        })
    }

    /// Microseconds since 0001-01-01T00:00 local time (ordinal 1 is day 0).
    fn local_micros(self) -> i128 {
        i128::from(self.date.to_ordinal()) * i128::from(US_PER_DAY) + i128::from(self.time.micros())
    }

    /// The offset in microseconds, or None for a naive value.
    fn offset_micros(self) -> Option<i128> {
        self.time.tz.map(|tz| tz.utcoffset().total_microseconds())
    }

    /// `self + td`; the result keeps the zone and has fold 0.
    ///
    /// # Errors
    /// `OverflowError("date value out of range")`.
    pub fn add_delta(self, td: TimeDelta) -> PyResult<Self> {
        let us = self.time.micros()
            + i64::from(td.seconds()) * US_PER_SECOND
            + i64::from(td.microseconds());
        let days = i64::from(td.days()) + us.div_euclid(US_PER_DAY);
        let date = self.date.add_days(days)?;
        let time = Time::from_micros(us.rem_euclid(US_PER_DAY), self.time.tz)?;
        Ok(Self { date, time })
    }

    /// `self - other`.
    ///
    /// # Errors
    /// TypeError when exactly one side is naive.
    pub fn subtract(self, other: Self) -> PyResult<TimeDelta> {
        let diff = match (self.offset_micros(), other.offset_micros()) {
            (None, None) => self.local_micros() - other.local_micros(),
            (Some(a), Some(b)) => (self.local_micros() - a) - (other.local_micros() - b),
            _ => {
                return Err(PyError::type_error(
                    "can't subtract offset-naive and offset-aware datetimes",
                ))
            }
        };
        TimeDelta::from_microseconds(&BigInt::from(diff))
    }

    /// Orders two datetimes as `==` then `<` do; fold is ignored.
    ///
    /// # Errors
    /// TypeError when exactly one side is naive (`==` is False, then `<` raises).
    pub fn compare(self, other: Self) -> PyResult<Ordering> {
        match (self.offset_micros(), other.offset_micros()) {
            (None, None) => Ok(self.local_micros().cmp(&other.local_micros())),
            (Some(a), Some(b)) => Ok((self.local_micros() - a).cmp(&(other.local_micros() - b))),
            _ => Err(PyError::type_error(
                "can't compare offset-naive and offset-aware datetimes",
            )),
        }
    }

    /// `isoformat(sep, timespec)`; `sep` is any single code point.
    pub fn isoformat(self, sep: u32, spec: TimeSpec) -> PyStr {
        let mut out = String::with_capacity(48);
        self.date.write_isoformat(&mut out);
        let date_len = out.len();
        self.time.write_isoformat(&mut out, spec);
        // Everything but `sep` is ASCII; `sep` may be any code point, even a surrogate.
        let mut points = Vec::with_capacity(out.len() + 1);
        points.extend(out.as_bytes()[..date_len].iter().map(|&b| u32::from(b)));
        points.push(sep);
        points.extend(out.as_bytes()[date_len..].iter().map(|&b| u32::from(b)));
        PyStr::from_code_points(points)
    }

    /// `repr(dt)`.
    pub fn repr(self) -> String {
        let t = self.time;
        let mut out = format!(
            "datetime.datetime({}, {}, {}, {}, {}",
            self.date.year(),
            self.date.month(),
            self.date.day(),
            t.hour,
            t.minute
        );
        if t.microsecond != 0 {
            let _ = write!(out, ", {}, {}", t.second, t.microsecond);
        } else if t.second != 0 {
            let _ = write!(out, ", {}", t.second);
        }
        if t.fold != 0 {
            let _ = write!(out, ", fold={}", t.fold);
        }
        if let Some(tz) = t.tz {
            let _ = write!(out, ", tzinfo={}", tz.repr());
        }
        out.push(')');
        out
    }

    /// The shim's `_dt_out`.
    pub fn to_value(self) -> Value {
        let t = self.time;
        Value::tuple(vec![
            Value::int(self.date.year()),
            Value::int(self.date.month()),
            Value::int(self.date.day()),
            Value::int(t.hour),
            Value::int(t.minute),
            Value::int(t.second),
            Value::int(t.microsecond),
            offset_out(t.tz),
            Value::int(t.fold),
        ])
    }
}

// ------------------------------------------------------------------ shim inputs

/// The shim's `_time_in(t)`: `time(t[0], t[1], t[2], t[3], _tz(t[4]), fold=t[5])`.
///
/// # Errors
/// Indexing errors and `_tz`'s errors in evaluation order, then the constructor's.
pub fn time_in(t: &Value) -> PyResult<Time> {
    let fields = [item(t, 0)?, item(t, 1)?, item(t, 2)?, item(t, 3)?];
    let tz = offset_in(&*item(t, 4)?)?;
    let fold = item(t, 5)?;
    TimeArgs {
        hour: &fields[0],
        minute: &fields[1],
        second: &fields[2],
        microsecond: &fields[3],
        tz,
        fold: &fold,
    }
    .convert()
}

/// The shim's `_dt_in(d)`: `datetime(d[0], ..., d[6], _tz(d[7]), fold=d[8])`.
///
/// # Errors
/// Indexing errors and `_tz`'s errors in evaluation order, then the constructor's.
pub fn dt_in(d: &Value) -> PyResult<DateTime> {
    let fields = [
        item(d, 0)?,
        item(d, 1)?,
        item(d, 2)?,
        item(d, 3)?,
        item(d, 4)?,
        item(d, 5)?,
        item(d, 6)?,
    ];
    let tz = offset_in(&*item(d, 7)?)?;
    let fold = item(d, 8)?;
    let [year, month, day, hour, minute, second, microsecond] = &fields;
    DateTime::convert(
        year,
        month,
        day,
        &TimeArgs {
            hour,
            minute,
            second,
            microsecond,
            tz,
            fold: &fold,
        },
    )
}

/// `datetime(*fields, tz, fold=fold)` for seven already-evaluated field values.
fn datetime_from_values(
    fields: &[Cow<'_, Value>],
    tz: Option<FixedOffset>,
    fold: &Value,
) -> PyResult<DateTime> {
    let [year, month, day, hour, minute, second, microsecond] = fields else {
        return Err(PyError::type_error("datetime needs seven fields"));
    };
    DateTime::convert(
        year,
        month,
        day,
        &TimeArgs {
            hour,
            minute,
            second,
            microsecond,
            tz,
            fold,
        },
    )
}

// ------------------------------------------------------------------ shim functions

const MAKE_TIME: [Param; 6] = [
    Param::optional("hour", Default::Int(0)),
    Param::optional("minute", Default::Int(0)),
    Param::optional("second", Default::Int(0)),
    Param::optional("microsecond", Default::Int(0)),
    Param::optional("utcoffset", Default::None),
    Param::optional("fold", Default::Int(0)),
];

const MAKE_DATETIME: [Param; 9] = [
    Param::required("year"),
    Param::required("month"),
    Param::required("day"),
    Param::optional("hour", Default::Int(0)),
    Param::optional("minute", Default::Int(0)),
    Param::optional("second", Default::Int(0)),
    Param::optional("microsecond", Default::Int(0)),
    Param::optional("utcoffset", Default::None),
    Param::optional("fold", Default::Int(0)),
];

/// `pc_datetime.make_time`.
///
/// # Errors
/// Binding errors, `_tz`'s errors, then those of `time(...)`.
pub fn make_time(args: &[Value], kwargs: &Kwargs) -> PyResult<Value> {
    let v = crate::timedelta::bind_ref("make_time", &MAKE_TIME, args, kwargs)?;
    let tz = offset_in(&v[4])?;
    let time = TimeArgs {
        hour: &v[0],
        minute: &v[1],
        second: &v[2],
        microsecond: &v[3],
        tz,
        fold: &v[5],
    }
    .convert()?;
    Ok(time.to_value())
}

/// `pc_datetime.make_datetime`.
///
/// # Errors
/// Binding errors, `_tz`'s errors, then those of `datetime(...)`.
pub fn make_datetime(args: &[Value], kwargs: &Kwargs) -> PyResult<Value> {
    let v = crate::timedelta::bind_ref("make_datetime", &MAKE_DATETIME, args, kwargs)?;
    let tz = offset_in(&v[7])?;
    Ok(datetime_from_values(&v[..7], tz, &v[8])?.to_value())
}

/// `pc_datetime.datetime_add`.
///
/// # Errors
/// Input errors, then `OverflowError("date value out of range")`.
pub fn datetime_add(args: &[Value], kwargs: &Kwargs) -> PyResult<Value> {
    let v = crate::timedelta::bind_ref(
        "datetime_add",
        &[Param::required("d"), Param::required("td")],
        args,
        kwargs,
    )?;
    let d = dt_in(&v[0])?;
    let td = td_in(&v[1])?;
    Ok(d.add_delta(td)?.to_value())
}

fn bind_pair(name: &str, args: &[Value], kwargs: &Kwargs) -> PyResult<(DateTime, DateTime)> {
    let v = crate::timedelta::bind_ref(
        name,
        &[Param::required("a"), Param::required("b")],
        args,
        kwargs,
    )?;
    Ok((dt_in(&v[0])?, dt_in(&v[1])?))
}

/// `pc_datetime.datetime_subtract`.
///
/// # Errors
/// Input errors, then TypeError for naive minus aware.
pub fn datetime_subtract(args: &[Value], kwargs: &Kwargs) -> PyResult<Value> {
    let (a, b) = bind_pair("datetime_subtract", args, kwargs)?;
    Ok(a.subtract(b)?.to_value())
}

/// `pc_datetime.datetime_compare`.
///
/// # Errors
/// Input errors, then TypeError for naive against aware.
pub fn datetime_compare(args: &[Value], kwargs: &Kwargs) -> PyResult<Value> {
    let (a, b) = bind_pair("datetime_compare", args, kwargs)?;
    Ok(Value::int(match a.compare(b)? {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }))
}

const D: [Param; 1] = [Param::required("d")];

/// `pc_datetime.datetime_str`.
///
/// # Errors
/// Input errors.
pub fn datetime_str(args: &[Value], kwargs: &Kwargs) -> PyResult<Value> {
    let v = crate::timedelta::bind_ref("datetime_str", &D, args, kwargs)?;
    let d = dt_in(&v[0])?;
    Ok(Value::Str(d.isoformat(u32::from(' '), TimeSpec::Auto)))
}

/// `pc_datetime.datetime_repr`.
///
/// # Errors
/// Input errors.
pub fn datetime_repr(args: &[Value], kwargs: &Kwargs) -> PyResult<Value> {
    let v = crate::timedelta::bind_ref("datetime_repr", &D, args, kwargs)?;
    Ok(Value::str(dt_in(&v[0])?.repr()))
}

/// `pc_datetime.datetime_isoformat`.
///
/// # Errors
/// Input errors, then the `C`/`s` argument-format errors and `Unknown timespec value`.
pub fn datetime_isoformat(args: &[Value], kwargs: &Kwargs) -> PyResult<Value> {
    let v = crate::timedelta::bind_ref(
        "datetime_isoformat",
        &[
            Param::required("d"),
            Param::optional("sep", Default::Str("T")),
            Param::optional("timespec", Default::Str("auto")),
        ],
        args,
        kwargs,
    )?;
    let d = dt_in(&v[0])?;
    let sep = match &*v[1] {
        Value::Str(s) if s.len() == 1 => match s.code_points() {
            [c] => *c,
            _ => 0,
        },
        other => {
            return Err(PyError::type_error(format!(
                "isoformat() argument 1 must be a unicode character, not {}",
                arg_type_name(other)
            )))
        }
    };
    let spec = timespec_arg(&v[2], 2)?;
    Ok(Value::Str(d.isoformat(sep, spec)))
}

/// `pc_datetime.time_isoformat`.
///
/// # Errors
/// Input errors, then the `s` argument-format errors and `Unknown timespec value`.
pub fn time_isoformat(args: &[Value], kwargs: &Kwargs) -> PyResult<Value> {
    let v = crate::timedelta::bind_ref(
        "time_isoformat",
        &[
            Param::required("t"),
            Param::optional("timespec", Default::Str("auto")),
        ],
        args,
        kwargs,
    )?;
    let t = time_in(&v[0])?;
    let spec = timespec_arg(&v[1], 1)?;
    Ok(Value::str(t.isoformat(spec)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dt(fields: [i64; 7], offset: Option<i64>, fold: i64) -> Value {
        let mut items: Vec<Value> = fields.iter().map(|&n| Value::int(n)).collect();
        items.push(offset.map_or(Value::None, Value::int));
        items.push(Value::int(fold));
        Value::tuple(items)
    }

    #[test]
    fn repr_and_isoformat() {
        let d = dt([2024, 2, 29, 13, 5, 9, 123_456], Some(3600), 1);
        assert_eq!(
            datetime_repr(std::slice::from_ref(&d), &[]),
            Ok(Value::str(
                "datetime.datetime(2024, 2, 29, 13, 5, 9, 123456, fold=1, \
                 tzinfo=datetime.timezone(datetime.timedelta(seconds=3600)))"
            ))
        );
        assert_eq!(
            datetime_str(&[d], &[]),
            Ok(Value::str("2024-02-29 13:05:09.123456+01:00"))
        );
    }

    #[test]
    fn compare_across_zones() {
        let a = dt([2000, 1, 1, 1, 0, 0, 0], Some(3600), 0);
        let b = dt([2000, 1, 1, 0, 0, 0, 0], Some(0), 0);
        assert_eq!(datetime_compare(&[a, b], &[]), Ok(Value::int(0)));
    }
}
