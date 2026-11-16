//! `isodates.build_date_regexps` and `parse_date`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use num_bigint::BigInt;
use pc_datetime_port_core::date::date_new;
use pc_datetime_port_core::date::Date;
use pc_datetime_port_core::Value;
use pc_re_port_core::Pattern;

use super::{cps, repr_message};
use crate::error::Result;
use crate::rx::{self, Groups, Named};

/// Which of upstream's date formats a pattern recognises (its named groups).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DateForm {
    /// `sign`, `year`, `month`, `day`.
    Complete,
    /// `sign`, `year`, `week`, `day`.
    WeekDay,
    /// `sign`, `year`, `day`.
    Ordinal,
    /// `sign`, `year`, `week`.
    Week,
    /// `sign`, `year`, `month`.
    Month,
    /// `sign`, `year`.
    Year,
    /// `sign`, `century`.
    Century,
}

/// The suffixes after `(?P<sign>[+-]){S}(?P<year>[0-9]{Y})`, in upstream's order.
const YEAR_FORMS: [(DateForm, &str); 11] = [
    (
        DateForm::Complete,
        r"-(?P<month>[0-9]{2})-(?P<day>[0-9]{2})",
    ),
    (DateForm::Complete, r"(?P<month>[0-9]{2})(?P<day>[0-9]{2})"),
    (DateForm::WeekDay, r"-W(?P<week>[0-9]{2})-(?P<day>[0-9]{1})"),
    (DateForm::WeekDay, r"W(?P<week>[0-9]{2})(?P<day>[0-9]{1})"),
    (DateForm::Ordinal, r"-(?P<day>[0-9]{3})"),
    (DateForm::Ordinal, r"(?P<day>[0-9]{3})"),
    (DateForm::Week, r"-W(?P<week>[0-9]{2})"),
    (DateForm::Week, r"W(?P<week>[0-9]{2})"),
    (DateForm::Month, r"-(?P<month>[0-9]{2})"),
    (DateForm::Month, r"(?P<month>[0-9]{2})"),
    (DateForm::Year, ""),
];

/// The compiled patterns of one `build_date_regexps(yeardigits, expanded)` call.
#[derive(Debug)]
pub struct DateRegexps {
    patterns: Vec<(DateForm, Arc<Pattern>)>,
    /// The repeat counts when the pattern texts are plain small numbers, so the
    /// patterns can be matched by hand.
    shape: Option<Shape>,
}

/// The `{n}` counts of one set of date patterns.
#[derive(Debug, Clone, Copy)]
struct Shape {
    /// Signs: 0 or 1.
    sign: usize,
    /// Year digits.
    year: usize,
    /// Century digits.
    century: usize,
}

/// One `YEAR_FORMS` suffix, as hand-matched pieces.
#[derive(Debug, Clone, Copy)]
enum Piece {
    /// A literal character.
    Lit(char),
    /// A named group of exactly `n` digits.
    Digits(&'static str, usize),
}

/// `YEAR_FORMS`, piece by piece, in the same order.
const SUFFIX_PIECES: [(DateForm, &[Piece]); 11] = {
    use Piece::{Digits, Lit};
    [
        (
            DateForm::Complete,
            &[Lit('-'), Digits("month", 2), Lit('-'), Digits("day", 2)],
        ),
        (DateForm::Complete, &[Digits("month", 2), Digits("day", 2)]),
        (
            DateForm::WeekDay,
            &[
                Lit('-'),
                Lit('W'),
                Digits("week", 2),
                Lit('-'),
                Digits("day", 1),
            ],
        ),
        (
            DateForm::WeekDay,
            &[Lit('W'), Digits("week", 2), Digits("day", 1)],
        ),
        (DateForm::Ordinal, &[Lit('-'), Digits("day", 3)]),
        (DateForm::Ordinal, &[Digits("day", 3)]),
        (DateForm::Week, &[Lit('-'), Lit('W'), Digits("week", 2)]),
        (DateForm::Week, &[Lit('W'), Digits("week", 2)]),
        (DateForm::Month, &[Lit('-'), Digits("month", 2)]),
        (DateForm::Month, &[Digits("month", 2)]),
        (DateForm::Year, &[]),
    ]
};

impl Shape {
    /// The counts behind the pattern texts, if they are plain numbers of a few digits
    /// (`{n}` then means exactly `n`).
    fn of(expanded: bool, year_text: &[u32], century_text: &[u32]) -> Option<Self> {
        let count = |text: &[u32]| -> Option<usize> {
            if text.is_empty() || text.len() > 3 {
                return None;
            }
            text.iter().try_fold(0_usize, |acc, &c| {
                let digit = c.checked_sub(0x30).filter(|d| *d <= 9)?;
                Some(acc * 10 + usize::try_from(digit).ok()?)
            })
        };
        Some(Self {
            sign: usize::from(expanded),
            year: count(year_text)?,
            century: count(century_text)?,
        })
    }

    /// `\A(?P<sign>[+-]){S}(?P<lead>[0-9]{n})`: the groups so far and the end.
    fn head<'t>(
        self,
        text: &'t [u32],
        lead: &'static str,
        n: usize,
    ) -> Option<(Groups<'t>, usize)> {
        let mut groups = Groups::new(text);
        let mut i = 0;
        if self.sign == 1 {
            if !rx::is_at(text, 0, "+-") {
                return None;
            }
            groups.set("sign", 0, 1);
            i = 1;
        }
        let end = rx::digits(text, i, n)?;
        groups.set(lead, i, end);
        Some((groups, end))
    }

    /// The first pattern `text` matches (as `pattern.match` with `\A...\Z`).
    fn find(self, text: &[u32]) -> Option<(DateForm, Groups<'_>)> {
        for (form, pieces) in SUFFIX_PIECES {
            let Some((mut groups, mut i)) = self.head(text, "year", self.year) else {
                break;
            };
            let matched = pieces.iter().all(|piece| match *piece {
                Piece::Lit(c) => {
                    let found = text.get(i) == Some(&u32::from(c));
                    i += 1;
                    found
                }
                Piece::Digits(name, n) => match rx::digits(text, i, n) {
                    Some(end) => {
                        groups.set(name, i, end);
                        i = end;
                        true
                    }
                    None => false,
                },
            });
            if matched && i == text.len() {
                return Some((form, groups));
            }
        }
        let (groups, end) = self.head(text, "century", self.century)?;
        (end == text.len()).then_some((DateForm::Century, groups))
    }
}

impl DateRegexps {
    /// The patterns, in the order `parse_date` tries them.
    pub fn patterns(&self) -> impl Iterator<Item = &Arc<Pattern>> {
        self.patterns.iter().map(|(_, p)| p)
    }

    fn build(expanded: bool, year_text: &[u32], century_text: &[u32]) -> Result<Self> {
        let sign = cps(if expanded { "1" } else { "0" });
        let head = |group: &str, digits: &[u32]| -> Vec<u32> {
            let mut out = cps(r"\A(?P<sign>[+-]){");
            out.extend_from_slice(&sign);
            out.extend(cps(&format!("}}(?P<{group}>[0-9]{{")));
            out.extend_from_slice(digits);
            out.push(u32::from('}'));
            out.push(u32::from(')'));
            out
        };
        let mut patterns = Vec::with_capacity(YEAR_FORMS.len() + 1);
        for (form, suffix) in YEAR_FORMS {
            let mut source = head("year", year_text);
            source.extend(cps(suffix));
            source.extend(cps(r"\Z"));
            patterns.push((form, rx::compile(&source)?));
        }
        let mut source = head("century", century_text);
        source.extend(cps(r"\Z"));
        patterns.push((DateForm::Century, rx::compile(&source)?));
        Ok(Self {
            patterns,
            shape: Shape::of(expanded, year_text, century_text),
        })
    }
}

/// Cache key: `expanded`, then the `%d` texts of `yeardigits` and `yeardigits - 2`.
type CacheKey = (bool, Vec<u32>, Vec<u32>);

static CACHE: OnceLock<Mutex<HashMap<CacheKey, Arc<DateRegexps>>>> = OnceLock::new();
static DEFAULT: OnceLock<Arc<DateRegexps>> = OnceLock::new();

/// `build_date_regexps` given the pattern texts it substitutes: `expanded` is the
/// sign count (already forced for `yeardigits != 4`), `year_text` is `'%d' % yeardigits`
/// and `century_text` is `'%d' % (yeardigits - 2)`. Compiled once per key.
///
/// # Errors
/// `re.compile`'s errors (`OverflowError` for huge repeat counts).
pub fn date_regexps(
    expanded: bool,
    year_text: &[u32],
    century_text: &[u32],
) -> Result<Arc<DateRegexps>> {
    if !expanded && year_text == [u32::from('4')] && century_text == [u32::from('2')] {
        return default_date_regexps().map(Arc::clone);
    }
    // One lock for lookup and insert, so concurrent callers never build an entry twice.
    let mut cache = CACHE
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let key = (expanded, year_text.to_vec(), century_text.to_vec());
    if let Some(found) = cache.get(&key) {
        return Ok(Arc::clone(found));
    }
    let built = Arc::new(DateRegexps::build(expanded, year_text, century_text)?);
    cache.insert(key, Arc::clone(&built));
    Ok(built)
}

/// `build_date_regexps()` (4 digit years, no sign).
///
/// # Errors
/// None in practice; compile errors are reported as `re` would.
pub fn default_date_regexps() -> Result<&'static Arc<DateRegexps>> {
    if let Some(found) = DEFAULT.get() {
        return Ok(found);
    }
    let built = Arc::new(DateRegexps::build(false, &cps("4"), &cps("2"))?);
    Ok(DEFAULT.get_or_init(|| built))
}

fn group(caps: &impl Named, name: &str) -> BigInt {
    caps.name(name).map(rx::big_int).unwrap_or_default()
}

/// `int(groups[name])` for the year or century group, which odd `yeardigits` can leave
/// empty (`{0}`) or holding a literal `{-1}`.
fn int_group(caps: &impl Named, name: &str) -> Result<BigInt> {
    let text = caps.name(name).unwrap_or_default();
    if !text.is_empty() && text.iter().all(|&c| (0x30..=0x39).contains(&c)) {
        return Ok(rx::big_int(text));
    }
    let repr = crate::PyStr::from_code_points(text.to_vec()).repr();
    Err(crate::Error::value(format!(
        "invalid literal for int() with base 10: {repr}"
    )))
}

/// `parse_date(text, ...)` with the patterns of `build_date_regexps(yeardigits, expanded)`.
/// `default_month` and `default_day` are passed to `date()` as given.
///
/// # Errors
/// `ISO8601Error('Unrecognised ISO 8601 date format: %r')`, or `date`'s
/// TypeError/ValueError/OverflowError for the matched fields.
pub fn parse_date(
    text: &[u32],
    regexps: &DateRegexps,
    default_month: &Value,
    default_day: &Value,
) -> Result<Date> {
    if let Some(shape) = regexps.shape {
        return match shape.find(text) {
            Some((form, caps)) => date_of(form, &caps, default_month, default_day),
            None => Err(repr_message("Unrecognised ISO 8601 date format: ", text)),
        };
    }
    let string: Arc<[u32]> = Arc::from(text);
    for (form, pattern) in &regexps.patterns {
        if let Some(caps) = rx::run(pattern, &string, pc_re_port_core::matching::Mode::Match)? {
            return date_of(*form, &caps, default_month, default_day);
        }
    }
    Err(repr_message("Unrecognised ISO 8601 date format: ", text))
}

/// `int(group)` when the group is 1 to 9 ASCII digits, so the value (and its negation)
/// fits a C `int`; `None` otherwise (absent, empty, or longer).
fn c_int_group(caps: &impl Named, name: &str) -> Option<i64> {
    let text = caps.name(name)?;
    if text.is_empty() || text.len() > 9 {
        return None;
    }
    text.iter().try_fold(0_i64, |acc, &c| {
        let digit = c.checked_sub(0x30).filter(|d| *d <= 9)?;
        Some(acc * 10 + i64::from(digit))
    })
}

/// [`date_of`] without `BigInt`s, for the forms whose fields are all read from groups
/// that fit a C `int`. `date_new` converts such ints with `as_c_int` (which can't fail
/// for them) and then calls `Date::new`, so calling `Date::new` directly is the same.
/// `None` when the slow path is needed.
fn quick_date(form: DateForm, caps: &impl Named, negative: bool) -> Option<Result<Date>> {
    let year = c_int_group(caps, "year").map(|y| if negative { -y } else { y });
    let new = |y: i64, m: i64, d: i64| Date::new(y, m, d).map_err(crate::Error::from);
    match form {
        DateForm::Complete => {
            let month = c_int_group(caps, "month").filter(|m| *m != 0)?;
            let day = c_int_group(caps, "day")?;
            Some(new(year?, month, day))
        }
        DateForm::WeekDay | DateForm::Week | DateForm::Ordinal => {
            let start = match new(year?, 1, 1) {
                Ok(start) => start,
                Err(e) => return Some(Err(e)),
            };
            // Present but too long for the quick path: let the slow path read it.
            let field = |name: &str| match caps.name(name) {
                None => Some(None),
                Some(_) => c_int_group(caps, name).map(Some),
            };
            let day = field("day")?;
            let week = field("week")?;
            Some((|| -> Result<Date> {
                if form == DateForm::Ordinal {
                    return Ok(start.add_days(day.unwrap_or(0) - 1)?);
                }
                let (_, iso_week, iso_day) = start.iso_calendar();
                let day = if form == DateForm::WeekDay {
                    day.unwrap_or(0)
                } else {
                    1
                };
                let weeks = week.unwrap_or(0) - i64::from(iso_week == 1);
                Ok(start.add_days(weeks * 7 - iso_day + day)?)
            })())
        }
        _ => None,
    }
}

/// The date for a match of `form`'s pattern.
fn date_of(
    form: DateForm,
    caps: &impl Named,
    default_month: &Value,
    default_day: &Value,
) -> Result<Date> {
    let negative = caps.name("sign") == Some(&[u32::from('-')][..]);
    if let Some(quick) = quick_date(form, caps, negative) {
        return quick;
    }
    slow_date_of(form, caps, negative, default_month, default_day)
}

/// [`date_of`] through `BigInt`s and `date_new`, for any match.
fn slow_date_of(
    form: DateForm,
    caps: &impl Named,
    negative: bool,
    default_month: &Value,
    default_day: &Value,
) -> Result<Date> {
    let signed = |n: BigInt| if negative { -n } else { n };
    let int = |n: BigInt| Value::Int(n);
    Ok(match form {
        DateForm::Century => {
            let year = signed(int_group(caps, "century")? * 100 + 1);
            date_new(&[&int(year), default_month, default_day])?
        }
        DateForm::WeekDay | DateForm::Week | DateForm::Ordinal | DateForm::Year => {
            let one = Value::int(1);
            let start = date_new(&[&int(signed(int_group(caps, "year")?)), &one, &one])?;
            match form {
                DateForm::Ordinal => start.add_days(small(&group(caps, "day")) - 1)?,
                DateForm::Year => {
                    let year = Value::int(start.year());
                    date_new(&[&year, default_month, default_day])?
                }
                _ => {
                    let (_, iso_week, iso_day) = start.iso_calendar();
                    let day = if form == DateForm::WeekDay {
                        small(&group(caps, "day"))
                    } else {
                        1
                    };
                    // Upstream subtracts a week when Jan 1 is in week 1.
                    let weeks = small(&group(caps, "week")) - i64::from(iso_week == 1);
                    start.add_days(weeks * 7 - iso_day + day)?
                }
            }
        }
        DateForm::Complete | DateForm::Month => {
            let month = group(caps, "month");
            let month = if month == BigInt::from(0) {
                default_month.clone()
            } else {
                int(month)
            };
            let day = match caps.name("day") {
                Some(day) => int(rx::big_int(day)),
                None => default_day.clone(),
            };
            date_new(&[&int(signed(int_group(caps, "year")?)), &month, &day])?
        }
    })
}

/// A field of at most three digits.
fn small(n: &BigInt) -> i64 {
    i64::try_from(n).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(text: &str) -> Result<Date> {
        let one = Value::int(1);
        parse_date(&cps(text), default_date_regexps()?, &one, &one)
    }

    use pc_re_port_core::matching::Mode;

    #[test]
    fn shapes_are_pc_re() -> Result<()> {
        let texts = rx::sample_texts(
            &[
                "1985-04-12",
                "19850412",
                "1985-W15-5",
                "1985W155",
                "1985-102",
                "1985102",
                "1985-W15",
                "1985W15",
                "1985-04",
                "198504",
                "1985",
                "19",
                "+001985-04-12",
                "-0019850412",
                "+12345-W01",
                "-19",
                "+0019",
                "+1",
                "",
            ],
            15_000,
        );
        let names = ["sign", "year", "century", "month", "day", "week"];
        for (expanded, yeardigits) in [
            (false, 4),
            (true, 4),
            (true, 6),
            (true, 5),
            (true, 2),
            (true, 3),
        ] {
            let year = cps(&yeardigits.to_string());
            let century = cps(&(yeardigits - 2).to_string());
            let built = DateRegexps::build(expanded, &year, &century)?;
            let shape = built.shape.ok_or_else(|| crate::Error::value("no shape"))?;
            for text in &texts {
                let string: Arc<[u32]> = Arc::from(text.as_slice());
                let mut slow = None;
                for (form, pattern) in &built.patterns {
                    if let Some(caps) = rx::run(pattern, &string, Mode::Match)? {
                        slow = Some((*form, caps));
                        break;
                    }
                }
                let quick = shape.find(text);
                assert_eq!(slow.is_some(), quick.is_some(), "{text:?}");
                if let (Some((slow_form, slow)), Some((quick_form, quick))) = (slow, quick) {
                    assert_eq!(slow_form, quick_form, "{text:?}");
                    for name in names {
                        assert_eq!(
                            slow.name(name),
                            Named::name(&quick, name),
                            "{name} in {text:?}"
                        );
                    }
                }
            }
        }
        // Texts that aren't plain counts keep the regex.
        assert!(Shape::of(true, &cps("-1"), &cps("-3")).is_none());
        assert!(Shape::of(true, &cps("1000"), &cps("998")).is_none());
        Ok(())
    }

    #[test]
    fn quick_date_is_slow_date() -> Result<()> {
        let texts = rx::sample_texts(
            &[
                "1985-04-12",
                "19850412",
                "1985-W15-5",
                "1985W155",
                "1985-102",
                "1985102",
                "1985-W15",
                "1985W15",
                "1985-04",
                "198504",
                "1985",
                "19",
                "0000-00-00",
                "9999-12-31",
                "2009-W53-7",
                "2004-366",
                "-0001-W01",
                "+001985-04-12",
                "-0019850412",
                "+12345-W01",
                "+999999-999",
                "-123456789-W99-9",
                "+1234567890-01-01",
            ],
            15_000,
        );
        let one = Value::int(1);
        let other = Value::Float(2.5);
        let mut matched = 0;
        for (expanded, yeardigits) in [(false, 4), (true, 4), (true, 6), (true, 9), (true, 10)] {
            let year = cps(&yeardigits.to_string());
            let century = cps(&(yeardigits - 2).to_string());
            let built = DateRegexps::build(expanded, &year, &century)?;
            let shape = built.shape.ok_or_else(|| crate::Error::value("no shape"))?;
            for text in &texts {
                let Some((form, caps)) = shape.find(text) else {
                    continue;
                };
                let negative = caps.name("sign") == Some(&[u32::from('-')][..]);
                matched += 1;
                for default in [&one, &other] {
                    let quick = date_of(form, &caps, default, default);
                    let slow = slow_date_of(form, &caps, negative, default, default);
                    assert_eq!(quick, slow, "{text:?}");
                }
            }
        }
        assert!(matched > 2_000, "only {matched} matches");
        Ok(())
    }

    #[test]
    fn formats() -> Result<()> {
        for text in [
            "1985-04-12",
            "19850412",
            "1985-W15-5",
            "1985W155",
            "1985-102",
            "1985102",
        ] {
            assert_eq!(date(text)?, Date::new(1985, 4, 12)?, "{text}");
        }
        assert_eq!(date("1985-W15")?, Date::new(1985, 4, 8)?);
        assert_eq!(date("19")?, Date::new(1901, 1, 1)?);
        assert_eq!(date("2009-W01-1")?, Date::new(2008, 12, 29)?);
        assert!(date("x").is_err());
        Ok(())
    }
}
