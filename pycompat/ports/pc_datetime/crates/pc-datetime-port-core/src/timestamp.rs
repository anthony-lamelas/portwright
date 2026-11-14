//! POSIX timestamps and zone changes: `timestamp()`, `fromtimestamp()`, `astimezone()`.
//!
//! Reference: CPython 3.12 `_datetime` (`datetime_timestamp`, `datetime_from_timestamp`,
//! `datetime_astimezone`) on top of `_PyTime_ObjectToTimeval` and glibc `gmtime_r`.

use num_bigint::BigInt;

use crate::args::Param;
use crate::date::{ord_to_ymd, Date};
use crate::datetime::{dt_in, DateTime};
use crate::error::{PyError, PyResult};
use crate::timedelta::num::true_divide;
use crate::timedelta::{offset_in, FixedOffset};
use crate::{Kwargs, Value};

const SECONDS_PER_DAY: i64 = 86_400;
const US_PER_SECOND: i64 = 1_000_000;
/// Proleptic Gregorian ordinal of 1970-01-01.
const EPOCH_ORDINAL: i64 = 719_163;
/// `2**63` as a double: `time_t` holds `[-2**63, 2**63)`.
const TIME_T_LIMIT: f64 = 9_223_372_036_854_775_808.0;

fn time_t_overflow() -> PyError {
    PyError::overflow_error("timestamp out of range for platform time_t")
}

/// glibc `gmtime_r` failing with `EOVERFLOW` (the year doesn't fit `tm_year`).
///
/// CPython raises `OSError(75, 'Value too large for defined data type')` here.
fn gmtime_overflow() -> PyError {
    PyError::os_error(75, "Value too large for defined data type")
}

/// The UTC epoch, `datetime(1970, 1, 1, tzinfo=timezone.utc)`.
fn epoch() -> PyResult<DateTime> {
    DateTime::from_date_micros(Date::new(1970, 1, 1)?, 0, Some(FixedOffset::default()))
}

/// `dt.timestamp()` for an aware datetime: `(dt - epoch).total_seconds()`, correctly rounded.
///
/// # Errors
/// None in practice; the subtraction's range errors are propagated.
pub fn aware_timestamp(dt: DateTime) -> PyResult<f64> {
    let delta = dt.subtract(epoch()?)?;
    true_divide(
        &delta.total_microseconds_big(),
        &BigInt::from(US_PER_SECOND),
    )
}

/// `_PyTime_ObjectToTimeval(obj, ROUND_HALF_EVEN)`: whole seconds and microseconds.
///
/// # Errors
/// ValueError for NaN, OverflowError outside `time_t`, TypeError for non-numbers.
pub fn to_timeval(value: &Value) -> PyResult<(i64, i64)> {
    match value {
        Value::Float(f) => double_to_timeval(*f),
        Value::Bool(b) => Ok((i64::from(*b), 0)),
        Value::Int(n) => i64::try_from(n)
            .map(|s| (s, 0))
            .map_err(|_| time_t_overflow()),
        other => Err(PyError::type_error(format!(
            "'{}' object cannot be interpreted as an integer",
            other.type_name()
        ))),
    }
}

/// `_PyTime_DoubleToDenominator(d, 1e6, ROUND_HALF_EVEN)`.
fn double_to_timeval(d: f64) -> PyResult<(i64, i64)> {
    if d.is_nan() {
        return Err(PyError::value_error("Invalid value NaN (not a number)"));
    }
    if d.is_infinite() {
        return Err(time_t_overflow());
    }
    // modf: both parts are exact for finite doubles.
    let mut intpart = d.trunc();
    let mut floatpart = ((d - intpart) * 1e6).round_ties_even();
    if floatpart >= 1e6 {
        floatpart -= 1e6;
        intpart += 1.0;
    } else if floatpart < 0.0 {
        floatpart += 1e6;
        intpart -= 1.0;
    }
    if !(-TIME_T_LIMIT..TIME_T_LIMIT).contains(&intpart) {
        return Err(time_t_overflow());
    }
    Ok((f64_to_i64(intpart), f64_to_i64(floatpart)))
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "callers pass integral values already checked to lie within i64"
)]
fn f64_to_i64(f: f64) -> i64 {
    f as i64
}

/// `datetime.fromtimestamp(timestamp, tz)` for a fixed-offset zone.
///
/// # Errors
/// [`to_timeval`]'s errors, then gmtime's overflow, then `year N is out of range`, then
/// `OverflowError("date value out of range")` from shifting into `tz`.
pub fn from_timestamp(timestamp: &Value, tz: FixedOffset) -> PyResult<DateTime> {
    let (seconds, us) = to_timeval(timestamp)?;
    // glibc __offtime: days and seconds of day, then the year in a long, then tm_year.
    let days = seconds.div_euclid(SECONDS_PER_DAY);
    let second_of_day = seconds.rem_euclid(SECONDS_PER_DAY);
    let (year, month, day) = ord_to_ymd(days + EPOCH_ORDINAL);
    let tm_year = i32::try_from(year - 1900).map_err(|_| gmtime_overflow())?;
    // CPython computes `tm_year + 1900` in a C int, which wraps for the largest years.
    let year = tm_year.wrapping_add(1900);
    let date = Date::new(i64::from(year), month, day)?;
    let utc = DateTime::from_date_micros(date, second_of_day * US_PER_SECOND + us, Some(tz))?;
    utc.add_delta(tz.utcoffset())
}

/// `dt.astimezone(tz)` for an aware `dt` and a fixed-offset `tz`.
///
/// # Errors
/// `OverflowError("date value out of range")` when either shift leaves the range.
pub fn astimezone(dt: DateTime, tz: FixedOffset) -> PyResult<DateTime> {
    let own = dt.time().tz();
    let Some(own) = own else {
        // Only aware values reach here; the shim refuses naive ones.
        return Ok(dt);
    };
    // `self_tzinfo is tzinfo`: only `timezone.utc` is a shared singleton.
    if own.is_utc() && tz.is_utc() {
        return Ok(dt);
    }
    let utc = dt.add_delta(own.utcoffset().negate()?)?;
    utc.with_tz(Some(tz)).add_delta(tz.utcoffset())
}

// ------------------------------------------------------------------ shim functions

/// `pc_datetime.datetime_timestamp`.
///
/// # Errors
/// Indexing errors, ValueError for a naive value, then `_dt_in`'s errors.
pub fn datetime_timestamp(args: &[Value], kwargs: &Kwargs) -> PyResult<Value> {
    let v =
        crate::timedelta::bind_ref("datetime_timestamp", &[Param::required("d")], args, kwargs)?;
    if v[0].get_index(7)? == Value::None {
        return Err(PyError::value_error(
            "naive datetimes depend on the local time zone and aren't supported",
        ));
    }
    let dt = dt_in(&v[0])?;
    Ok(Value::Float(aware_timestamp(dt)?))
}

/// `pc_datetime.datetime_fromtimestamp`.
///
/// # Errors
/// ValueError for a missing offset, `_tz`'s errors, then [`from_timestamp`]'s.
pub fn datetime_fromtimestamp(args: &[Value], kwargs: &Kwargs) -> PyResult<Value> {
    let v = crate::timedelta::bind_ref(
        "datetime_fromtimestamp",
        &[Param::required("timestamp"), Param::required("utcoffset")],
        args,
        kwargs,
    )?;
    let Some(tz) = offset_in(&v[1])? else {
        return Err(PyError::value_error(
            "a UTC offset is required; local time isn't supported",
        ));
    };
    Ok(from_timestamp(&v[0], tz)?.to_value())
}

/// `pc_datetime.datetime_astimezone`.
///
/// # Errors
/// Indexing errors, ValueError for a missing offset, `_dt_in`'s and `_tz`'s errors, then
/// [`astimezone`]'s.
pub fn datetime_astimezone(args: &[Value], kwargs: &Kwargs) -> PyResult<Value> {
    let v = crate::timedelta::bind_ref(
        "datetime_astimezone",
        &[Param::required("d"), Param::required("utcoffset")],
        args,
        kwargs,
    )?;
    if v[0].get_index(7)? == Value::None || *v[1] == Value::None {
        return Err(PyError::value_error(
            "both offsets are required; local time isn't supported",
        ));
    }
    let dt = dt_in(&v[0])?;
    let Some(tz) = offset_in(&v[1])? else {
        return Err(PyError::value_error(
            "both offsets are required; local time isn't supported",
        ));
    };
    Ok(astimezone(dt, tz)?.to_value())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounds_half_even() {
        assert_eq!(double_to_timeval(5e-7), Ok((0, 0)));
        assert_eq!(double_to_timeval(1.5e-6), Ok((0, 2)));
        assert_eq!(double_to_timeval(-1.5), Ok((-2, 500_000)));
    }

    #[test]
    fn time_t_edges() {
        assert!(double_to_timeval(TIME_T_LIMIT).is_err());
        assert!(double_to_timeval(-TIME_T_LIMIT).is_ok());
    }

    fn year_error(ts: i64) -> PyResult<DateTime> {
        from_timestamp(&Value::int(ts), FixedOffset::default())
    }

    #[test]
    fn gmtime_year_edges() {
        let msg = |y: &str| Err(PyError::value_error(format!("year {y} is out of range")));
        assert_eq!(year_error(253_402_300_800), msg("10000"));
        assert_eq!(year_error(-62_135_596_801), msg("0"));
        assert_eq!(year_error(67_767_976_233_532_800), msg("-2147483648"));
        assert_eq!(year_error(67_768_036_191_676_799), msg("-2147481749"));
        assert_eq!(year_error(67_768_036_191_676_800), Err(gmtime_overflow()));
        assert_eq!(year_error(-67_768_040_609_740_800), msg("-2147481748"));
        assert_eq!(year_error(-67_768_040_609_740_801), Err(gmtime_overflow()));
    }
}
