//! Proleptic Gregorian ordinals, ported from CPython's `_datetimemodule.c`.

use crate::error::Error;
use crate::types::{is_leap, Date};

const DAYS_BEFORE_MONTH: [i64; 13] = [0, 0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];

fn days_before_year(year: i64) -> i64 {
    let y = year - 1;
    y * 365 + y.div_euclid(4) - y.div_euclid(100) + y.div_euclid(400)
}

fn ymd_to_ord(year: i64, month: i64, day: i64) -> i64 {
    let idx = usize::try_from(month).unwrap_or(0).min(12);
    let leap = i64::from(month > 2 && is_leap(year));
    days_before_year(year) + DAYS_BEFORE_MONTH[idx] + leap + day
}

/// `date.toordinal()`.
pub(super) fn to_ordinal(d: &Date) -> i64 {
    ymd_to_ord(i64::from(d.year), i64::from(d.month), i64::from(d.day))
}

/// `date.weekday()` (Monday = 0) of an ordinal.
pub(super) fn weekday(ordinal: i64) -> i64 {
    (ordinal + 6).rem_euclid(7)
}

fn ord_to_ymd(ordinal: i64) -> (i64, i64, i64) {
    let n = ordinal - 1;
    let (n400, n) = (n.div_euclid(146_097), n.rem_euclid(146_097));
    let mut year = n400 * 400 + 1;
    let (n100, n) = (n / 36_524, n % 36_524);
    let (n4, n) = (n / 1_461, n % 1_461);
    let (n1, n) = (n / 365, n % 365);
    year += n100 * 100 + n4 * 4 + n1;
    if n1 == 4 || n100 == 4 {
        return (year - 1, 12, 31);
    }
    let leap = n1 == 3 && (n4 != 24 || n100 == 3);
    let mut month = (n + 50) >> 5;
    let before = |m: i64| {
        DAYS_BEFORE_MONTH[usize::try_from(m).unwrap_or(0).min(12)] + i64::from(m > 2 && leap)
    };
    let mut preceding = before(month);
    if preceding > n {
        month -= 1;
        preceding = before(month);
    }
    (year, month, n - preceding + 1)
}

/// `date.fromordinal(n)`.
pub(super) fn from_ordinal(ordinal: i64) -> Result<Date, Error> {
    if ordinal < 1 {
        return Err(Error::Value("ordinal must be >= 1".into()));
    }
    let (y, m, d) = ord_to_ymd(ordinal);
    Date::new(y, m, d)
}

/// `date.fromisocalendar(year, week, day)`.
pub(super) fn from_iso_calendar(year: i64, week: i64, day: i64) -> Result<Date, Error> {
    if !(1..=9999).contains(&year) {
        return Err(Error::Value(format!("Year is out of range: {year}")));
    }
    if !(1..53).contains(&week) {
        let mut out_of_range = true;
        if week == 53 {
            let first_weekday = ymd_to_ord(year, 1, 1) % 7;
            if first_weekday == 4 || (first_weekday == 3 && is_leap(year)) {
                out_of_range = false;
            }
        }
        if out_of_range {
            return Err(Error::Value(format!("Invalid week: {week}")));
        }
    }
    if !(1..8).contains(&day) {
        return Err(Error::Value(format!(
            "Invalid day: {day} (range is [1, 7])"
        )));
    }
    let first_day = ymd_to_ord(year, 1, 1);
    let first_weekday = (first_day + 6) % 7;
    let mut week1_monday = first_day - first_weekday;
    if first_weekday > 3 {
        week1_monday += 7;
    }
    let (y, m, d) = ord_to_ymd(week1_monday + (week - 1) * 7 + day - 1);
    Date::new(y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::days_in_month;

    #[test]
    fn ordinal_roundtrip() {
        for ord in [1, 59, 60, 365, 366, 730_000, 738_000, 3_652_059] {
            let d = from_ordinal(ord).unwrap();
            assert_eq!(to_ordinal(&d), ord);
            assert!(i64::from(d.day) <= days_in_month(i64::from(d.year), i64::from(d.month)));
        }
        assert_eq!(to_ordinal(&Date::new(2020, 3, 1).unwrap()), 737_485);
    }
}
