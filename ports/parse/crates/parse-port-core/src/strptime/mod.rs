//! Port of CPython 3.12 `_strptime` (C/English locale, `time.tzname ==
//! ('UTC', 'UTC')`, no daylight saving) plus upstream's `strf_date_convert`.
//!
//! Positions and slices are in code points, like Python `str`.

mod calendar;

use std::borrow::Cow;
use std::sync::Arc;

use num_bigint::BigInt;
use num_traits::ToPrimitive;

use crate::error::Error;
use crate::pyre::cache::BoundedCache;
use crate::pyre::unicode15 as pystr;
use crate::pyre::{Flags, Regex};
use crate::types::{current_year, is_leap, Date, DateTime, Temporal, TimeDelta, TzInfo};

use calendar::{from_iso_calendar, from_ordinal, to_ordinal, weekday};

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
const AM_PM: [&str; 2] = ["am", "pm"];

/// Regex snippet for one directive key of `TimeRE`, or `None` (KeyError).
fn directive(key: &str) -> Option<String> {
    let s: &str = match key {
        "d" => r"(?P<d>3[0-1]|[1-2]\d|0[1-9]|[1-9]| [1-9])",
        "f" => r"(?P<f>[0-9]{1,6})",
        "H" => r"(?P<H>2[0-3]|[0-1]\d|\d)",
        "I" => r"(?P<I>1[0-2]|0[1-9]|[1-9]| [1-9])",
        "G" => r"(?P<G>\d\d\d\d)",
        "j" => r"(?P<j>36[0-6]|3[0-5]\d|[1-2]\d\d|0[1-9]\d|00[1-9]|[1-9]\d|0[1-9]|[1-9])",
        "m" => r"(?P<m>1[0-2]|0[1-9]|[1-9])",
        "M" => r"(?P<M>[0-5]\d|\d)",
        "S" => r"(?P<S>6[0-1]|[0-5]\d|\d)",
        "U" => r"(?P<U>5[0-3]|[0-4]\d|\d)",
        "W" => r"(?P<W>5[0-3]|[0-4]\d|\d)",
        "w" => r"(?P<w>[0-6])",
        "u" => r"(?P<u>[1-7])",
        "V" => r"(?P<V>5[0-3]|0[1-9]|[1-4]\d|\d)",
        "y" => r"(?P<y>\d\d)",
        "Y" => r"(?P<Y>\d\d\d\d)",
        "z" => r"(?P<z>[+-]\d\d:?[0-5]\d(:?[0-5]\d(\.\d{1,6})?)?|(?-i:Z))",
        "A" => "(?P<A>wednesday|thursday|saturday|tuesday|monday|friday|sunday)",
        "a" => "(?P<a>mon|tue|wed|thu|fri|sat|sun)",
        "B" => "(?P<B>september|february|november|december|january|october|august|march|april|june|july|may)",
        "b" => "(?P<b>jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec)",
        "p" => "(?P<p>am|pm)",
        "Z" => "(?P<Z>utc|gmt)",
        "%" => "%",
        "Od" | "Om" | "Oy" | "OH" | "OI" | "OM" | "OS" => {
            return Some(format!(r"(?P<{}>\d\d|\d| \d)", &key[1..]));
        }
        "Ow" => r"(?P<w>\d)",
        // C locale: LC_time, LC_date, LC_date_time run through `pattern()`.
        "X" => return pattern("%H:%M:%S").ok(),
        "x" => return pattern("%m/%d/%y").ok(),
        "c" => return pattern("%a %b %d %H:%M:%S %Y").ok(),
        _ => return None,
    };
    Some(s.to_owned())
}

/// `TimeRE.pattern(format)`. `Err(key)` is the KeyError for a bad directive.
fn pattern(format: &str) -> Result<String, String> {
    // Step 1-3: escape regex syntax, whitespace runs -> `\s+`, `'` -> `['ʼ]`.
    let mut escaped: Vec<char> = Vec::with_capacity(format.len() * 2);
    let mut chars = format.chars().peekable();
    while let Some(c) = chars.next() {
        if r"\.^$*+?(){}[]|".contains(c) {
            escaped.push('\\');
            escaped.push(c);
        } else if pystr::is_space(c) {
            while chars.peek().is_some_and(|&n| pystr::is_space(n)) {
                chars.next();
            }
            escaped.extend(r"\s+".chars());
        } else {
            escaped.push(c);
        }
    }
    let mut quoted: Vec<char> = Vec::with_capacity(escaped.len());
    for c in escaped {
        if c == '\'' {
            quoted.extend("['\u{02bc}]".chars());
        } else {
            quoted.push(c);
        }
    }
    // Step 4: re.sub(r'%(O?.)', repl, format).
    let mut out = String::with_capacity(quoted.len() * 4);
    let mut i = 0;
    while i < quoted.len() {
        let c = quoted[i];
        if c == '%' {
            let next = quoted.get(i + 1).copied().filter(|&n| n != '\n');
            let after = quoted.get(i + 2).copied().filter(|&n| n != '\n');
            let key = match (next, after) {
                (Some('O'), Some(a)) => Some(format!("O{a}")),
                (Some(n), _) => Some(n.to_string()),
                (None, _) => None,
            };
            if let Some(key) = key {
                let rep = directive(&key).ok_or_else(|| key.clone())?;
                out.push_str(&rep);
                i += 1 + key.chars().count();
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    Ok(out)
}

/// `_TimeRE_cache.compile(format)` with `_strptime`'s error translation.
fn compile(format: &str) -> Result<Arc<Regex>, Error> {
    static CACHE: BoundedCache<Regex> = BoundedCache::new(64);
    CACHE.get_or_try_insert(format, || {
        let pat = pattern(format).map_err(|key| {
            let bad = if key == "\\" { "%" } else { key.as_str() };
            Error::Value(format!("'{bad}' is a bad directive in format '{format}'"))
        })?;
        Regex::compile(
            &pat,
            Flags {
                ignorecase: true,
                dotall: false,
            },
        )
    })
}

/// A `datetime.strptime` result: the value plus the `%Z` text that names
/// the `datetime.timezone` when `%z` was also present (`timezone(delta, name)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Strptime<T> {
    /// The parsed value. A `%z` offset is `TzInfo::Timezone`.
    pub value: T,
    /// `Some(name)` when the timezone must be `timezone(offset, name)`.
    pub tzname: Option<String>,
}

fn py_int(s: &str) -> Result<i64, Error> {
    let v = pystr::py_int(s, 10)?;
    // Every regex-bounded value fits; out of range only via huge inputs.
    v.to_i64()
        .ok_or_else(|| Error::Overflow("Python int too large to convert to C int".into()))
}

fn index_of(list: &[&str], value: &str) -> Result<i64, Error> {
    let lowered = pystr::lower(value);
    list.iter()
        .position(|&x| x == lowered)
        .and_then(|p| i64::try_from(p).ok())
        .ok_or_else(|| Error::Value(format!("{} is not in list", pystr::repr(&lowered))))
}

/// `repr(timedelta)`.
fn timedelta_repr(td: &TimeDelta) -> String {
    let mut parts = Vec::new();
    if td.days() != 0 {
        parts.push(format!("days={}", td.days()));
    }
    if td.seconds() != 0 {
        parts.push(format!("seconds={}", td.seconds()));
    }
    if td.microseconds() != 0 {
        parts.push(format!("microseconds={}", td.microseconds()));
    }
    if parts.is_empty() {
        "datetime.timedelta(0)".to_owned()
    } else {
        format!("datetime.timedelta({})", parts.join(", "))
    }
}

/// Parsed `%z` text: (gmtoff seconds, fraction microseconds).
fn parse_z(z: &str) -> Result<(i64, i64), Error> {
    if z == "Z" {
        return Ok((0, 0));
    }
    let original = z;
    let mut z: Vec<char> = z.chars().collect();
    let slice = |z: &[char], a: usize, b: usize| -> String {
        let b = b.min(z.len());
        if a >= b {
            String::new()
        } else {
            z[a..b].iter().collect()
        }
    };
    if z.get(3) == Some(&':') {
        z.remove(3);
        if z.len() > 5 {
            if z.get(5) != Some(&':') {
                return Err(Error::Value(format!("Inconsistent use of : in {original}")));
            }
            z.remove(5);
        }
    }
    let hours = py_int(&slice(&z, 1, 3))?;
    let minutes = py_int(&slice(&z, 3, 5))?;
    let sec_text = slice(&z, 5, 7);
    let seconds = if sec_text.is_empty() {
        0
    } else {
        py_int(&sec_text)?
    };
    let mut gmtoff = hours * 3600 + minutes * 60 + seconds;
    let mut rem = slice(&z, 8, z.len());
    let pad = 6usize.saturating_sub(rem.chars().count());
    rem.push_str(&"0".repeat(pad));
    let mut fraction = py_int(&rem)?;
    if z.first() == Some(&'-') {
        gmtoff = -gmtoff;
        fraction = -fraction;
    }
    Ok((gmtoff, fraction))
}

/// `_calc_julian_from_U_or_W`.
fn julian_from_u_or_w(
    year: i64,
    week_of_year: i64,
    day_of_week: i64,
    week_starts_mon: bool,
) -> Result<i64, Error> {
    let mut first_weekday = weekday(to_ordinal(&Date::new(year, 1, 1)?));
    let mut day_of_week = day_of_week;
    if !week_starts_mon {
        first_weekday = (first_weekday + 1) % 7;
        day_of_week = (day_of_week + 1) % 7;
    }
    let week_0_length = (7 - first_weekday) % 7;
    Ok(if week_of_year == 0 {
        1 + day_of_week - first_weekday
    } else {
        1 + week_0_length + 7 * (week_of_year - 1) + day_of_week
    })
}

/// `datetime.datetime.strptime(data, format)`.
pub fn strptime(data: &str, format: &str) -> Result<Strptime<DateTime>, Error> {
    let regex = compile(format)?;
    let text: Vec<char> = data.chars().collect();
    let found = regex.match_at(&text, 0, text.len()).ok_or_else(|| {
        Error::Value(format!(
            "time data {} does not match format {}",
            pystr::repr(data),
            pystr::repr(format)
        ))
    })?;
    let end = found.span(0).map_or(0, |(_, e)| e);
    if end != text.len() {
        let rest: String = text[end..].iter().collect();
        return Err(Error::Value(format!("unconverted data remains: {rest}")));
    }
    // `found.groupdict()` minus unmatched groups, in group-name order. Names
    // are unique, so a small `Vec` is an ordered map. For ASCII data, code
    // point and byte offsets agree, so values borrow from `data`.
    let ascii = data.is_ascii();
    let mut groups: Vec<(&str, Cow<'_, str>)> = Vec::with_capacity(regex.groupindex().len());
    for (name, &idx) in regex.groupindex() {
        let value = match found.span(idx) {
            Some((s, e)) if ascii => data.get(s..e).map(Cow::Borrowed),
            Some(_) => found.text(&text, idx).map(Cow::Owned),
            None => None,
        };
        if let Some(t) = value {
            groups.push((name.as_str(), t));
        }
    }
    let group = |name: &str| groups.iter().find(|(k, _)| *k == name).map(|(_, v)| v);

    let mut iso_year: Option<i64> = None;
    let mut year: Option<i64> = None;
    let (mut month, mut day) = (1i64, 1i64);
    let (mut hour, mut minute, mut second, mut fraction) = (0i64, 0i64, 0i64, 0i64);
    let mut gmtoff: Option<i64> = None;
    let mut gmtoff_fraction = 0i64;
    let mut iso_week: Option<i64> = None;
    let mut week_of_year: Option<i64> = None;
    let mut week_of_year_start = 0i64;
    let mut weekday_v: Option<i64> = None;
    let mut julian: Option<i64> = None;

    for (key, value) in &groups {
        let key = *key;
        match key {
            "y" => {
                let y = py_int(value)?;
                year = Some(if y <= 68 { y + 2000 } else { y + 1900 });
            }
            "Y" => year = Some(py_int(value)?),
            "G" => iso_year = Some(py_int(value)?),
            "m" => month = py_int(value)?,
            "B" => month = index_of(&F_MONTH, value)?,
            "b" => month = index_of(&A_MONTH, value)?,
            "d" => day = py_int(value)?,
            "H" => hour = py_int(value)?,
            "I" => {
                hour = py_int(value)?;
                let ampm = group("p").map(|p| pystr::lower(p)).unwrap_or_default();
                if ampm.is_empty() || ampm == AM_PM[0] {
                    if hour == 12 {
                        hour = 0;
                    }
                } else if ampm == AM_PM[1] && hour != 12 {
                    hour += 12;
                }
            }
            "M" => minute = py_int(value)?,
            "S" => second = py_int(value)?,
            "f" => {
                let mut s = value.to_string();
                let pad = 6usize.saturating_sub(s.chars().count());
                s.push_str(&"0".repeat(pad));
                fraction = py_int(&s)?;
            }
            "A" => weekday_v = Some(index_of(&F_WEEKDAY, value)?),
            "a" => weekday_v = Some(index_of(&A_WEEKDAY, value)?),
            "w" => {
                let w = py_int(value)?;
                weekday_v = Some(if w == 0 { 6 } else { w - 1 });
            }
            "u" => weekday_v = Some(py_int(value)? - 1),
            "j" => julian = Some(py_int(value)?),
            "U" | "W" => {
                week_of_year = Some(py_int(value)?);
                week_of_year_start = if key == "U" { 6 } else { 0 };
            }
            "V" => iso_week = Some(py_int(value)?),
            "z" => {
                let (off, frac) = parse_z(value)?;
                gmtoff = Some(off);
                gmtoff_fraction = frac;
            }
            _ => {}
        }
    }

    if iso_year.is_some() {
        if julian.is_some() {
            return Err(Error::Value(
                "Day of the year directive '%j' is not compatible with ISO year directive '%G'. Use '%Y' instead.".into(),
            ));
        } else if iso_week.is_none() || weekday_v.is_none() {
            return Err(Error::Value(
                "ISO year directive '%G' must be used with the ISO week directive '%V' and a weekday directive ('%A', '%a', '%w', or '%u').".into(),
            ));
        }
    } else if iso_week.is_some() {
        if year.is_none() || weekday_v.is_none() {
            return Err(Error::Value(
                "ISO week directive '%V' must be used with the ISO year directive '%G' and a weekday directive ('%A', '%a', '%w', or '%u').".into(),
            ));
        }
        return Err(Error::Value(
            "ISO week directive '%V' is incompatible with the year directive '%Y'. Use the ISO year '%G' instead.".into(),
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

    if julian.is_none() {
        if let Some(wd) = weekday_v {
            if let Some(woy) = week_of_year {
                julian = Some(julian_from_u_or_w(year, woy, wd, week_of_year_start == 0)?);
            } else if let (Some(iy), Some(iw)) = (iso_year, iso_week) {
                let d = from_iso_calendar(iy, iw, wd + 1)?;
                year = i64::from(d.year);
                month = i64::from(d.month);
                day = i64::from(d.day);
            }
            if let Some(j) = julian {
                if j <= 0 {
                    year -= 1;
                    julian = Some(j + if is_leap(year) { 366 } else { 365 });
                }
            }
        }
    }

    if let Some(j) = julian {
        let base = to_ordinal(&Date::new(year, 1, 1)?);
        let d = from_ordinal(j - 1 + base)?;
        year = i64::from(d.year);
        month = i64::from(d.month);
        day = i64::from(d.day);
    } else {
        // Validates the date (e.g. "day is out of range for month").
        Date::new(year, month, day)?;
    }
    if leap_year_fix {
        year = 1900;
    }

    let mut tzname = None;
    let tz = match gmtoff {
        None => None,
        Some(off) => {
            let total = BigInt::from(off) * 1_000_000 + BigInt::from(gmtoff_fraction);
            let td = TimeDelta::from_microseconds(&total)?;
            if td.total_microseconds().abs() >= 86_400_000_000 {
                return Err(Error::Value(format!(
                    "offset must be a timedelta strictly between -timedelta(hours=24) and timedelta(hours=24), not {}.",
                    timedelta_repr(&td)
                )));
            }
            tzname = group("Z").filter(|z| !z.is_empty()).map(|z| z.to_string());
            Some(TzInfo::Timezone(td))
        }
    };
    let value = DateTime::new(year, month, day, hour, minute, second, fraction, tz)?;
    Ok(Strptime { value, tzname })
}

/// Upstream `strf_date_convert(x, _, type)`: `datetime.strptime` against the
/// current year when the format has no `%y`/`%Y`, narrowed to a date or time.
pub fn strf_date_convert(x: &str, type_: &str) -> Result<Strptime<Temporal>, Error> {
    // `f"%{d}" in type_`; every `d` below is ASCII.
    let has = |d: char| {
        type_
            .as_bytes()
            .windows(2)
            .any(|w| w[0] == b'%' && char::from(w[1]) == d)
    };
    let is_date = "aAwdbBmyYjUW".chars().any(has);
    let is_time = "HIpMSfz".chars().any(has);
    let (data, format): (Cow<'_, str>, Cow<'_, str>) =
        if !type_.contains("%y") && !type_.contains("%Y") {
            (
                Cow::Owned(format!("{x} {}", current_year())),
                Cow::Owned(format!("{type_} %Y")),
            )
        } else {
            (Cow::Borrowed(x), Cow::Borrowed(type_))
        };
    let Strptime { value: dt, tzname } = strptime(&data, &format)?;
    let value = match (is_date, is_time) {
        (true, true) => Temporal::DateTime(dt),
        (true, false) => Temporal::Date(dt.date),
        (false, true) => Temporal::Time(dt.time),
        (false, false) => return Err(Error::Value("Datetime not a date nor a time?".into())),
    };
    let tzname = if matches!(value, Temporal::Date(_)) {
        None
    } else {
        tzname
    };
    Ok(Strptime { value, tzname })
}

#[cfg(test)]
mod tests;
