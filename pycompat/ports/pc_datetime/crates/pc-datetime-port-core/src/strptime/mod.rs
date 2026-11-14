//! `datetime.strptime(text, fmt)`: a port of CPython 3.12's `_strptime` in the C locale
//! with TZ=UTC.

mod pattern;
mod text;

use std::borrow::Cow;

use crate::args::Param;
use crate::date::{check_date_args, Date};
use crate::error::{PyError, PyResult};
use crate::pystr::PyStr;
use crate::timedelta::{FixedOffset, TimeDelta};
use crate::{Kwargs, Value};

use text::{py_int, to_wtf8, wtf8_len};

const PARAMS: [Param; 2] = [Param::required("text"), Param::required("fmt")];

const F_WEEKDAY: [&str; 7] = [
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
];
const A_WEEKDAY: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
const F_MONTH: [&str; 13] = [
    "",
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];
const A_MONTH: [&str; 13] = [
    "", "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];

/// `pc_datetime.strptime(text, fmt)`: the datetime tuple
/// `(year, month, day, hour, minute, second, microsecond, utcoffset, fold)`.
///
/// # Errors
/// The TypeErrors of argument parsing, `_strptime`'s ValueErrors, `re.error` for a
/// repeated directive, and the date/timezone/datetime constructors' ValueErrors.
pub fn strptime(args: &[Value], kwargs: &Kwargs) -> PyResult<Value> {
    let v = crate::timedelta::bind_ref("strptime", &PARAMS, args, kwargs)?;
    let text = str_arg(&v[0], 1)?;
    let format = str_arg(&v[1], 2)?;
    let parsed = parse(text, format)?;
    parsed.into_datetime()
}

/// The `U` argument format of `datetime.strptime`.
fn str_arg(value: &Value, position: usize) -> PyResult<&PyStr> {
    match value {
        Value::Str(s) => Ok(s),
        other => {
            let name = match other {
                Value::None => "None",
                _ => other.type_name(),
            };
            Err(PyError::type_error(format!(
                "strptime() argument {position} must be str, not {name}"
            )))
        }
    }
}

/// `list.index(value.lower())` on a locale name list. ASCII text lowercases the same
/// way in both forms, so it is compared without allocating; other text (and every
/// miss, for the error message) goes through `str.lower()`.
fn index_of_lower(list: &[&str], value: &str) -> PyResult<i64> {
    if value.is_ascii() {
        if let Some(position) = list
            .iter()
            .position(|item| item.eq_ignore_ascii_case(value))
        {
            return i64::try_from(position)
                .map_err(|_| PyError::overflow_error("index out of range"));
        }
    }
    index_of(list, &value.to_lowercase())
}

/// `list.index(value)` on a locale name list.
fn index_of(list: &[&str], value: &str) -> PyResult<i64> {
    let position = list.iter().position(|&item| item == value).ok_or_else(|| {
        PyError::value_error(format!("{} is not in list", PyStr::from(value).repr()))
    })?;
    i64::try_from(position).map_err(|_| PyError::overflow_error("index out of range"))
}

fn points(s: &str) -> Vec<u32> {
    s.chars().map(u32::from).collect()
}

fn int_of(s: &str) -> PyResult<i64> {
    // Plain ASCII digits (what the directives almost always capture) parse the same
    // way `int()` does; anything else takes the full Unicode path.
    if !s.is_empty() && s.len() <= 18 && s.bytes().all(|b| b.is_ascii_digit()) {
        return Ok(s.bytes().fold(0, |n, b| n * 10 + i64::from(b - b'0')));
    }
    py_int(&points(s))
}

/// What `_strptime._strptime` returns, before the datetime is built.
#[derive(Debug, Default)]
struct Parsed {
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
    fraction: i64,
    /// `gmtoff` in microseconds (`gmtoff * 10**6 + gmtoff_fraction`).
    offset_us: Option<i64>,
}

/// `calendar.isleap`, with Python's floor modulo.
fn is_leap(year: i64) -> bool {
    year.rem_euclid(4) == 0 && (year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0)
}

/// `_calc_julian_from_U_or_W`.
fn julian_from_u_or_w(
    year: i64,
    week_of_year: i64,
    day_of_week: i64,
    week_starts_mon: bool,
) -> PyResult<i64> {
    let mut first_weekday = Date::new(year, 1, 1)?.weekday();
    let mut day_of_week = day_of_week;
    if !week_starts_mon {
        first_weekday = (first_weekday + 1).rem_euclid(7);
        day_of_week = (day_of_week + 1).rem_euclid(7);
    }
    let week_0_length = (7 - first_weekday).rem_euclid(7);
    if week_of_year == 0 {
        Ok(1 + day_of_week - first_weekday)
    } else {
        Ok(1 + week_0_length + 7 * (week_of_year - 1) + day_of_week)
    }
}

/// The `%z` group: the offset in microseconds.
fn parse_offset(found: &str) -> PyResult<i64> {
    if found == "Z" {
        return Ok(0);
    }
    let mut z: Vec<u32> = points(found);
    if z.get(3) == Some(&u32::from(':')) {
        z.remove(3);
        if z.len() > 5 {
            if z.get(5) != Some(&u32::from(':')) {
                return Err(PyError::value_error(format!(
                    "Inconsistent use of : in {found}"
                )));
            }
            z.remove(5);
        }
    }
    let slice = |a: usize, b: usize| -> &[u32] {
        let a = a.min(z.len());
        &z[a..b.min(z.len()).max(a)]
    };
    let hours = py_int(slice(1, 3))?;
    let minutes = py_int(slice(3, 5))?;
    let seconds_text = slice(5, 7);
    let seconds = if seconds_text.is_empty() {
        0
    } else {
        py_int(seconds_text)?
    };
    let mut remainder = slice(8, z.len()).to_vec();
    while remainder.len() < 6 {
        remainder.push(u32::from('0'));
    }
    let fraction = py_int(&remainder)?;
    let total = (hours * 3600 + minutes * 60 + seconds) * 1_000_000 + fraction;
    Ok(if z.first() == Some(&u32::from('-')) {
        -total
    } else {
        total
    })
}

/// `_strptime._strptime(text, format)`.
fn parse(text: &PyStr, format: &PyStr) -> PyResult<Parsed> {
    let compiled = pattern::compile(format)?;
    let data = to_wtf8(text.code_points());
    let Some(found) = compiled.regex.captures(&data) else {
        return Err(PyError::value_error(format!(
            "time data {} does not match format {}",
            text.repr(),
            format.repr()
        )));
    };
    let end = found.get(0).map_or(0, |m| m.end());
    if end != data.len() {
        let rest = &text.code_points()[wtf8_len(&data[..end])..];
        let mut message = PyStr::from("unconverted data remains: ");
        message.push_str(&PyStr::from_code_points(rest.to_vec()));
        return Err(PyError::ValueError(message));
    }
    let groups: Vec<(&str, Cow<str>)> = compiled
        .names
        .iter()
        .filter_map(|(index, name)| {
            let value = found.get(*index)?;
            Some((name.as_str(), String::from_utf8_lossy(value.as_bytes())))
        })
        .collect();
    let group = |key: &str| {
        groups
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| value.as_ref())
    };

    let mut iso_year = None;
    let mut year = None;
    let mut month = 1;
    let mut day = 1;
    let (mut hour, mut minute, mut second, mut fraction) = (0, 0, 0, 0);
    let mut offset_us = None;
    let mut iso_week = None;
    let mut week_of_year = None;
    let mut week_starts_mon = false;
    let mut weekday: Option<i64> = None;
    let mut julian: Option<i64> = None;
    for (key, value) in &groups {
        let value = value.as_ref();
        match *key {
            "y" => {
                let y = int_of(value)?;
                year = Some(if y <= 68 { y + 2000 } else { y + 1900 });
            }
            "Y" => year = Some(int_of(value)?),
            "G" => iso_year = Some(int_of(value)?),
            "m" => month = int_of(value)?,
            "B" => month = index_of_lower(&F_MONTH, value)?,
            "b" => month = index_of_lower(&A_MONTH, value)?,
            "d" => day = int_of(value)?,
            "H" => hour = int_of(value)?,
            "I" => {
                hour = int_of(value)?;
                let ampm = group("p").unwrap_or("").to_lowercase();
                if ampm.is_empty() || ampm == "am" {
                    if hour == 12 {
                        hour = 0;
                    }
                } else if ampm == "pm" && hour != 12 {
                    hour += 12;
                }
            }
            "M" => minute = int_of(value)?,
            "S" => second = int_of(value)?,
            "f" => {
                let mut digits = points(value);
                while digits.len() < 6 {
                    digits.push(u32::from('0'));
                }
                fraction = py_int(&digits)?;
            }
            "A" => weekday = Some(index_of_lower(&F_WEEKDAY, value)?),
            "a" => weekday = Some(index_of_lower(&A_WEEKDAY, value)?),
            "w" => {
                let w = int_of(value)?;
                weekday = Some(if w == 0 { 6 } else { w - 1 });
            }
            "u" => weekday = Some(int_of(value)? - 1),
            "j" => julian = Some(int_of(value)?),
            "U" | "W" => {
                week_of_year = Some(int_of(value)?);
                week_starts_mon = *key == "W";
            }
            "V" => iso_week = Some(int_of(value)?),
            "z" => offset_us = Some(parse_offset(value)?),
            _ => {}
        }
    }

    if iso_year.is_some() {
        if julian.is_some() {
            return Err(PyError::value_error(
                "Day of the year directive '%j' is not compatible with ISO year directive \
                 '%G'. Use '%Y' instead.",
            ));
        } else if iso_week.is_none() || weekday.is_none() {
            return Err(PyError::value_error(
                "ISO year directive '%G' must be used with the ISO week directive '%V' and \
                 a weekday directive ('%A', '%a', '%w', or '%u').",
            ));
        }
    } else if iso_week.is_some() {
        if year.is_none() || weekday.is_none() {
            return Err(PyError::value_error(
                "ISO week directive '%V' must be used with the ISO year directive '%G' and \
                 a weekday directive ('%A', '%a', '%w', or '%u').",
            ));
        }
        return Err(PyError::value_error(
            "ISO week directive '%V' is incompatible with the year directive '%Y'. Use the \
             ISO year '%G' instead.",
        ));
    }

    let mut leap_year_fix = false;
    let mut year = match year {
        Some(y) => y,
        None if month == 2 && day == 29 => {
            leap_year_fix = true;
            1904
        }
        None => 1900,
    };

    if let (None, Some(wd)) = (julian, weekday) {
        if let Some(week) = week_of_year {
            julian = Some(julian_from_u_or_w(year, week, wd, week_starts_mon)?);
        } else if let (Some(iy), Some(iw)) = (iso_year, iso_week) {
            let as_i32 =
                |n: i64| i32::try_from(n).map_err(|_| PyError::overflow_error("int too large"));
            let result = Date::from_iso_calendar(as_i32(iy)?, as_i32(iw)?, as_i32(wd + 1)?)?;
            year = i64::from(result.year());
            month = i64::from(result.month());
            day = i64::from(result.day());
        }
        if let Some(j) = julian {
            if j <= 0 {
                year -= 1;
                julian = Some(j + if is_leap(year) { 366 } else { 365 });
            }
        }
    }

    match julian {
        None => {
            Date::new(year, month, day)?;
            Date::new(year, 1, 1)?;
        }
        Some(j) => {
            let first = Date::new(year, 1, 1)?.to_ordinal();
            let result = Date::from_ordinal(j - 1 + first)?;
            year = i64::from(result.year());
            month = i64::from(result.month());
            day = i64::from(result.day());
        }
    }
    if leap_year_fix {
        year = 1900;
    }
    Ok(Parsed {
        year,
        month,
        day,
        hour,
        minute,
        second,
        fraction,
        offset_us,
    })
}

impl Parsed {
    /// `_strptime_datetime`: builds the timezone, then `datetime(*args)`.
    fn into_datetime(self) -> PyResult<Value> {
        let offset = match self.offset_us {
            Some(us) => {
                let delta = TimeDelta::from_microseconds_i128(i128::from(us))?;
                FixedOffset::new(delta)?;
                Value::int(us.div_euclid(1_000_000))
            }
            None => Value::None,
        };
        check_date_args(self.year, self.month, self.day)?;
        let checks = [
            (self.hour, 23, "hour must be in 0..23"),
            (self.minute, 59, "minute must be in 0..59"),
            (self.second, 59, "second must be in 0..59"),
            (self.fraction, 999_999, "microsecond must be in 0..999999"),
        ];
        for (value, max, message) in checks {
            if !(0..=max).contains(&value) {
                return Err(PyError::value_error(message));
            }
        }
        Ok(Value::tuple(vec![
            Value::int(self.year),
            Value::int(self.month),
            Value::int(self.day),
            Value::int(self.hour),
            Value::int(self.minute),
            Value::int(self.second),
            Value::int(self.fraction),
            offset,
            Value::int(0),
        ]))
    }
}
