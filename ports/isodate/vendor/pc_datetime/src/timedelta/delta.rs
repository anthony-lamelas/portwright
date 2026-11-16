//! The `timedelta` value and its constructor, mirroring CPython's `delta_new`.

use std::borrow::Cow;
use std::fmt;

use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{FromPrimitive, ToPrimitive, Zero};

use crate::error::{PyError, PyResult};
use crate::value::Value;

/// Largest `|days|` a timedelta may hold.
pub const MAX_DELTA_DAYS: i32 = 999_999_999;
const US_PER_SECOND: i64 = 1_000_000;
const SECONDS_PER_DAY: i64 = 86_400;

/// A normalized `datetime.timedelta`: `0 <= seconds < 86400`, `0 <= microseconds < 10**6`
/// and `|days| <= 999999999`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct TimeDelta {
    days: i32,
    seconds: i32,
    microseconds: i32,
}

/// One keyword of the `timedelta` constructor, in the order `delta_new` accumulates them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    /// `microseconds=`.
    Microseconds,
    /// `milliseconds=`.
    Milliseconds,
    /// `seconds=`.
    Seconds,
    /// `minutes=`.
    Minutes,
    /// `hours=`.
    Hours,
    /// `days=`.
    Days,
    /// `weeks=`.
    Weeks,
}

impl Unit {
    fn name(self) -> &'static str {
        match self {
            Self::Microseconds => "microseconds",
            Self::Milliseconds => "milliseconds",
            Self::Seconds => "seconds",
            Self::Minutes => "minutes",
            Self::Hours => "hours",
            Self::Days => "days",
            Self::Weeks => "weeks",
        }
    }

    fn factor(self) -> i64 {
        match self {
            Self::Microseconds => 1,
            Self::Milliseconds => 1_000,
            Self::Seconds => US_PER_SECOND,
            Self::Minutes => 60 * US_PER_SECOND,
            Self::Hours => 3_600 * US_PER_SECOND,
            Self::Days => SECONDS_PER_DAY * US_PER_SECOND,
            Self::Weeks => 7 * SECONDS_PER_DAY * US_PER_SECOND,
        }
    }
}

/// `int(f)` for a float with integral value (or any float: truncates), with CPython's errors.
///
/// # Errors
/// `ValueError` for NaN, `OverflowError` for infinities.
pub fn float_to_int(f: f64) -> PyResult<BigInt> {
    if f.is_nan() {
        return Err(PyError::value_error("cannot convert float NaN to integer"));
    }
    BigInt::from_f64(f.trunc())
        .ok_or_else(|| PyError::overflow_error("cannot convert float infinity to integer"))
}

#[expect(
    clippy::cast_precision_loss,
    reason = "unit factors are below 2**53, so the conversion is exact"
)]
fn factor_f64(factor: i64) -> f64 {
    factor as f64
}

/// CPython's `accum`: adds `num * factor` microseconds to `sofar`, keeping float
/// fractions in `leftover`.
fn accum(unit: Unit, sofar: BigInt, num: &Value, leftover: &mut f64) -> PyResult<BigInt> {
    let factor = unit.factor();
    match num {
        Value::Int(n) => Ok(sofar + n * factor),
        Value::Bool(b) => Ok(sofar + i64::from(*b) * factor),
        Value::Float(f) => {
            let intpart = float_to_int(*f)?;
            let fracpart = f.fract();
            let sum = sofar + intpart * factor;
            if fracpart == 0.0 {
                return Ok(sum);
            }
            let scaled = factor_f64(factor) * fracpart;
            let whole = float_to_int(scaled)?;
            *leftover += scaled.fract();
            Ok(sum + whole)
        }
        other => Err(PyError::type_error(format!(
            "unsupported type for timedelta {} component: {}",
            unit.name(),
            other.type_name()
        ))),
    }
}

/// `datetime.timedelta(**components)`, where `components` lists the keywords that were
/// passed. They're accumulated in `delta_new`'s order, whatever order they're given in.
///
/// # Errors
/// The constructor's TypeError, ValueError and OverflowError.
pub fn new_delta(components: &[(Unit, &Value)]) -> PyResult<TimeDelta> {
    const ORDER: [Unit; 7] = [
        Unit::Microseconds,
        Unit::Milliseconds,
        Unit::Seconds,
        Unit::Minutes,
        Unit::Hours,
        Unit::Days,
        Unit::Weeks,
    ];
    if let Some(delta) = new_delta_small(&ORDER, components)? {
        return Ok(delta);
    }
    new_delta_big(&ORDER, components)
}

/// [`new_delta`] with `BigInt` accumulation: CPython's algorithm for any input.
fn new_delta_big(order: &[Unit], components: &[(Unit, &Value)]) -> PyResult<TimeDelta> {
    let mut total = BigInt::zero();
    let mut leftover = 0.0_f64;
    for &unit in order {
        if let Some((_, value)) = components.iter().find(|(u, _)| *u == unit) {
            total = accum(unit, total, value, &mut leftover)?;
        }
    }
    if leftover != 0.0 {
        let mut whole = leftover.round();
        if (whole - leftover).abs() == 0.5 {
            let odd = if total.is_odd() { 1.0 } else { 0.0 };
            whole = 2.0 * ((leftover + odd) * 0.5).round() - odd;
        }
        total += whole.to_i64().unwrap_or(0);
    }
    TimeDelta::from_microseconds(&total)
}

/// `f.trunc()` as an `i64` when it is finite and fits; `None` otherwise.
fn small_trunc(f: f64) -> Option<i64> {
    // 2**63 is exactly representable; every finite float below it in magnitude truncates
    // to an i64 exactly.
    const LIMIT: f64 = 9_223_372_036_854_775_808.0;
    let t = f.trunc();
    if t.is_finite() && t > -LIMIT && t < LIMIT {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "t is integral and strictly within the i64 range"
        )]
        Some(t as i64)
    } else {
        None
    }
}

/// [`new_delta`] in `i128`, for the common case where every component is small.
/// Returns `Ok(None)` when a value or the running total leaves the fast range; the
/// caller then redoes the whole computation with `BigInt`. Errors are raised exactly
/// where the `BigInt` path raises them, since both visit the units in the same order and
/// the fast path only gives up, never fails, on a value the slow path accepts.
fn new_delta_small(order: &[Unit], components: &[(Unit, &Value)]) -> PyResult<Option<TimeDelta>> {
    let mut total: i128 = 0;
    let mut leftover = 0.0_f64;
    for &unit in order {
        let Some((_, num)) = components.iter().find(|(u, _)| *u == unit) else {
            continue;
        };
        let factor = i128::from(unit.factor());
        let add = match num {
            Value::Int(n) => match n.to_i64() {
                Some(n) => i128::from(n) * factor,
                None => return Ok(None),
            },
            Value::Bool(b) => i128::from(*b) * factor,
            Value::Float(f) => {
                if f.is_nan() {
                    return Err(PyError::value_error("cannot convert float NaN to integer"));
                }
                if f.is_infinite() {
                    return Err(PyError::overflow_error(
                        "cannot convert float infinity to integer",
                    ));
                }
                let Some(intpart) = small_trunc(*f) else {
                    return Ok(None);
                };
                let fracpart = f.fract();
                let mut add = i128::from(intpart) * factor;
                if fracpart != 0.0 {
                    let scaled = factor_f64(unit.factor()) * fracpart;
                    // |scaled| < factor <= 6.048e11, so it always fits.
                    let Some(whole) = small_trunc(scaled) else {
                        return Ok(None);
                    };
                    leftover += scaled.fract();
                    add += i128::from(whole);
                }
                add
            }
            other => {
                return Err(PyError::type_error(format!(
                    "unsupported type for timedelta {} component: {}",
                    unit.name(),
                    other.type_name()
                )))
            }
        };
        let Some(sum) = total.checked_add(add) else {
            return Ok(None);
        };
        total = sum;
    }
    if leftover != 0.0 {
        let mut whole = leftover.round();
        if (whole - leftover).abs() == 0.5 {
            let odd = if total.is_odd() { 1.0 } else { 0.0 };
            whole = 2.0 * ((leftover + odd) * 0.5).round() - odd;
        }
        let Some(sum) = total.checked_add(i128::from(whole.to_i64().unwrap_or(0))) else {
            return Ok(None);
        };
        total = sum;
    }
    TimeDelta::from_microseconds_i128(total).map(Some)
}

fn as_c_int(n: &BigInt) -> PyResult<i32> {
    n.to_i32()
        .ok_or_else(|| PyError::overflow_error("Python int too large to convert to C int"))
}

impl TimeDelta {
    /// The zero timedelta.
    pub const ZERO: Self = Self {
        days: 0,
        seconds: 0,
        microseconds: 0,
    };

    /// CPython's `microseconds_to_delta`.
    ///
    /// # Errors
    /// OverflowError when the days don't fit a C int or exceed 999999999 in magnitude.
    pub fn from_microseconds(us: &BigInt) -> PyResult<Self> {
        if let Some(us) = us.to_i128() {
            return Self::from_microseconds_i128(us);
        }
        let (secs, micro) = us.div_mod_floor(&BigInt::from(US_PER_SECOND));
        let (days, secs) = secs.div_mod_floor(&BigInt::from(SECONDS_PER_DAY));
        let days = as_c_int(&days)?;
        Self::new(days, as_c_int(&secs)?, as_c_int(&micro)?)
    }

    /// [`Self::from_microseconds`] for an `i128` total, without `BigInt` arithmetic.
    ///
    /// # Errors
    /// The same OverflowErrors as [`Self::from_microseconds`].
    pub fn from_microseconds_i128(us: i128) -> PyResult<Self> {
        let (secs, micro) = (
            us.div_euclid(i128::from(US_PER_SECOND)),
            us.rem_euclid(i128::from(US_PER_SECOND)),
        );
        let (days, secs) = (
            secs.div_euclid(i128::from(SECONDS_PER_DAY)),
            secs.rem_euclid(i128::from(SECONDS_PER_DAY)),
        );
        let small = |n: i128| {
            i32::try_from(n)
                .map_err(|_| PyError::overflow_error("Python int too large to convert to C int"))
        };
        let days = small(days)?;
        Self::new(days, small(secs)?, small(micro)?)
    }

    /// CPython's `new_delta` with normalized seconds and microseconds.
    ///
    /// # Errors
    /// OverflowError `days=N; must have magnitude <= 999999999`.
    fn new(days: i32, seconds: i32, microseconds: i32) -> PyResult<Self> {
        if days.unsigned_abs() > MAX_DELTA_DAYS.unsigned_abs() {
            return Err(PyError::overflow_error(format!(
                "days={days}; must have magnitude <= {MAX_DELTA_DAYS}"
            )));
        }
        Ok(Self {
            days,
            seconds,
            microseconds,
        })
    }

    /// `td.days`.
    pub fn days(self) -> i32 {
        self.days
    }

    /// `td.seconds`, in `0..86400`.
    pub fn seconds(self) -> i32 {
        self.seconds
    }

    /// `td.microseconds`, in `0..1000000`.
    pub fn microseconds(self) -> i32 {
        self.microseconds
    }

    /// The whole span in microseconds (always fits: |value| < 2**67).
    pub fn total_microseconds(self) -> i128 {
        (i128::from(self.days) * i128::from(SECONDS_PER_DAY) + i128::from(self.seconds))
            * i128::from(US_PER_SECOND)
            + i128::from(self.microseconds)
    }

    /// [`Self::total_microseconds`] as a `BigInt`.
    pub fn total_microseconds_big(self) -> BigInt {
        BigInt::from(self.total_microseconds())
    }

    /// `bool(td)`.
    pub fn is_zero(self) -> bool {
        self == Self::ZERO
    }

    /// `-td`.
    ///
    /// # Errors
    /// OverflowError when the result is out of range.
    pub fn negate(self) -> PyResult<Self> {
        Self::from_microseconds(&-self.total_microseconds_big())
    }

    /// `(days, seconds, microseconds)` as the shim returns it.
    pub fn to_value(self) -> Value {
        Value::tuple(vec![
            Value::int(self.days),
            Value::int(self.seconds),
            Value::int(self.microseconds),
        ])
    }

    /// `repr(td)`, such as `datetime.timedelta(days=-1, seconds=86399)`.
    pub fn repr(self) -> String {
        let mut parts = Vec::new();
        if self.days != 0 {
            parts.push(format!("days={}", self.days));
        }
        if self.seconds != 0 {
            parts.push(format!("seconds={}", self.seconds));
        }
        if self.microseconds != 0 {
            parts.push(format!("microseconds={}", self.microseconds));
        }
        if parts.is_empty() {
            "datetime.timedelta(0)".to_owned()
        } else {
            format!("datetime.timedelta({})", parts.join(", "))
        }
    }
}

impl fmt::Display for TimeDelta {
    /// `str(td)`, such as `-1 day, 23:59:59.000001`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let minutes = self.seconds / 60;
        let (hours, minutes, seconds) = (minutes / 60, minutes % 60, self.seconds % 60);
        if self.days != 0 {
            let plural = if self.days.abs() == 1 { "" } else { "s" };
            write!(f, "{} day{plural}, ", self.days)?;
        }
        write!(f, "{hours}:{minutes:02}:{seconds:02}")?;
        if self.microseconds != 0 {
            write!(f, ".{:06}", self.microseconds)?;
        }
        Ok(())
    }
}

/// The shim's `_td_in(td)`: `timedelta(days=td[0], seconds=td[1], microseconds=td[2])`.
///
/// # Errors
/// Indexing errors from `td`, then the constructor's errors.
pub fn td_in(td: &Value) -> PyResult<TimeDelta> {
    let days = item(td, 0)?;
    let seconds = item(td, 1)?;
    let microseconds = item(td, 2)?;
    new_delta(&[
        (Unit::Days, &days),
        (Unit::Seconds, &seconds),
        (Unit::Microseconds, &microseconds),
    ])
}

/// `v[index]` like [`Value::get_index`], borrowing tuple and list items instead of
/// cloning them. Other values (and out-of-range indexes) go through `get_index`, so
/// the result and errors are the same.
///
/// # Errors
/// [`Value::get_index`]'s.
pub(crate) fn item(v: &Value, index: usize) -> PyResult<Cow<'_, Value>> {
    match v {
        Value::Tuple(items) | Value::List(items) if index < items.len() => {
            Ok(Cow::Borrowed(&items[index]))
        }
        _ => v.get_index(index).map(Cow::Owned),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn str_and_repr() {
        let td = TimeDelta::from_microseconds(&BigInt::from(-1)).unwrap_or_default();
        assert_eq!(td.to_string(), "-1 day, 23:59:59.999999");
        assert_eq!(
            td.repr(),
            "datetime.timedelta(days=-1, seconds=86399, microseconds=999999)"
        );
        assert_eq!(TimeDelta::ZERO.repr(), "datetime.timedelta(0)");
        assert_eq!(TimeDelta::ZERO.to_string(), "0:00:00");
    }

    #[test]
    fn small_path_matches_bigint_path() {
        const ORDER: [Unit; 7] = [
            Unit::Microseconds,
            Unit::Milliseconds,
            Unit::Seconds,
            Unit::Minutes,
            Unit::Hours,
            Unit::Days,
            Unit::Weeks,
        ];
        let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let specials = [
            0.5,
            1.5,
            2.5,
            -0.5,
            -2.5,
            0.1,
            86_399.999_999_5,
            1e15,
            -1e15,
            1e18,
            9.3e18,
            -9.3e18,
            1e300,
            f64::NAN,
            f64::INFINITY,
            -0.0,
        ];
        for _ in 0..20_000 {
            let values: Vec<Value> = (0..7)
                .map(|_| {
                    let r = next();
                    match r % 6 {
                        0 => Value::Float(
                            specials[usize::try_from(r >> 8).unwrap_or(0) % specials.len()],
                        ),
                        1 => Value::Float(f64::from_bits(next()) % 1e12),
                        2 => Value::int(i64::from_ne_bytes(next().to_ne_bytes()) >> (r >> 58)),
                        3 => Value::Float(
                            f64::from(i32::from_ne_bytes([1, 2, 3, (r >> 8) as u8])) / 64.0,
                        ),
                        4 => Value::Bool(r & 256 != 0),
                        _ => Value::int(BigInt::from(1u8) << 70usize),
                    }
                })
                .collect();
            let count = usize::try_from(next() % 8).unwrap_or(0);
            let comps: Vec<(Unit, &Value)> =
                ORDER.iter().copied().zip(&values).take(count).collect();
            let fast = new_delta_small(&ORDER, &comps);
            let slow = new_delta_big(&ORDER, &comps);
            match fast {
                Ok(Some(td)) => assert_eq!(Ok(td), slow, "{comps:?}"),
                Ok(None) => {}
                Err(e) => assert_eq!(Err(e), slow, "{comps:?}"),
            }
        }
    }

    #[test]
    fn float_leftovers_round_half_even() {
        let half = Value::Float(0.5);
        let td = new_delta(&[(Unit::Microseconds, &half)]);
        assert_eq!(td, Ok(TimeDelta::ZERO));
        let one_half = Value::Float(1.5);
        let td = new_delta(&[(Unit::Microseconds, &one_half)]);
        assert_eq!(td.map(TimeDelta::microseconds), Ok(2));
    }
}
