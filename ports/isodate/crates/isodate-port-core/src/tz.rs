//! `isodate.tzinfo` and `isodate.isotzinfo` logic (task `tz`).
//!
//! The tzinfo classes themselves live in the PyO3 layer (they subclass
//! `datetime.tzinfo`); this module holds what they compute: fixed offsets,
//! time zone designator parsing and offset formatting.

use pc_datetime_port_core::timedelta::{new_delta, TimeDelta, Unit};
use pc_datetime_port_core::Value;

use crate::error::{Error, Result};
use crate::rx::{self, is_at, Groups, Named, Regex};
use crate::PyStr;

/// `isotzinfo.TZ_REGEX`.
pub const TZ_REGEX: &str =
    r"(?P<tzname>(Z|(?P<tzsign>[+-])(?P<tzhour>[0-9]{2})(:?(?P<tzmin>[0-9]{2}))?)?)";

/// `isotzinfo.TZ_RE`.
pub static TZ_RE: Regex = Regex::new(TZ_REGEX);

/// The tzinfo a time zone designator describes (`build_tzinfo`'s result).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TzSpec {
    /// `None`: no designator.
    Naive,
    /// The `UTC` singleton (`Z`).
    Utc,
    /// `FixedOffset(hours, minutes, name)`, signs already applied.
    Fixed {
        /// Signed hours.
        hours: i64,
        /// Signed minutes.
        minutes: i64,
        /// The designator text, used as the tzinfo's name.
        name: Vec<u32>,
    },
}

/// `build_tzinfo(tzname, tzsign, tzhour, tzmin)` for string groups.
#[must_use]
pub fn build_tzspec(
    tzname: Option<&[u32]>,
    tzsign: Option<&[u32]>,
    tzhour: i64,
    tzmin: i64,
) -> TzSpec {
    let Some(name) = tzname.filter(|n| !n.is_empty()) else {
        return TzSpec::Naive;
    };
    if name == [u32::from('Z')] {
        return TzSpec::Utc;
    }
    let sign = if tzsign == Some(&[u32::from('-')][..]) {
        -1
    } else {
        1
    };
    TzSpec::Fixed {
        hours: sign * tzhour,
        minutes: sign * tzmin,
        name: name.to_vec(),
    }
}

/// `TZ_REGEX` at `start`, greedy as `re` tries it: records `tzname` (always, maybe
/// empty), `tzsign`, `tzhour` and `tzmin`, and returns the end.
///
/// `Z` comes first; otherwise `[+-][0-9]{2}`, then `:?[0-9]{2}` if it is all there.
/// When nothing fits, the group is empty.
pub fn match_tz(text: &[u32], start: usize, groups: &mut Groups<'_>) -> usize {
    let end = if is_at(text, start, "Z") {
        start + 1
    } else if let Some(hour_end) = is_at(text, start, "+-")
        .then(|| rx::digits(text, start + 1, 2))
        .flatten()
    {
        groups.set("tzsign", start, start + 1);
        groups.set("tzhour", start + 1, hour_end);
        let min_start = hour_end + usize::from(is_at(text, hour_end, ":"));
        match rx::digits(text, min_start, 2) {
            Some(min_end) => {
                groups.set("tzmin", min_start, min_end);
                min_end
            }
            None => hour_end,
        }
    } else {
        start
    };
    groups.set("tzname", start, end);
    end
}

/// [`build_tzspec`] for the groups [`match_tz`] found.
#[must_use]
pub fn tzspec_of(groups: &impl Named) -> TzSpec {
    build_tzspec(
        groups.name("tzname"),
        groups.name("tzsign"),
        rx::small_int(groups.name("tzhour")),
        rx::small_int(groups.name("tzmin")),
    )
}

/// `parse_tzinfo(tzstring)`: parses the designator at the start of `text`.
///
/// `TZ_RE.match` always succeeds (every part is optional), so a string without a
/// designator gives [`TzSpec::Naive`] and trailing text is ignored.
///
/// # Errors
/// None: kept for the signature `re` errors would need.
pub fn parse_tzinfo(text: &[u32]) -> Result<TzSpec> {
    let mut groups = Groups::new(text);
    match_tz(text, 0, &mut groups);
    Ok(tzspec_of(&groups))
}

/// `timedelta(hours=hours, minutes=minutes)`, the offset of a `FixedOffset`.
///
/// # Errors
/// `timedelta`'s TypeError, ValueError and OverflowError.
pub fn fixed_offset_delta(hours: &Value, minutes: &Value) -> Result<TimeDelta> {
    Ok(new_delta(&[
        (Unit::Hours, hours),
        (Unit::Minutes, minutes),
    ])?)
}

/// [`fixed_offset_delta`] for `int` hours and minutes, without building `Value`s:
/// `new_delta` sums whole units exactly, as this does.
///
/// # Errors
/// `timedelta`'s OverflowError when the days leave its range.
pub fn fixed_offset_delta_ints(hours: i64, minutes: i64) -> Result<TimeDelta> {
    let us = (i128::from(hours) * 60 + i128::from(minutes)) * 60_000_000;
    Ok(TimeDelta::from_microseconds_i128(us)?)
}

/// The formats `tz_isoformat` accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OffsetFormat {
    /// `%Z`: `+HH:MM`.
    Extended,
    /// `%z`: `+HHMM`.
    Basic,
    /// `%h`: `+HH`.
    Hours,
}

impl OffsetFormat {
    /// The format named by `text`, if it is one of `%Z`, `%z`, `%h`.
    #[must_use]
    pub fn from_code_points(text: &[u32]) -> Option<Self> {
        match text {
            [0x25, 0x5A] => Some(Self::Extended),
            [0x25, 0x7A] => Some(Self::Basic),
            [0x25, 0x68] => Some(Self::Hours),
            _ => None,
        }
    }
}

/// A non-zero UTC offset split for `tz_isoformat`; seconds are dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SplitOffset {
    negative: bool,
    hours: i64,
    minutes: i64,
}

impl SplitOffset {
    /// Splits `days * 86400 + seconds` of an offset.
    ///
    /// # Errors
    /// `OverflowError('can not handle differences > 99 hours')`.
    pub fn from_seconds(seconds: i64) -> Result<Self> {
        let negative = seconds < 0;
        let total = seconds.unsigned_abs();
        let minutes = total / 60;
        let hours = minutes / 60;
        if hours > 99 {
            return Err(overflow_99());
        }
        // Both fit: hours <= 99.
        Ok(Self {
            negative,
            hours: i64::try_from(hours).map_err(|_| overflow_99())?,
            minutes: i64::try_from(minutes % 60).map_err(|_| overflow_99())?,
        })
    }

    /// The offset in `format`.
    #[must_use]
    pub fn format(self, format: OffsetFormat) -> String {
        let sign = if self.negative { '-' } else { '+' };
        let (h, m) = (self.hours, self.minutes);
        match format {
            OffsetFormat::Extended => format!("{sign}{h:02}:{m:02}"),
            OffsetFormat::Basic => format!("{sign}{h:02}{m:02}"),
            OffsetFormat::Hours => format!("{sign}{h:02}"),
        }
    }
}

fn overflow_99() -> Error {
    Error::overflow("can not handle differences > 99 hours")
}

/// `ValueError('unknown format string "%s"' % format)`.
#[must_use]
pub fn unknown_format(format: &[u32]) -> Error {
    let mut message: Vec<u32> = "unknown format string \"".chars().map(u32::from).collect();
    message.extend_from_slice(format);
    message.push(u32::from('"'));
    Error::value(PyStr::from_code_points(message))
}

/// The non-`Z` part of `tz_isoformat`: formats an offset of `seconds`
/// (`days * 86400 + seconds`) with the format string `format`.
///
/// # Errors
/// `OverflowError` beyond 99 hours (checked first), then `ValueError` for an
/// unknown format.
pub fn format_offset(seconds: i64, format: &[u32]) -> Result<PyStr> {
    let split = SplitOffset::from_seconds(seconds)?;
    let format = OffsetFormat::from_code_points(format).ok_or_else(|| unknown_format(format))?;
    Ok(PyStr::from_code_points(
        split.format(format).chars().map(u32::from).collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cps(s: &str) -> Vec<u32> {
        s.chars().map(u32::from).collect()
    }

    #[test]
    fn fixed_offset_ints_is_new_delta() {
        let int = |n: i64| Value::Int(num_bigint::BigInt::from(n));
        let values = [
            0,
            1,
            -1,
            5,
            -5,
            30,
            -30,
            59,
            99,
            -99,
            1440,
            23_999_999_976,
            -23_999_999_976,
            23_999_999_977,
            i64::MAX,
            i64::MIN,
        ];
        for &h in &values {
            for &m in &values {
                let fast = fixed_offset_delta_ints(h, m);
                let slow = fixed_offset_delta(&int(h), &int(m));
                assert_eq!(fast, slow, "{h} {m}");
            }
        }
    }

    #[test]
    fn parses_prefixes() -> Result<()> {
        assert_eq!(parse_tzinfo(&cps("Zulu"))?, TzSpec::Utc);
        assert_eq!(parse_tzinfo(&cps("x"))?, TzSpec::Naive);
        assert_eq!(
            parse_tzinfo(&cps("-05:30"))?,
            TzSpec::Fixed {
                hours: -5,
                minutes: -30,
                name: cps("-05:30")
            }
        );
        assert_eq!(
            parse_tzinfo(&cps("+0100x"))?,
            TzSpec::Fixed {
                hours: 1,
                minutes: 0,
                name: cps("+0100")
            }
        );
        Ok(())
    }

    #[test]
    fn match_tz_is_tz_re() -> Result<()> {
        let texts = rx::sample_texts(
            &[
                "Z", "+01", "-05:30", "+0530", "+01:", "+01:3", "-0", "Zulu", "+12:34x", "",
            ],
            20_000,
        );
        let names = ["tzname", "tzsign", "tzhour", "tzmin"];
        rx::assert_same_groups(TZ_RE.pattern()?, &texts, &names, |text| {
            let mut groups = Groups::new(text);
            match_tz(text, 0, &mut groups);
            Some(groups)
        });
        Ok(())
    }

    #[test]
    fn formats_offsets() -> Result<()> {
        assert_eq!(
            format_offset(-3600 - 61, &cps("%Z"))?.to_string_lossy(),
            "-01:01"
        );
        assert_eq!(format_offset(19800, &cps("%z"))?.to_string_lossy(), "+0530");
        assert_eq!(format_offset(19800, &cps("%h"))?.to_string_lossy(), "+05");
        assert!(format_offset(100 * 3600, &cps("%x")).is_err());
        assert_eq!(
            format_offset(1, &cps("%x")),
            Err(Error::value("unknown format string \"%x\""))
        );
        Ok(())
    }
}
