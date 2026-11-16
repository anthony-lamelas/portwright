//! `parse_date`, `parse_time`, `parse_datetime`, `parse_duration` (task `parse`).
//!
//! Every pattern is upstream's text compiled through `pc_re`, so `re`'s semantics
//! (`\Z`, `$` before a trailing newline, `P(?!\b)`, `{-2}` read as literals) hold.

mod dates;
mod durations;
mod times;

pub use dates::{date_regexps, default_date_regexps, parse_date, DateRegexps};
pub use durations::{
    duration_timedelta, parse_duration, parse_duration_text, DurationFields, DurationKind,
    DurationParse, ParsedDuration,
};
pub use times::{parse_time, time_regexps, TimeFields};

use pc_datetime_port_core::date::Date;

use crate::error::{Error, Result};
use crate::PyStr;

/// Code points of an ASCII literal.
fn cps(text: &str) -> Vec<u32> {
    text.chars().map(u32::from).collect()
}

/// `"<prefix>%r" % text` for a `str` argument.
fn repr_message(prefix: &str, text: &[u32]) -> Error {
    let repr = PyStr::from_code_points(text.to_vec()).repr();
    Error::iso8601(format!("{prefix}{repr}"))
}

/// `datetimestring.split('T')` into exactly two parts, or `None`.
#[must_use]
pub fn split_datetime(text: &[u32]) -> Option<(&[u32], &[u32])> {
    let mut parts = text.split(|&c| c == u32::from('T'));
    let date = parts.next()?;
    let time = parts.next()?;
    parts.next().is_none().then_some((date, time))
}

/// `parse_datetime(text)` for a `str`: the date and time parts, before `combine`.
///
/// # Errors
/// `ISO8601Error` when the `T` designator isn't there exactly once, then
/// [`parse_date`]'s and [`parse_time`]'s errors.
pub fn parse_datetime(text: &[u32]) -> Result<(Date, TimeFields)> {
    let Some((date_text, time_text)) = split_datetime(text) else {
        return Err(missing_designator(
            &PyStr::from_code_points(text.to_vec()).repr(),
        ));
    };
    let unit = pc_datetime_port_core::Value::int(1);
    let date = parse_date(date_text, default_date_regexps()?, &unit, &unit)?;
    let time = parse_time(time_text)?;
    Ok((date, time))
}

/// The `ISO8601Error` for a datetime string without exactly one `T`; `repr` is `%r` of it.
#[must_use]
pub fn missing_designator(repr: &str) -> Error {
    Error::iso8601(format!(
        "ISO 8601 time designator 'T' missing. Unable to parse datetime string {repr}"
    ))
}
