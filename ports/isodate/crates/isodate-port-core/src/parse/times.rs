//! `isotime.build_time_regexps` and `parse_time`.

use std::sync::{Arc, OnceLock};

use num_bigint::BigInt;
use pc_datetime_port_core::PyError;
use pc_re_port_core::Pattern;

use super::{cps, repr_message};
use crate::decimal::{Decimal, Rounding};
use crate::error::{Error, Result};
use crate::rx::{self, Groups, Named};
use crate::tz::{match_tz, tzspec_of, TzSpec, TZ_REGEX};

/// Which fields a time pattern carries (besides the time zone).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimeForm {
    /// `hour`, `minute`, `second` (fraction on the seconds).
    Second,
    /// `hour`, `minute` (fraction on the minutes).
    Minute,
    /// `hour` (fraction on the hours).
    Hour,
}

/// The bodies between `\A` and `TZ_REGEX\Z`, in upstream's order.
const TIME_FORMS: [(TimeForm, &str); 5] = [
    (
        TimeForm::Second,
        r"T?(?P<hour>[0-9]{2}):(?P<minute>[0-9]{2}):(?P<second>[0-9]{2}([,.][0-9]+)?)",
    ),
    (
        TimeForm::Second,
        r"T?(?P<hour>[0-9]{2})(?P<minute>[0-9]{2})(?P<second>[0-9]{2}([,.][0-9]+)?)",
    ),
    (
        TimeForm::Minute,
        r"T?(?P<hour>[0-9]{2}):(?P<minute>[0-9]{2}([,.][0-9]+)?)",
    ),
    (
        TimeForm::Minute,
        r"T?(?P<hour>[0-9]{2})(?P<minute>[0-9]{2}([,.][0-9]+)?)",
    ),
    (TimeForm::Hour, r"T?(?P<hour>[0-9]{2}([,.][0-9]+)?)"),
];

type TimePatterns = Vec<(TimeForm, Arc<Pattern>)>;

static PATTERNS: OnceLock<TimePatterns> = OnceLock::new();

fn patterns() -> Result<&'static TimePatterns> {
    if let Some(found) = PATTERNS.get() {
        return Ok(found);
    }
    let built = TIME_FORMS
        .iter()
        .map(|&(form, body)| {
            let source = cps(&format!(r"\A{body}{TZ_REGEX}\Z"));
            Ok((form, rx::compile(&source)?))
        })
        .collect::<Result<TimePatterns>>()?;
    Ok(PATTERNS.get_or_init(|| built))
}

/// `build_time_regexps()`: the compiled patterns, in order.
///
/// # Errors
/// None in practice; compile errors are reported as `re` would.
pub fn time_regexps() -> Result<Vec<Arc<Pattern>>> {
    Ok(patterns()?.iter().map(|(_, p)| Arc::clone(p)).collect())
}

/// One of the `TIME_FORMS` patterns, `\A...TZ_REGEX\Z`, matched by hand.
///
/// Every part is fixed or greedy with nothing after it that could take back what it
/// consumed, so the greedy pass is the only way each pattern can match.
fn match_form(text: &[u32], form: TimeForm, colons: bool) -> Option<Groups<'_>> {
    let mut groups = Groups::new(text);
    let mut i = usize::from(rx::is_at(text, 0, "T"));
    // `hh`, then `:` in the extended forms, for each field before the last.
    let mut field = |name: &'static str, i: usize, last: bool| -> Option<usize> {
        let end = rx::digits(text, i, 2)?;
        if last {
            let end = rx::fraction(text, end);
            groups.set(name, i, end);
            return Some(end);
        }
        groups.set(name, i, end);
        if colons {
            rx::is_at(text, end, ":").then_some(end + 1)
        } else {
            Some(end)
        }
    };
    i = field("hour", i, form == TimeForm::Hour)?;
    if form != TimeForm::Hour {
        i = field("minute", i, form == TimeForm::Minute)?;
    }
    if form == TimeForm::Second {
        i = field("second", i, true)?;
    }
    let end = match_tz(text, i, &mut groups);
    (end == text.len()).then_some(groups)
}

/// `[(form, extended)]` in the order of `TIME_FORMS`.
const MATCH_ORDER: [(TimeForm, bool); 5] = [
    (TimeForm::Second, true),
    (TimeForm::Second, false),
    (TimeForm::Minute, true),
    (TimeForm::Minute, false),
    (TimeForm::Hour, false),
];

/// The first of upstream's time patterns `text` matches, with its groups.
fn match_time(text: &[u32]) -> Option<(TimeForm, Groups<'_>)> {
    MATCH_ORDER
        .iter()
        .find_map(|&(form, colons)| Some((form, match_form(text, form, colons)?)))
}

/// The arguments `parse_time` passes to `time(...)`, already validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeFields {
    /// `0..=23`.
    pub hour: i32,
    /// `0..=59`.
    pub minute: i32,
    /// `0..=59`.
    pub second: i32,
    /// `0..=999999`.
    pub microsecond: i32,
    /// The tzinfo `build_tzinfo` gives.
    pub tz: TzSpec,
}

/// A group with `,` replaced by `.`, as a `Decimal`.
fn decimal_group(caps: &impl Named, name: &str) -> Result<Decimal> {
    let text: Vec<u32> = caps
        .name(name)
        .unwrap_or_default()
        .iter()
        .map(|&c| {
            if c == u32::from(',') {
                u32::from('.')
            } else {
                c
            }
        })
        .collect();
    Decimal::parse(&text)
}

/// `int(d)` as a `Decimal`.
fn trunc(d: &Decimal) -> Result<Decimal> {
    Ok(Decimal::from_bigint(&d.to_bigint()?))
}

fn to_field(n: &BigInt) -> i64 {
    // Out-of-range values only need to fail the range check below.
    i64::try_from(n).unwrap_or(i64::MAX)
}

/// `int((x - int(x)) * int(1e6)).to_integral())`.
fn microseconds(x: &Decimal) -> Result<i64> {
    let million = Decimal::from_bigint(&BigInt::from(1_000_000));
    let us = x.sub(&trunc(x)?)?.mul(&million)?;
    Ok(to_field(&us.to_integral(Rounding::HalfEven)?.to_bigint()?))
}

/// `Decimal('.000001')`, parsed once.
static MICRO: OnceLock<Decimal> = OnceLock::new();

/// `Decimal(x).quantize(Decimal('.000001'), rounding=ROUND_FLOOR)`.
fn floor_micro(x: &Decimal) -> Result<Decimal> {
    let exp = match MICRO.get() {
        Some(exp) => exp,
        None => {
            let parsed = Decimal::parse(&cps(".000001"))?;
            MICRO.get_or_init(|| parsed)
        }
    };
    x.quantize(exp, Rounding::Floor)
}

/// The second and microsecond upstream computes for a `[0-9]{2}([,.][0-9]+)?` seconds
/// group: `floor_micro(Decimal(text))` and its `microseconds`.
///
/// For a non-negative value with two integer digits, flooring to six places keeps the
/// first six fraction digits (at most eight digits, well inside the precision), so the
/// whole seconds are the integer digits and the microseconds the six digits, zero-padded.
fn split_seconds(text: &[u32]) -> (i64, i64) {
    let dot = text
        .iter()
        .position(|&c| c == 0x2C || c == 0x2E)
        .unwrap_or(text.len());
    let (whole, rest) = text.split_at(dot);
    let fraction = rest.get(1..).unwrap_or_default();
    let mut micro = [u32::from('0'); 6];
    for (slot, &digit) in micro.iter_mut().zip(fraction) {
        *slot = digit;
    }
    (rx::small_int(Some(whole)), rx::small_int(Some(&micro)))
}

/// CPython's `check_time_args` for `time(hour, minute, second, microsecond)`.
fn check_time(
    hour: i64,
    minute: i64,
    second: i64,
    microsecond: i64,
    tz: TzSpec,
) -> Result<TimeFields> {
    let field = |value: i64, max: i64, message: &str| -> Result<i32> {
        if (0..=max).contains(&value) {
            i32::try_from(value).map_err(|_| Error::value(message))
        } else {
            Err(PyError::value_error(message).into())
        }
    };
    Ok(TimeFields {
        hour: field(hour, 23, "hour must be in 0..23")?,
        minute: field(minute, 59, "minute must be in 0..59")?,
        second: field(second, 59, "second must be in 0..59")?,
        microsecond: field(microsecond, 999_999, "microsecond must be in 0..999999")?,
        tz,
    })
}

/// `parse_time(text)` for a `str`.
///
/// # Errors
/// `ISO8601Error('Unrecognised ISO 8601 time format: %r')`, `time`'s ValueErrors, or a
/// `decimal` signal.
pub fn parse_time(text: &[u32]) -> Result<TimeFields> {
    if let Some((form, caps)) = match_time(text) {
        let tz = tzspec_of(&caps);
        return match form {
            TimeForm::Second => {
                let (second, us) = split_seconds(caps.name("second").unwrap_or_default());
                check_time(
                    rx::small_int(caps.name("hour")),
                    rx::small_int(caps.name("minute")),
                    second,
                    us,
                    tz,
                )
            }
            TimeForm::Minute => {
                let minute = decimal_group(&caps, "minute")?;
                let sixty = Decimal::from_bigint(&BigInt::from(60));
                let second = floor_micro(&minute.sub(&trunc(&minute)?)?.mul(&sixty)?)?;
                let us = microseconds(&second)?;
                check_time(
                    rx::small_int(caps.name("hour")),
                    to_field(&minute.to_bigint()?),
                    to_field(&second.to_bigint()?),
                    us,
                    tz,
                )
            }
            TimeForm::Hour => {
                let hour = decimal_group(&caps, "hour")?;
                let sixty = Decimal::from_bigint(&BigInt::from(60));
                let minute = hour.sub(&trunc(&hour)?)?.mul(&sixty)?;
                let second = minute.sub(&trunc(&minute)?)?.mul(&sixty)?;
                let us = microseconds(&second)?;
                check_time(
                    to_field(&hour.to_bigint()?),
                    to_field(&minute.to_bigint()?),
                    to_field(&second.to_bigint()?),
                    us,
                    tz,
                )
            }
        };
    }
    Err(repr_message("Unrecognised ISO 8601 time format: ", text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hand_matcher_is_pc_re() -> Result<()> {
        let texts = rx::sample_texts(
            &[
                "15:27:46.123456+01:00",
                "152746,5-0530",
                "T15:27Z",
                "1527.5+01",
                "15,25",
                "T15",
                "15:27:46\n",
                "23:59:60.999999999Z",
            ],
            30_000,
        );
        let names = [
            "hour", "minute", "second", "tzname", "tzsign", "tzhour", "tzmin",
        ];
        for (i, (_, pattern)) in patterns()?.iter().enumerate() {
            let (form, colons) = MATCH_ORDER[i];
            rx::assert_same_groups(pattern, &texts, &names, |text| {
                match_form(text, form, colons)
            });
        }
        Ok(())
    }

    #[test]
    fn split_seconds_is_decimal() -> Result<()> {
        let mut fraction = String::new();
        for n in 0..400_u64 {
            let whole = format!("{:02}", n % 100);
            for sep in [None, Some(','), Some('.')] {
                let text = match sep {
                    None => whole.clone(),
                    Some(sep) => format!("{whole}{sep}{fraction}"),
                };
                let decimal_text: String = text.replace(',', ".");
                let second = floor_micro(&Decimal::parse(&cps(&decimal_text))?)?;
                let expected = (to_field(&second.to_bigint()?), microseconds(&second)?);
                assert_eq!(split_seconds(&cps(&text)), expected, "{text}");
            }
            // Fractions of 1 to 40 digits, varied.
            fraction.push(char::from(
                b'0' + u8::try_from((n * 7 + 3) % 10).unwrap_or(0),
            ));
            if fraction.len() > 40 {
                fraction = format!("{}", n % 10);
            }
        }
        Ok(())
    }

    #[test]
    fn fractions() -> Result<()> {
        let t = parse_time(&cps("23:20:50,5"))?;
        assert_eq!(
            (t.hour, t.minute, t.second, t.microsecond),
            (23, 20, 50, 500_000)
        );
        // Rounds up to a full second: upstream's microsecond overflow.
        assert!(parse_time(&cps("23.9999999999999")).is_err());
        assert!(parse_time(&cps("24")).is_err());
        assert_eq!(parse_time(&cps("12Z"))?.tz, TzSpec::Utc);
        Ok(())
    }
}
