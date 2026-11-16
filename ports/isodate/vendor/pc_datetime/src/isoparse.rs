//! Task `isoparse`: `date.fromisoformat`, `time.fromisoformat` and
//! `datetime.fromisoformat`, ported from CPython 3.12's C parsers in `_datetimemodule.c`.
//!
//! CPython parses the UTF-8 bytes of the string through a NUL-terminated C buffer, so
//! lengths are byte lengths and reading past the end sees `0`. [`Bytes::at`] models
//! that terminator; every quirk of the C code (unchecked trailing text, ignored garbage
//! before an offset, a zero offset with microseconds becoming UTC) is kept.

use std::borrow::Cow;

use crate::args::Param;
use crate::date::{
    check_date_args, is_leap, iso_week1_monday, ord_to_ymd, ymd_to_ord, Date, MAXYEAR, MINYEAR,
};
use crate::error::{PyError, PyResult};
use crate::pystr::PyStr;
use crate::timedelta::{FixedOffset, TimeDelta};
use crate::{Kwargs, Value};

/// A UTF-8 buffer read like a NUL-terminated C string.
struct Bytes(Vec<u8>);

impl Bytes {
    /// The byte at `i`, or `0` at and past the end.
    fn at(&self, i: usize) -> u8 {
        self.0.get(i).copied().unwrap_or(0)
    }

    fn len(&self) -> usize {
        self.0.len()
    }

    /// CPython's `parse_digits`: `n` ASCII digits from `pos`, accumulated into `value`.
    fn digits(&self, pos: usize, n: usize, value: &mut i32) -> Option<usize> {
        for i in 0..n {
            let d = self.at(pos + i).wrapping_sub(b'0');
            if d > 9 {
                return None;
            }
            *value = value.saturating_mul(10).saturating_add(i32::from(d));
        }
        Some(pos + n)
    }
}

/// UTF-8 encodes `text`, or None when it holds a lone surrogate (`PyUnicode_AsUTF8AndSize`
/// fails, which every parser reports as an invalid string).
fn encode(text: &PyStr) -> Option<Bytes> {
    // `text.to_str()`'s bytes, written straight into a buffer of the final size.
    let points = text.code_points();
    let mut out = Vec::with_capacity(points.len());
    for &c in points {
        let ch = char::from_u32(c)?;
        match u8::try_from(c) {
            Ok(b) if b.is_ascii() => out.push(b),
            _ => out.extend_from_slice(ch.encode_utf8(&mut [0; 4]).as_bytes()),
        }
    }
    Some(Bytes(out))
}

fn is_digit(c: u8) -> bool {
    c.is_ascii_digit()
}

/// A parsed calendar date, not yet validated.
#[derive(Debug, Default, Clone, Copy)]
struct Ymd {
    year: i32,
    month: i32,
    day: i32,
}

/// CPython's `iso_to_ymd`; None for every failure code.
fn iso_to_ymd(year: i32, week: i32, day: i32) -> Option<Ymd> {
    let y = i64::from(year);
    if !(MINYEAR..=MAXYEAR).contains(&y) {
        return None;
    }
    if week <= 0 || week >= 53 {
        let first_weekday = (ymd_to_ord(y, 1, 1) + 6) % 7;
        let long_year = first_weekday == 3 || (first_weekday == 2 && is_leap(y));
        if !(week == 53 && long_year) {
            return None;
        }
    }
    if day <= 0 || day >= 8 {
        return None;
    }
    let ordinal = iso_week1_monday(y) + i64::from(week - 1) * 7 + i64::from(day) - 1;
    let (ny, nm, nd) = ord_to_ymd(ordinal);
    Some(Ymd {
        year: i32::try_from(ny).ok()?,
        month: i32::try_from(nm).ok()?,
        day: i32::try_from(nd).ok()?,
    })
}

/// CPython's `parse_isoformat_date` over `buf[..len]` (text past `len` is not checked).
fn parse_date(buf: &Bytes, len: usize) -> Option<Ymd> {
    let mut year = 0;
    let mut p = buf.digits(0, 4, &mut year)?;
    let uses_separator = buf.at(p) == b'-';
    if uses_separator {
        p += 1;
    }
    if buf.at(p) == b'W' {
        p += 1;
        let mut week = 0;
        let mut day = 0;
        p = buf.digits(p, 2, &mut week)?;
        if p < len {
            if uses_separator {
                if buf.at(p) != b'-' {
                    return None;
                }
                p += 1;
            }
            buf.digits(p, 1, &mut day)?;
        } else {
            day = 1;
        }
        return iso_to_ymd(year, week, day);
    }
    let mut month = 0;
    let mut day = 0;
    p = buf.digits(p, 2, &mut month)?;
    if uses_separator {
        if buf.at(p) != b'-' {
            return None;
        }
        p += 1;
    }
    buf.digits(p, 2, &mut day)?;
    Some(Ymd { year, month, day })
}

/// Hour, minute, second and microsecond as parsed.
#[derive(Debug, Default, Clone, Copy)]
struct Hms {
    hour: i32,
    minute: i32,
    second: i32,
    microsecond: i32,
}

/// CPython's `parse_hh_mm_ss_ff` over `buf[start..end]`: `None` on a parse error (C's `-1`),
/// and `true` alongside the time when text remains (C's `1`).
fn parse_clock(buf: &Bytes, start: usize, end: usize) -> Option<(Hms, bool)> {
    let mut hms = Hms::default();
    let mut p = start;
    let mut has_separator = true;
    for i in 0..3 {
        let slot = match i {
            0 => &mut hms.hour,
            1 => &mut hms.minute,
            _ => &mut hms.second,
        };
        p = buf.digits(p, 2, slot)?;
        let c = buf.at(p);
        p += 1;
        if i == 0 {
            has_separator = c == b':';
        }
        if p >= end {
            return Some((hms, c != 0));
        } else if has_separator && c == b':' {
            continue;
        } else if c == b'.' || c == b',' {
            break;
        } else if !has_separator {
            p -= 1;
        } else {
            return None;
        }
    }
    let to_parse = end.saturating_sub(p).min(6);
    let mut us = 0;
    p = buf.digits(p, to_parse, &mut us)?;
    for _ in to_parse..6 {
        us *= 10;
    }
    hms.microsecond = us;
    while is_digit(buf.at(p)) {
        p += 1;
    }
    Some((hms, buf.at(p) != 0))
}

/// A UTC offset as parsed: whole seconds and microseconds, both carrying the sign.
#[derive(Debug, Clone, Copy)]
struct ParsedOffset {
    seconds: i32,
    microseconds: i32,
}

/// CPython's `parse_isoformat_time` over `buf[start..start + len]`.
fn parse_time(buf: &Bytes, start: usize, len: usize) -> Option<(Hms, Option<ParsedOffset>)> {
    let end = start + len;
    let mut tz_pos = start;
    loop {
        if matches!(buf.at(tz_pos), b'Z' | b'+' | b'-') {
            break;
        }
        tz_pos += 1;
        if tz_pos >= end {
            break;
        }
    }
    let (hms, more) = parse_clock(buf, start, tz_pos)?;
    if tz_pos == end {
        return if more { None } else { Some((hms, None)) };
    }
    if buf.at(tz_pos) == b'Z' {
        if buf.at(tz_pos + 1) != 0 {
            return None;
        }
        let utc = ParsedOffset {
            seconds: 0,
            microseconds: 0,
        };
        return Some((hms, Some(utc)));
    }
    let sign = if buf.at(tz_pos) == b'-' { -1 } else { 1 };
    let (tz, more) = parse_clock(buf, tz_pos + 1, end)?;
    if more {
        return None;
    }
    let seconds = tz.hour * 3600 + tz.minute * 60 + tz.second;
    Some((
        hms,
        Some(ParsedOffset {
            seconds: sign * seconds,
            microseconds: sign * tz.microsecond,
        }),
    ))
}

/// CPython's `tzinfo_from_isoformat_results`: a zero seconds field is UTC even when
/// microseconds are present.
fn make_zone(offset: Option<ParsedOffset>) -> PyResult<Option<FixedOffset>> {
    let Some(off) = offset else {
        return Ok(None);
    };
    if off.seconds == 0 {
        return Ok(Some(FixedOffset::default()));
    }
    let total = i128::from(off.seconds) * 1_000_000 + i128::from(off.microseconds);
    FixedOffset::new(TimeDelta::from_microseconds_i128(total)?).map(Some)
}

/// The shim's `_offset`: whole seconds of the offset, microseconds dropped.
fn offset_out(tz: Option<FixedOffset>) -> Value {
    tz.map_or(Value::None, |tz| {
        let off = tz.utcoffset();
        Value::int(i64::from(off.days()) * 86_400 + i64::from(off.seconds()))
    })
}

/// CPython's `check_time_args` for a parsed time (fold is always 0).
fn check_time(hms: Hms) -> PyResult<()> {
    let checks = [
        (hms.hour, 23, "hour must be in 0..23"),
        (hms.minute, 59, "minute must be in 0..59"),
        (hms.second, 59, "second must be in 0..59"),
        (hms.microsecond, 999_999, "microsecond must be in 0..999999"),
    ];
    for (value, max, message) in checks {
        if !(0..=max).contains(&value) {
            return Err(PyError::value_error(message));
        }
    }
    Ok(())
}

/// `ValueError("Invalid isoformat string: " + repr(text))`.
fn invalid(text: &PyStr) -> PyError {
    PyError::value_error(format!("Invalid isoformat string: {}", text.repr()))
}

/// Binds the single `text` argument and requires a `str`.
fn bind_text<'a>(func: &str, args: &'a [Value], kwargs: &'a Kwargs) -> PyResult<Cow<'a, PyStr>> {
    let v = crate::timedelta::bind_ref(func, &[Param::required("text")], args, kwargs)?;
    match v.into_iter().next() {
        Some(Cow::Borrowed(Value::Str(s))) => Ok(Cow::Borrowed(s)),
        Some(Cow::Owned(Value::Str(s))) => Ok(Cow::Owned(s)),
        _ => Err(PyError::type_error("fromisoformat: argument must be str")),
    }
}

/// `date.fromisoformat(text)`.
fn date_from_iso(text: &PyStr) -> PyResult<Date> {
    let buf = encode(text).ok_or_else(|| invalid(text))?;
    let ymd = if matches!(buf.len(), 7 | 8 | 10) {
        parse_date(&buf, buf.len())
    } else {
        None
    };
    let ymd = ymd.ok_or_else(|| invalid(text))?;
    Date::new(
        i64::from(ymd.year),
        i64::from(ymd.month),
        i64::from(ymd.day),
    )
}

/// `time.fromisoformat(text)` as the shim's `_time_out` tuple.
fn time_from_iso(text: &PyStr) -> PyResult<Value> {
    let buf = encode(text).ok_or_else(|| invalid(text))?;
    let start = usize::from(buf.at(0) == b'T');
    let (hms, offset) = parse_time(&buf, start, buf.len() - start).ok_or_else(|| invalid(text))?;
    let tz = make_zone(offset)?;
    check_time(hms)?;
    Ok(Value::tuple(vec![
        Value::int(hms.hour),
        Value::int(hms.minute),
        Value::int(hms.second),
        Value::int(hms.microsecond),
        offset_out(tz),
        Value::int(0),
    ]))
}

/// CPython 3.12's `_sanitize_isoformat_str`: None for fewer than 7 code points;
/// otherwise the first non-ASCII code point (surrogates included) at index 7, 8 or 10
/// becomes `T`, so any single non-ASCII separator is accepted.
fn sanitize(text: &PyStr) -> Option<Cow<'_, PyStr>> {
    let points = text.code_points();
    if points.len() < 7 {
        return None;
    }
    for pos in [7, 8, 10] {
        if pos > points.len() {
            break;
        }
        if points.get(pos).is_some_and(|&c| c > 127) {
            let mut out = points.to_vec();
            out[pos] = u32::from('T');
            return Some(Cow::Owned(PyStr::from_code_points(out)));
        }
    }
    Some(Cow::Borrowed(text))
}

/// CPython's `_find_isoformat_datetime_separator`; None for its `-1`.
fn find_separator(buf: &Bytes) -> Option<usize> {
    let len = buf.len();
    if len == 7 {
        return Some(7);
    }
    if buf.at(4) == b'-' {
        if buf.at(5) != b'W' {
            return Some(10);
        }
        if len < 8 {
            return None;
        }
        if len > 8 && buf.at(8) == b'-' {
            if len == 9 {
                return None;
            }
            if len > 10 && is_digit(buf.at(10)) {
                return Some(8);
            }
            return Some(10);
        }
        return Some(8);
    }
    if buf.at(4) != b'W' {
        return Some(8);
    }
    let mut idx = 7;
    while idx < len && is_digit(buf.at(idx)) {
        idx += 1;
    }
    if idx < 9 {
        Some(idx)
    } else if idx % 2 == 0 {
        Some(7)
    } else {
        Some(8)
    }
}

/// `datetime.fromisoformat(text)` as the shim's `_dt_out` tuple.
fn datetime_from_iso(text: &PyStr) -> PyResult<Value> {
    let clean = sanitize(text).ok_or_else(|| invalid(text))?;
    let buf = encode(&clean).ok_or_else(|| invalid(text))?;
    let len = buf.len();
    let sep = find_separator(&buf).ok_or_else(|| invalid(text))?;
    let ymd = parse_date(&buf, sep).ok_or_else(|| invalid(text))?;
    let (hms, offset) = if len > sep {
        parse_time(&buf, sep + 1, len - sep - 1).ok_or_else(|| invalid(text))?
    } else {
        (Hms::default(), None)
    };
    let tz = make_zone(offset)?;
    let (y, m, d) = (
        i64::from(ymd.year),
        i64::from(ymd.month),
        i64::from(ymd.day),
    );
    check_date_args(y, m, d)?;
    check_time(hms)?;
    Ok(Value::tuple(vec![
        Value::int(y),
        Value::int(m),
        Value::int(d),
        Value::int(hms.hour),
        Value::int(hms.minute),
        Value::int(hms.second),
        Value::int(hms.microsecond),
        offset_out(tz),
        Value::int(0),
    ]))
}

/// `pc_datetime.date_fromisoformat`.
///
/// # Errors
/// Binding errors, `TypeError` for a non-str, `ValueError` for an invalid string or date.
pub fn date_fromisoformat(args: &[Value], kwargs: &Kwargs) -> PyResult<Value> {
    let text = bind_text("date_fromisoformat", args, kwargs)?;
    Ok(date_from_iso(&text)?.to_value())
}

/// `pc_datetime.time_fromisoformat`.
///
/// # Errors
/// Binding errors, `TypeError` for a non-str, `ValueError` for an invalid string,
/// offset or time.
pub fn time_fromisoformat(args: &[Value], kwargs: &Kwargs) -> PyResult<Value> {
    let text = bind_text("time_fromisoformat", args, kwargs)?;
    time_from_iso(&text)
}

/// `pc_datetime.datetime_fromisoformat`.
///
/// # Errors
/// Binding errors, `TypeError` for a non-str, `ValueError` for an invalid string,
/// offset, date or time.
pub fn datetime_fromisoformat(args: &[Value], kwargs: &Kwargs) -> PyResult<Value> {
    let text = bind_text("datetime_fromisoformat", args, kwargs)?;
    datetime_from_iso(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clock(text: &str) -> Option<(i32, i32, i32, i32, bool)> {
        let buf = Bytes(text.as_bytes().to_vec());
        parse_clock(&buf, 0, buf.len())
            .map(|(h, more)| (h.hour, h.minute, h.second, h.microsecond, more))
    }

    #[test]
    fn clock_forms() {
        assert_eq!(clock("12:34:56.789"), Some((12, 34, 56, 789_000, false)));
        assert_eq!(clock("12:34:56,5"), Some((12, 34, 56, 500_000, false)));
        assert_eq!(clock("123456.1234567"), Some((12, 34, 56, 123_456, false)));
        assert_eq!(clock("12:34"), Some((12, 34, 0, 0, false)));
        assert_eq!(clock("12"), Some((12, 0, 0, 0, false)));
    }

    #[test]
    fn clock_errors() {
        assert_eq!(clock("1"), None);
        assert_eq!(clock("12:3"), None);
        assert_eq!(clock("12:34;56"), None);
    }
}
