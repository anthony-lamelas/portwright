//! `parse_duration`.

use num_bigint::BigInt;
use pc_datetime_port_core::timedelta::{new_delta, TimeDelta, Unit};
use pc_datetime_port_core::Value;

#[cfg(test)]
use super::cps;
use super::{parse_datetime, repr_message};
use crate::decimal::Decimal;
use crate::error::Result;
#[cfg(test)]
use crate::rx::Regex;
use crate::rx::{self, Named};

/// `isoduration.ISO8601_PERIOD_REGEX`, which [`match_period`] matches by hand.
#[cfg(test)]
static PERIOD: Regex = Regex::new(concat!(
    r"^(?P<sign>[+-])?",
    r"P(?!\b)",
    r"(?P<years>[0-9]+([,.][0-9]+)?Y)?",
    r"(?P<months>[0-9]+([,.][0-9]+)?M)?",
    r"(?P<weeks>[0-9]+([,.][0-9]+)?W)?",
    r"(?P<days>[0-9]+([,.][0-9]+)?D)?",
    r"((?P<separator>T)(?P<hours>[0-9]+([,.][0-9]+)?H)?",
    r"(?P<minutes>[0-9]+([,.][0-9]+)?M)?",
    r"(?P<seconds>[0-9]+([,.][0-9]+)?S)?)?$",
));

/// `PERIOD.match(text)` by hand.
///
/// Each designator group is `[0-9]+([,.][0-9]+)?` plus its own letter, and only that
/// group can take a number ending in that letter, so the greedy pass is the only way the
/// pattern can match. `(?!\b)` after `P` needs a word character next. Any other
/// character than a digit or `T` there fails the rest of the pattern anyway, so only
/// those two are accepted.
fn match_period(text: &[u32]) -> Option<PeriodMatch<'_>> {
    let mut groups = PeriodMatch {
        text,
        spans: [None; PERIOD_GROUPS.len()],
    };
    let mut i = 0;
    if rx::is_at(text, 0, "+-") {
        groups.spans[SIGN] = Some((0, 1));
        i = 1;
    }
    if !rx::is_at(text, i, "P") {
        return None;
    }
    i += 1;
    if !rx::is_at(text, i, "0123456789T") {
        return None;
    }
    let mut designator = |slot: usize, letter: &str, i: usize| -> usize {
        match rx::number(text, i) {
            Some(end) if rx::is_at(text, end, letter) => {
                if let Some(span) = groups.spans.get_mut(slot) {
                    *span = Some((i, end + 1));
                }
                end + 1
            }
            _ => i,
        }
    };
    i = designator(YEARS, "Y", i);
    i = designator(MONTHS, "M", i);
    i = designator(WEEKS, "W", i);
    i = designator(DAYS, "D", i);
    if rx::is_at(text, i, "T") {
        let separator = i;
        i = designator(HOURS, "H", i + 1);
        i = designator(MINUTES, "M", i);
        i = designator(SECONDS, "S", i);
        groups.spans[SEPARATOR] = Some((separator, separator + 1));
    }
    // `$`: the end, or a final newline.
    let at_end = i == text.len() || (i + 1 == text.len() && rx::is_at(text, i, "\n"));
    at_end.then_some(groups)
}

/// `PERIOD`'s named groups, in the order of [`PeriodMatch`]'s slots.
const PERIOD_GROUPS: [&str; 9] = [
    "sign",
    "years",
    "months",
    "weeks",
    "days",
    "separator",
    "hours",
    "minutes",
    "seconds",
];
const SIGN: usize = 0;
const YEARS: usize = 1;
const MONTHS: usize = 2;
const WEEKS: usize = 3;
const DAYS: usize = 4;
const SEPARATOR: usize = 5;
const HOURS: usize = 6;
const MINUTES: usize = 7;
const SECONDS: usize = 8;

/// The groups [`match_period`] found, one slot per entry of [`PERIOD_GROUPS`], so
/// reading one is an index rather than a search by name.
#[derive(Debug, Clone)]
struct PeriodMatch<'t> {
    text: &'t [u32],
    spans: [Option<(usize, usize)>; PERIOD_GROUPS.len()],
}

impl<'t> PeriodMatch<'t> {
    /// Group `slot`'s text, or `None` if it didn't take part.
    fn group(&self, slot: usize) -> Option<&'t [u32]> {
        let (start, end) = (*self.spans.get(slot)?)?;
        self.text.get(start..end)
    }
}

impl Named for PeriodMatch<'_> {
    fn name(&self, name: &str) -> Option<&[u32]> {
        self.group(PERIOD_GROUPS.iter().position(|g| *g == name)?)
    }
}

/// The groups of a matched `PnYnMnWnDTnHnMnS` duration, converted as upstream does.
#[derive(Debug, Clone, PartialEq)]
pub struct DurationFields {
    /// The sign group was `-`.
    pub negative: bool,
    /// `Decimal` years.
    pub years: Decimal,
    /// `Decimal` months.
    pub months: Decimal,
    /// `float` weeks.
    pub weeks: f64,
    /// `float` days.
    pub days: f64,
    /// `float` hours.
    pub hours: f64,
    /// `float` minutes.
    pub minutes: f64,
    /// `float` seconds.
    pub seconds: f64,
}

/// What `parse_duration` found before choosing between `timedelta` and `Duration`.
#[derive(Debug, Clone, PartialEq)]
pub enum DurationParse {
    /// The designator format.
    Fields(DurationFields),
    /// The alternative format `P<datetime>`: `Duration(years, months, tdelta)`.
    Alternative {
        /// `Decimal(str(durdt.year))`.
        years: Decimal,
        /// `Decimal(str(durdt.month))`.
        months: Decimal,
        /// `timedelta(days=day, seconds=second, microseconds=..., minutes=, hours=)`.
        tdelta: TimeDelta,
    },
}

/// A group's number with its unit letter dropped (`,` still in place), or `None` when
/// the group didn't take part.
fn number_body(group: Option<&[u32]>) -> Option<&[u32]> {
    let text = group?;
    Some(text.get(..text.len().saturating_sub(1)).unwrap_or_default())
}

/// `Decimal(group)` with `,` made `.`; `Decimal("0")` when absent.
fn decimal_group(group: Option<&[u32]>) -> Result<Decimal> {
    let Some(body) = number_body(group) else {
        return Ok(Decimal::zero());
    };
    let comma = u32::from(',');
    if !body.contains(&comma) {
        return Decimal::parse(body);
    }
    let dotted: Vec<u32> = body
        .iter()
        .map(|&c| if c == comma { u32::from('.') } else { c })
        .collect();
    Decimal::parse(&dotted)
}

/// `float(text)` for `[0-9]+([,.][0-9]+)?` with `,` made `.`; `0.0` when absent.
/// Rust's parse is correctly rounded like CPython's.
fn float_group(group: Option<&[u32]>) -> f64 {
    let Some(body) = number_body(group) else {
        return 0.0;
    };
    if let Some(value) = short_float(body) {
        return value;
    }
    let ascii = |c: u32| match char::from_u32(c) {
        Some(',') => Some(b'.'),
        Some(c) if c.is_ascii() => u8::try_from(c).ok(),
        _ => None,
    };
    let mut buffer = [0_u8; 64];
    if body.len() <= buffer.len() {
        let mut len = 0;
        for (slot, &c) in buffer.iter_mut().zip(body) {
            let Some(b) = ascii(c) else {
                return 0.0;
            };
            *slot = b;
            len += 1;
        }
        return std::str::from_utf8(buffer.get(..len).unwrap_or_default())
            .ok()
            .and_then(|t| t.parse().ok())
            .unwrap_or(0.0);
    }
    let text: Option<String> = body.iter().map(|&c| ascii(c).map(char::from)).collect();
    text.and_then(|t| t.parse().ok()).unwrap_or(0.0)
}

/// Powers of ten that `f64` holds exactly (`10**0` to `10**15`).
const EXACT_POW10: [f64; 16] = [
    1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15,
];

/// `float(text)` for `[0-9]+([,.][0-9]+)?` with at most 15 digits, or `None` for
/// anything else.
///
/// The digits make an integer below `2**53` and the scale a power of ten up to `10**15`;
/// both are exact in `f64`, so one IEEE division gives the correctly rounded value,
/// which is what `str::parse` (and CPython's `float`) return (Clinger's fast path).
fn short_float(body: &[u32]) -> Option<f64> {
    let mut mantissa: u64 = 0;
    let mut digits = 0_usize;
    let mut fraction: Option<usize> = None;
    for &c in body {
        if (c == u32::from('.') || c == u32::from(',')) && fraction.is_none() {
            fraction = Some(0);
            continue;
        }
        let digit = c.checked_sub(u32::from('0')).filter(|d| *d <= 9)?;
        digits += 1;
        if digits > 15 {
            return None;
        }
        mantissa = mantissa * 10 + u64::from(digit);
        if let Some(f) = fraction.as_mut() {
            *f += 1;
        }
    }
    if digits == 0 {
        return None;
    }
    // `mantissa < 10**15 < 2**53`: both halves and the sum are exact.
    let high = u32::try_from(mantissa >> 32).ok()?;
    let low = u32::try_from(mantissa & 0xFFFF_FFFF).ok()?;
    let value = f64::from(high) * 4_294_967_296.0 + f64::from(low);
    match fraction {
        None | Some(0) => Some(value),
        Some(k) => Some(value / EXACT_POW10.get(k)?),
    }
}

/// Matches `text` and converts its groups, or parses the alternative format.
///
/// # Errors
/// `ISO8601Error('Unable to parse duration string %r')`, or `parse_datetime`'s errors on
/// `text[1:]` for the alternative format.
pub fn parse_duration_text(text: &[u32]) -> Result<DurationParse> {
    let Some(caps) = match_period(text) else {
        if let Some(rest) = text.strip_prefix(&[u32::from('P')]) {
            let (date, time) = parse_datetime(rest)?;
            let small = |n: i32| Value::int(n);
            let fields = [
                (Unit::Days, small(date.day())),
                (Unit::Seconds, small(time.second)),
                (Unit::Microseconds, small(time.microsecond)),
                (Unit::Minutes, small(time.minute)),
                (Unit::Hours, small(time.hour)),
            ];
            let parts: Vec<(Unit, &Value)> = fields.iter().map(|(u, v)| (*u, v)).collect();
            return Ok(DurationParse::Alternative {
                years: Decimal::from_bigint(&BigInt::from(date.year())),
                months: Decimal::from_bigint(&BigInt::from(date.month())),
                tdelta: new_delta(&parts)?,
            });
        }
        return Err(repr_message("Unable to parse duration string ", text));
    };
    Ok(DurationParse::Fields(DurationFields {
        negative: caps.group(SIGN) == Some(&[u32::from('-')][..]),
        years: decimal_group(caps.group(YEARS))?,
        months: decimal_group(caps.group(MONTHS))?,
        weeks: float_group(caps.group(WEEKS)),
        days: float_group(caps.group(DAYS)),
        hours: float_group(caps.group(HOURS)),
        minutes: float_group(caps.group(MINUTES)),
        seconds: float_group(caps.group(SECONDS)),
    }))
}

/// How a designator-format duration is returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurationKind {
    /// `timedelta(days=, hours=, minutes=, seconds=, weeks=)`.
    TimeDelta,
    /// `Duration(...)`, whose timedelta also gets `microseconds=0, milliseconds=0`.
    Duration,
}

/// The timedelta of `fields`, before the sign is applied.
///
/// # Errors
/// `timedelta`'s OverflowError for huge or infinite values.
pub fn duration_timedelta(fields: &DurationFields, kind: DurationKind) -> Result<TimeDelta> {
    // The `0` microseconds and milliseconds of `DurationKind::Duration` add nothing.
    if let Some(delta) = small_float_delta(fields) {
        return Ok(delta);
    }
    let float = Value::Float;
    let zero = Value::int(0);
    let values = [
        (Unit::Days, float(fields.days)),
        (Unit::Hours, float(fields.hours)),
        (Unit::Minutes, float(fields.minutes)),
        (Unit::Seconds, float(fields.seconds)),
        (Unit::Weeks, float(fields.weeks)),
    ];
    let [days, hours, minutes, seconds, weeks] = &values;
    let common = [days, hours, minutes, seconds, weeks].map(|(u, v)| (*u, v));
    if kind == DurationKind::Duration {
        let [a, b, c, d, e] = common;
        return Ok(new_delta(&[
            a,
            b,
            c,
            d,
            e,
            (Unit::Microseconds, &zero),
            (Unit::Milliseconds, &zero),
        ])?);
    }
    Ok(new_delta(&common)?)
}

/// [`duration_timedelta`] in `i64`, for fields small enough that nothing overflows, or
/// `None` (the caller then runs `new_delta`, which raises any error).
///
/// The steps are CPython's `delta_new`/`accum` for floats, in its unit order: the integer
/// part times the unit's factor is exact; the fraction times the factor is split again,
/// its whole part added at once and the rest kept as `leftover`, which is finally rounded
/// half-to-even against the total's parity.
fn small_float_delta(fields: &DurationFields) -> Option<TimeDelta> {
    const US: i64 = 1_000_000;
    let parts = [
        (fields.seconds, US),
        (fields.minutes, 60 * US),
        (fields.hours, 3_600 * US),
        (fields.days, 86_400 * US),
        (fields.weeks, 7 * 86_400 * US),
    ];
    let mut total: i64 = 0;
    let mut leftover = 0.0_f64;
    for (value, factor) in parts {
        // Every factor is below 2**53, so it is exact as a float.
        let factor_f = num_traits::ToPrimitive::to_f64(&factor)?;
        if !value.is_finite() {
            return None;
        }
        let intpart = num_traits::ToPrimitive::to_i64(&value.trunc())?;
        let fracpart = value.fract();
        let mut add = intpart.checked_mul(factor)?;
        if fracpart != 0.0 {
            let scaled = factor_f * fracpart;
            add = add.checked_add(num_traits::ToPrimitive::to_i64(&scaled.trunc())?)?;
            leftover += scaled.fract();
        }
        total = total.checked_add(add)?;
    }
    if leftover != 0.0 {
        let mut whole = leftover.round();
        if (whole - leftover).abs() == 0.5 {
            let odd = if total % 2 == 0 { 0.0 } else { 1.0 };
            whole = 2.0 * ((leftover + odd) * 0.5).round() - odd;
        }
        total = total.checked_add(num_traits::ToPrimitive::to_i64(&whole)?)?;
    }
    TimeDelta::from_microseconds_i128(i128::from(total)).ok()
}

impl DurationFields {
    /// `years == 0 and months == 0`.
    ///
    /// # Errors
    /// None for parsed values (no NaNs).
    pub fn is_day_based(&self) -> Result<bool> {
        let zero = Decimal::zero();
        Ok(self.years.equals(&zero)? && self.months.equals(&zero)?)
    }

    /// The `timedelta` result, with `timedelta(0) - ret` for a `-` sign.
    ///
    /// # Errors
    /// `timedelta`'s OverflowError.
    pub fn to_timedelta(&self) -> Result<TimeDelta> {
        let delta = duration_timedelta(self, DurationKind::TimeDelta)?;
        if self.negative {
            Ok(delta.negate()?)
        } else {
            Ok(delta)
        }
    }

    /// The `Duration` result's `(years, months, tdelta)`, with `Duration(0) - ret` for a
    /// `-` sign.
    ///
    /// # Errors
    /// `timedelta`'s OverflowError, decimal signals.
    pub fn to_duration(&self) -> Result<(Decimal, Decimal, TimeDelta)> {
        let delta = duration_timedelta(self, DurationKind::Duration)?;
        if !self.negative {
            return Ok((self.years.clone(), self.months.clone(), delta));
        }
        let zero = Decimal::zero();
        Ok((
            zero.sub(&self.years)?,
            zero.sub(&self.months)?,
            delta.negate()?,
        ))
    }
}

/// The value `parse_duration` returns.
#[derive(Debug, Clone, PartialEq)]
pub enum ParsedDuration {
    /// A `timedelta`.
    TimeDelta(TimeDelta),
    /// A `Duration(years=, months=)` holding `tdelta`.
    Duration {
        /// `Duration.years`.
        years: Decimal,
        /// `Duration.months`.
        months: Decimal,
        /// `Duration.tdelta`.
        tdelta: TimeDelta,
    },
}

/// `parse_duration(text, as_timedelta_if_possible)` for a `str`, given the flag's
/// truthiness. (Python evaluates the flag only after parsing; callers needing that order
/// use [`parse_duration_text`].)
///
/// # Errors
/// See [`parse_duration_text`] and [`DurationFields::to_timedelta`].
pub fn parse_duration(text: &[u32], as_timedelta_if_possible: bool) -> Result<ParsedDuration> {
    parse_duration_text(text)?.finish(as_timedelta_if_possible)
}

impl DurationParse {
    /// Builds the result once the flag's truthiness is known.
    ///
    /// # Errors
    /// `timedelta`'s OverflowError, decimal signals.
    pub fn finish(self, as_timedelta_if_possible: bool) -> Result<ParsedDuration> {
        match self {
            Self::Alternative {
                years,
                months,
                tdelta,
            } => Ok(ParsedDuration::Duration {
                years,
                months,
                tdelta,
            }),
            Self::Fields(fields) => {
                if as_timedelta_if_possible && fields.is_day_based()? {
                    return Ok(ParsedDuration::TimeDelta(fields.to_timedelta()?));
                }
                if fields.negative {
                    let (years, months, tdelta) = fields.to_duration()?;
                    return Ok(ParsedDuration::Duration {
                        years,
                        months,
                        tdelta,
                    });
                }
                // `to_duration` without the sign: the fields move instead of being cloned.
                let tdelta = duration_timedelta(&fields, DurationKind::Duration)?;
                Ok(ParsedDuration::Duration {
                    years: fields.years,
                    months: fields.months,
                    tdelta,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_period_is_pc_re() -> Result<()> {
        let texts = rx::sample_texts(
            &[
                "P3Y6M4DT12H30M5S",
                "-P1DT2H3M4.5S",
                "+P1,5Y2W",
                "PT",
                "P1M\n",
                "PT36H",
                "P",
                "P\n",
                "P2W1D",
                "P0.5YT1.5M",
            ],
            40_000,
        );
        rx::assert_same_groups(PERIOD.pattern()?, &texts, &PERIOD_GROUPS, match_period);
        Ok(())
    }

    /// `float(text)` as `str::parse` gives it, `,` read as `.`.
    fn parsed_float(text: &str) -> Option<f64> {
        text.replace(',', ".").parse().ok()
    }

    #[test]
    fn short_float_is_parse() {
        let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
        let mut next = |n: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % n
        };
        let mut texts: Vec<String> = [
            "0",
            "5",
            "4.5",
            "0,1",
            "0.3",
            "1.",
            ".5",
            ".",
            "",
            "1..2",
            "1.2.3",
            "007",
            "999999999999999",
            "9999999999999999",
            "0.000000000000001",
            "1.00000000000000",
            "123456789.123456",
            "12345678901234.5",
            "0.1234567890123456",
        ]
        .iter()
        .map(|t| (*t).to_owned())
        .collect();
        for _ in 0..200_000 {
            let int_len = next(17);
            let frac_len = next(18);
            let mut text: String = (0..int_len)
                .map(|_| char::from(b'0' + u8::try_from(next(10)).unwrap_or(0)))
                .collect();
            if next(3) > 0 {
                text.push(if next(2) == 0 { '.' } else { ',' });
                text.extend(
                    (0..frac_len).map(|_| char::from(b'0' + u8::try_from(next(10)).unwrap_or(0))),
                );
            }
            texts.push(text);
        }
        for text in texts {
            let cps = cps(&text);
            if let Some(quick) = short_float(&cps) {
                let slow = parsed_float(&text);
                assert_eq!(Some(quick.to_bits()), slow.map(f64::to_bits), "{text:?}");
            }
        }
    }

    #[test]
    fn small_float_delta_is_new_delta() {
        let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = |n: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % n
        };
        let specials = [
            0.0,
            0.5,
            1.5,
            2.5,
            0.25,
            0.1,
            0.3,
            4.5,
            1e-7,
            5e-7,
            1.5e-6,
            2.5e-6,
            0.0000005,
            123.456,
            1e9,
            9.99e8,
            1e12,
            1e15,
            3e15,
            1e17,
            1e300,
            f64::INFINITY,
            f64::NAN,
        ];
        let mut value = || -> f64 {
            match next(4) {
                0 => specials
                    .get(usize::try_from(next(23)).unwrap_or(0))
                    .copied()
                    .unwrap_or(0.0),
                1 => f64::from(u32::try_from(next(100_000)).unwrap_or(0)) / 8.0,
                2 => {
                    let digits = u32::try_from(next(1_000_000_000)).unwrap_or(0);
                    f64::from(digits) / 10_f64.powi(i32::try_from(next(12)).unwrap_or(0))
                }
                _ => 0.0,
            }
        };
        for i in 0..300_000 {
            let fields = DurationFields {
                negative: false,
                years: Decimal::zero(),
                months: Decimal::zero(),
                weeks: value(),
                days: value(),
                hours: value(),
                minutes: value(),
                seconds: value(),
            };
            let Some(quick) = small_float_delta(&fields) else {
                continue;
            };
            let float = Value::Float;
            let zero = Value::int(0);
            let values = [
                (Unit::Days, float(fields.days)),
                (Unit::Hours, float(fields.hours)),
                (Unit::Minutes, float(fields.minutes)),
                (Unit::Seconds, float(fields.seconds)),
                (Unit::Weeks, float(fields.weeks)),
            ];
            let mut parts: Vec<(Unit, &Value)> = values.iter().map(|(u, v)| (*u, v)).collect();
            if i % 2 == 0 {
                parts.push((Unit::Microseconds, &zero));
                parts.push((Unit::Milliseconds, &zero));
            }
            assert_eq!(new_delta(&parts).ok(), Some(quick), "{fields:?}");
        }
    }

    #[test]
    fn designators() -> Result<()> {
        let td = parse_duration(&cps("-P1DT2H"), true)?;
        let expected = new_delta(&[(Unit::Hours, &Value::int(-26))])?;
        assert_eq!(td, ParsedDuration::TimeDelta(expected));
        assert!(matches!(
            parse_duration(&cps("P1Y"), true)?,
            ParsedDuration::Duration { .. }
        ));
        assert!(parse_duration(&cps("P1Y\n"), true).is_ok());
        assert!(parse_duration(&cps("P"), true).is_err());
        Ok(())
    }
}
