//! Value types shared by the converters and the PyO3 layer: a faithful
//! `timedelta`, `FixedTzOffset`, and date/time values validated with
//! CPython's `datetime` error messages.

use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::ToPrimitive;

use crate::error::Error;

const US_PER_SECOND: i64 = 1_000_000;
const SECONDS_PER_DAY: i64 = 86_400;
const MAX_DAYS: i64 = 999_999_999;

/// A normalized `datetime.timedelta`: `0 <= seconds < 86400`,
/// `0 <= microseconds < 1_000_000`, `|days| <= 999_999_999`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct TimeDelta {
    days: i64,
    seconds: i64,
    microseconds: i64,
}

impl TimeDelta {
    /// `timedelta(0)`.
    pub const ZERO: TimeDelta = TimeDelta {
        days: 0,
        seconds: 0,
        microseconds: 0,
    };

    /// Build from a total number of microseconds, with CPython's range check.
    pub fn from_microseconds(total: &BigInt) -> Result<Self, Error> {
        let us_per_day = BigInt::from(SECONDS_PER_DAY * US_PER_SECOND);
        let (days, rem) = total.div_mod_floor(&us_per_day);
        let rem = rem.to_i64().unwrap_or(0);
        let days = match days.to_i64() {
            Some(d) if d.abs() <= MAX_DAYS => d,
            _ => {
                return Err(Error::Overflow(format!(
                    "days={days}; must have magnitude <= {MAX_DAYS}"
                )))
            }
        };
        Ok(Self {
            days,
            seconds: rem / US_PER_SECOND,
            microseconds: rem % US_PER_SECOND,
        })
    }

    /// `timedelta(minutes=m)` for an integer `m`.
    pub fn from_minutes_int(minutes: &BigInt) -> Result<Self, Error> {
        Self::from_microseconds(&(minutes * BigInt::from(60 * US_PER_SECOND)))
    }

    /// `timedelta(minutes=m)` for a float `m`, with CPython's rounding
    /// (round-half-even on the leftover fraction of a microsecond).
    pub fn from_minutes_float(minutes: f64) -> Result<Self, Error> {
        if minutes.is_nan() {
            return Err(Error::Value("cannot convert float NaN to integer".into()));
        }
        if minutes.is_infinite() {
            return Err(Error::Overflow(
                "cannot convert float infinity to integer".into(),
            ));
        }
        let factor = (60 * US_PER_SECOND) as f64;
        let int_part = minutes.trunc();
        let frac_part = minutes - int_part;
        let mut sum = float_to_bigint(int_part) * BigInt::from(60 * US_PER_SECOND);
        let mut leftover = 0.0;
        if frac_part != 0.0 {
            let dnum = frac_part * factor;
            let i2 = dnum.trunc();
            sum += float_to_bigint(i2);
            leftover = dnum - i2;
        }
        if leftover != 0.0 {
            let mut whole = leftover.round();
            if (whole - leftover).abs() == 0.5 {
                // Round half to even, judged together with the integral sum.
                let odd = if sum.is_odd() { 1.0 } else { 0.0 };
                whole = 2.0 * ((leftover + odd) * 0.5).round() - odd;
            }
            sum += float_to_bigint(whole);
        }
        Self::from_microseconds(&sum)
    }

    /// The `days` attribute.
    pub fn days(&self) -> i64 {
        self.days
    }

    /// The `seconds` attribute (`0..86400`).
    pub fn seconds(&self) -> i64 {
        self.seconds
    }

    /// The `microseconds` attribute (`0..1_000_000`).
    pub fn microseconds(&self) -> i64 {
        self.microseconds
    }

    /// Total microseconds (always fits: |days| <= 999_999_999).
    pub fn total_microseconds(&self) -> i128 {
        (i128::from(self.days) * i128::from(SECONDS_PER_DAY) + i128::from(self.seconds))
            * i128::from(US_PER_SECOND)
            + i128::from(self.microseconds)
    }
}

fn float_to_bigint(f: f64) -> BigInt {
    use num_traits::FromPrimitive;
    BigInt::from_f64(f.trunc()).unwrap_or_default()
}

impl std::fmt::Display for TimeDelta {
    /// `str(timedelta)`, e.g. `-1 day, 23:30:00` or `0:01:30.500000`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.days != 0 {
            let plural = if self.days.abs() != 1 { "s" } else { "" };
            write!(f, "{} day{}, ", self.days, plural)?;
        }
        let (h, rem) = (self.seconds / 3600, self.seconds % 3600);
        write!(f, "{}:{:02}:{:02}", h, rem / 60, rem % 60)?;
        if self.microseconds != 0 {
            write!(f, ".{:06}", self.microseconds)?;
        }
        Ok(())
    }
}

/// Core of `parse.FixedTzOffset`: an offset and a display name. The PyO3
/// class keeps the original Python `name` object; this holds `str(name)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixedTzOffset {
    /// `timedelta(minutes=offset)`.
    pub offset: TimeDelta,
    /// The zone name.
    pub name: String,
}

impl FixedTzOffset {
    /// `repr()`: `<FixedTzOffset {name} {offset}>`.
    pub fn repr(&self) -> String {
        format!("<FixedTzOffset {} {}>", self.name, self.offset)
    }
}

/// The `tzinfo` attached to a parsed date/time value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TzInfo {
    /// `parse.FixedTzOffset` (from `date_convert`).
    Fixed(FixedTzOffset),
    /// `datetime.timezone(offset)` (from `strptime`'s `%z`).
    Timezone(TimeDelta),
}

/// A validated `datetime.date`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Date {
    /// 1..=9999
    pub year: i32,
    /// 1..=12
    pub month: u32,
    /// 1..=days_in_month
    pub day: u32,
}

/// `True` for leap years in the proleptic Gregorian calendar.
pub fn is_leap(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

/// Days in `month` of `year` (month must be 1..=12).
pub fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn c_int(v: i64) -> Result<i32, Error> {
    i32::try_from(v).map_err(|_| {
        if v > 0 {
            Error::Overflow("signed integer is greater than maximum".into())
        } else {
            Error::Overflow("signed integer is less than minimum".into())
        }
    })
}

impl Date {
    /// `datetime.date(year, month, day)` with CPython's checks and messages.
    pub fn new(year: i64, month: i64, day: i64) -> Result<Self, Error> {
        let (y, m, d) = (c_int(year)?, c_int(month)?, c_int(day)?);
        if !(1..=9999).contains(&y) {
            return Err(Error::Value(format!("year {y} is out of range")));
        }
        if !(1..=12).contains(&m) {
            return Err(Error::Value("month must be in 1..12".into()));
        }
        if d < 1 || i64::from(d) > days_in_month(year, month) {
            return Err(Error::Value("day is out of range for month".into()));
        }
        Ok(Self {
            year: y,
            month: m as u32,
            day: d as u32,
        })
    }
}

/// A validated `datetime.time` (with optional tzinfo).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Time {
    /// 0..=23
    pub hour: u32,
    /// 0..=59
    pub minute: u32,
    /// 0..=59
    pub second: u32,
    /// 0..=999_999
    pub microsecond: u32,
    /// `tzinfo`
    pub tz: Option<TzInfo>,
}

impl Time {
    /// `datetime.time(h, m, s, us, tzinfo=tz)` with CPython's checks and messages.
    pub fn new(
        hour: i64,
        minute: i64,
        second: i64,
        microsecond: i64,
        tz: Option<TzInfo>,
    ) -> Result<Self, Error> {
        let (h, mi, s, us) = (
            c_int(hour)?,
            c_int(minute)?,
            c_int(second)?,
            c_int(microsecond)?,
        );
        if !(0..=23).contains(&h) {
            return Err(Error::Value("hour must be in 0..23".into()));
        }
        if !(0..=59).contains(&mi) {
            return Err(Error::Value("minute must be in 0..59".into()));
        }
        if !(0..=59).contains(&s) {
            return Err(Error::Value("second must be in 0..59".into()));
        }
        if !(0..=999_999).contains(&us) {
            return Err(Error::Value("microsecond must be in 0..999999".into()));
        }
        Ok(Self {
            hour: h as u32,
            minute: mi as u32,
            second: s as u32,
            microsecond: us as u32,
            tz,
        })
    }
}

/// A validated `datetime.datetime`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DateTime {
    /// Date part.
    pub date: Date,
    /// Time part, including tzinfo.
    pub time: Time,
}

impl DateTime {
    /// `datetime.datetime(y, m, d, H, M, S, us, tzinfo=tz)`; date checked first.
    #[allow(clippy::too_many_arguments, reason = "mirrors datetime()")]
    pub fn new(
        year: i64,
        month: i64,
        day: i64,
        hour: i64,
        minute: i64,
        second: i64,
        microsecond: i64,
        tz: Option<TzInfo>,
    ) -> Result<Self, Error> {
        let date = Date::new(year, month, day)?;
        let time = Time::new(hour, minute, second, microsecond, tz)?;
        Ok(Self { date, time })
    }
}

/// What a date/time converter returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Temporal {
    /// `datetime.datetime`
    DateTime(DateTime),
    /// `datetime.date`
    Date(Date),
    /// `datetime.time`
    Time(Time),
}

/// `datetime.today().year` (local time), used where upstream defaults the year.
pub fn current_year() -> i64 {
    use chrono::Datelike;
    i64::from(chrono::Local::now().year())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timedelta_str() {
        let td = TimeDelta::from_minutes_int(&BigInt::from(-30)).unwrap();
        assert_eq!(td.to_string(), "-1 day, 23:30:00");
        let td = TimeDelta::from_minutes_float(1.5).unwrap();
        assert_eq!(td.to_string(), "0:01:30");
        let td = TimeDelta::from_minutes_float(90.25).unwrap();
        assert_eq!(td.to_string(), "1:30:15");
        let td = TimeDelta::from_minutes_int(&BigInt::from(3000)).unwrap();
        assert_eq!(td.to_string(), "2 days, 2:00:00");
    }

    #[test]
    fn date_checks() {
        assert!(Date::new(2020, 2, 29).is_ok());
        assert_eq!(
            Date::new(2019, 2, 29).unwrap_err(),
            Error::Value("day is out of range for month".into())
        );
        assert_eq!(
            Date::new(0, 1, 1).unwrap_err(),
            Error::Value("year 0 is out of range".into())
        );
    }
}
