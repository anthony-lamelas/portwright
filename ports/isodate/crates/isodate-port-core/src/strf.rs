//! `isodate.isostrf`: format constants and `strftime` (task `strf`).
//!
//! The formatter scans the format once, left to right, as `re.sub` does with the
//! upstream alternation, and reads each value from a [`StrfSource`] only when its
//! directive is reached, so the first failing directive decides the error.
//! Values are Python numbers modelled by [`Num`]; `"%02d" % x` coercion, true
//! division and the year-format quirks for odd `yeardigits` follow CPython.

use std::cmp::Ordering;

use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{Signed, ToPrimitive, Zero};

use crate::decimal::{Decimal, Rounding};
use crate::error::{Error, PyStr};
use crate::pc_datetime::date::Date;
use crate::pc_datetime::value::float_repr;
use crate::tz::OffsetFormat;

/// `DATE_BAS_COMPLETE`.
pub const DATE_BAS_COMPLETE: &str = "%Y%m%d";
/// `DATE_EXT_COMPLETE`.
pub const DATE_EXT_COMPLETE: &str = "%Y-%m-%d";
/// `DATE_BAS_WEEK_COMPLETE`.
pub const DATE_BAS_WEEK_COMPLETE: &str = "%YW%W%w";
/// `DATE_EXT_WEEK_COMPLETE`.
pub const DATE_EXT_WEEK_COMPLETE: &str = "%Y-W%W-%w";
/// `DATE_BAS_ORD_COMPLETE`.
pub const DATE_BAS_ORD_COMPLETE: &str = "%Y%j";
/// `DATE_EXT_ORD_COMPLETE`.
pub const DATE_EXT_ORD_COMPLETE: &str = "%Y-%j";
/// `DATE_BAS_WEEK`.
pub const DATE_BAS_WEEK: &str = "%YW%W";
/// `DATE_EXT_WEEK`.
pub const DATE_EXT_WEEK: &str = "%Y-W%W";
/// `DATE_BAS_MONTH`.
pub const DATE_BAS_MONTH: &str = "%Y%m";
/// `DATE_EXT_MONTH`.
pub const DATE_EXT_MONTH: &str = "%Y-%m";
/// `DATE_YEAR`.
pub const DATE_YEAR: &str = "%Y";
/// `DATE_CENTURY`.
pub const DATE_CENTURY: &str = "%C";
/// `TIME_BAS_COMPLETE`.
pub const TIME_BAS_COMPLETE: &str = "%H%M%S";
/// `TIME_EXT_COMPLETE`.
pub const TIME_EXT_COMPLETE: &str = "%H:%M:%S";
/// `TIME_BAS_MINUTE`.
pub const TIME_BAS_MINUTE: &str = "%H%M";
/// `TIME_EXT_MINUTE`.
pub const TIME_EXT_MINUTE: &str = "%H:%M";
/// `TIME_HOUR`.
pub const TIME_HOUR: &str = "%H";
/// `TZ_BAS`.
pub const TZ_BAS: &str = "%z";
/// `TZ_EXT`.
pub const TZ_EXT: &str = "%Z";
/// `TZ_HOUR`.
pub const TZ_HOUR: &str = "%h";
/// `DT_EXT_COMPLETE`.
pub const DT_EXT_COMPLETE: &str = "%Y-%m-%dT%H:%M:%S%Z";
/// `DT_BAS_COMPLETE`.
pub const DT_BAS_COMPLETE: &str = "%Y%m%dT%H%M%S%z";
/// `DT_EXT_ORD_COMPLETE`.
pub const DT_EXT_ORD_COMPLETE: &str = "%Y-%jT%H:%M:%S%Z";
/// `DT_BAS_ORD_COMPLETE`.
pub const DT_BAS_ORD_COMPLETE: &str = "%Y%jT%H%M%S%z";
/// `DT_EXT_WEEK_COMPLETE`.
pub const DT_EXT_WEEK_COMPLETE: &str = "%Y-W%W-%wT%H:%M:%S%Z";
/// `DT_BAS_WEEK_COMPLETE`.
pub const DT_BAS_WEEK_COMPLETE: &str = "%YW%W%wT%H%M%S%z";
/// `D_DEFAULT`.
pub const D_DEFAULT: &str = "P%P";
/// `D_WEEK`.
pub const D_WEEK: &str = "P%p";
/// `D_ALT_EXT`.
pub const D_ALT_EXT: &str = "P%Y-%m-%dT%H:%M:%S";
/// `D_ALT_BAS`.
pub const D_ALT_BAS: &str = "P%Y%m%dT%H%M%S";
/// `D_ALT_EXT_ORD`.
pub const D_ALT_EXT_ORD: &str = "P%Y-%jT%H:%M:%S";
/// `D_ALT_BAS_ORD`.
pub const D_ALT_BAS_ORD: &str = "P%Y%jT%H%M%S";

/// Every format constant of `isodate.isostrf`, by name.
pub const CONSTANTS: &[(&str, &str)] = &[
    ("DATE_BAS_COMPLETE", DATE_BAS_COMPLETE),
    ("DATE_EXT_COMPLETE", DATE_EXT_COMPLETE),
    ("DATE_BAS_WEEK_COMPLETE", DATE_BAS_WEEK_COMPLETE),
    ("DATE_EXT_WEEK_COMPLETE", DATE_EXT_WEEK_COMPLETE),
    ("DATE_BAS_ORD_COMPLETE", DATE_BAS_ORD_COMPLETE),
    ("DATE_EXT_ORD_COMPLETE", DATE_EXT_ORD_COMPLETE),
    ("DATE_BAS_WEEK", DATE_BAS_WEEK),
    ("DATE_EXT_WEEK", DATE_EXT_WEEK),
    ("DATE_BAS_MONTH", DATE_BAS_MONTH),
    ("DATE_EXT_MONTH", DATE_EXT_MONTH),
    ("DATE_YEAR", DATE_YEAR),
    ("DATE_CENTURY", DATE_CENTURY),
    ("TIME_BAS_COMPLETE", TIME_BAS_COMPLETE),
    ("TIME_EXT_COMPLETE", TIME_EXT_COMPLETE),
    ("TIME_BAS_MINUTE", TIME_BAS_MINUTE),
    ("TIME_EXT_MINUTE", TIME_EXT_MINUTE),
    ("TIME_HOUR", TIME_HOUR),
    ("TZ_BAS", TZ_BAS),
    ("TZ_EXT", TZ_EXT),
    ("TZ_HOUR", TZ_HOUR),
    ("DT_EXT_COMPLETE", DT_EXT_COMPLETE),
    ("DT_BAS_COMPLETE", DT_BAS_COMPLETE),
    ("DT_EXT_ORD_COMPLETE", DT_EXT_ORD_COMPLETE),
    ("DT_BAS_ORD_COMPLETE", DT_BAS_ORD_COMPLETE),
    ("DT_EXT_WEEK_COMPLETE", DT_EXT_WEEK_COMPLETE),
    ("DT_BAS_WEEK_COMPLETE", DT_BAS_WEEK_COMPLETE),
    ("D_DEFAULT", D_DEFAULT),
    ("D_WEEK", D_WEEK),
    ("D_ALT_EXT", D_ALT_EXT),
    ("D_ALT_BAS", D_ALT_BAS),
    ("D_ALT_EXT_ORD", D_ALT_EXT_ORD),
    ("D_ALT_BAS_ORD", D_ALT_BAS_ORD),
];

/// Checks `str(n)` against the interpreter's current int→str digit limit
/// (`sys.set_int_max_str_digits`), which the core can't know.
type DigitCheck<'a, E> = &'a dyn Fn(&BigInt) -> Result<(), E>;

/// Longest padded `%d` result (in code points) the port will try to build.
///
/// CPython accepts any width up to `PY_SSIZE_T_MAX` and raises `MemoryError` once the
/// allocation fails. A longer result can't be served here: it is copied into code-point
/// buffers (4 bytes each), and sanitizer allocators abort on requests over 1 TiB instead
/// of failing. So the length is checked before anything is allocated.
const MAX_PADDED_LEN: usize = 1 << 36;

// ------------------------------------------------------------------ numbers

/// A Python value read from the object being formatted.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Num {
    /// `int` (and `bool`).
    Int(BigInt),
    /// `float`.
    Float(f64),
    /// `decimal.Decimal`.
    Decimal(Decimal),
    /// Any other object, by its type's `tp_name` (used in error messages).
    Other(String),
}

impl Num {
    /// `"%d" % self`: the integer the `%d` conversion prints.
    ///
    /// # Errors
    /// `TypeError` for non-numbers, `ValueError`/`OverflowError` for NaN/infinity.
    pub fn to_format_int(&self) -> Result<BigInt, Error> {
        match self {
            Self::Int(n) => Ok(n.clone()),
            Self::Float(f) => float_to_int(*f),
            Self::Decimal(d) => d.to_bigint(),
            Self::Other(tp) => Err(Error::type_error(format!(
                "%d format: a real number is required, not {tp}"
            ))),
        }
    }

    /// `self / n` (true division by an int).
    fn true_div(&self, n: i64) -> Result<Self, Error> {
        match self {
            Self::Int(a) => Ok(Self::Float(int_true_div(a, n)?)),
            Self::Float(f) => Ok(Self::Float(*f / int_as_f64(n))),
            Self::Decimal(d) => Ok(Self::Decimal(d.div(&Decimal::from_bigint(&n.into()))?)),
            Self::Other(tp) => Err(unsupported("/", tp)),
        }
    }

    /// `self // n`.
    fn floor_div(&self, n: i64) -> Result<Self, Error> {
        match self {
            Self::Int(a) => Ok(Self::Int(a.div_floor(&BigInt::from(n)))),
            Self::Float(f) => Ok(Self::Float(float_div_mod(*f, int_as_f64(n)).0)),
            Self::Decimal(d) => {
                let q = d.div(&Decimal::from_bigint(&n.into()))?;
                Ok(Self::Decimal(q.to_integral(Rounding::Down)?))
            }
            Self::Other(tp) => Err(unsupported("//", tp)),
        }
    }

    /// `self % n`.
    fn modulo(&self, n: i64) -> Result<Self, Error> {
        match self {
            Self::Int(a) => Ok(Self::Int(a.mod_floor(&BigInt::from(n)))),
            Self::Float(f) => Ok(Self::Float(float_div_mod(*f, int_as_f64(n)).1)),
            Self::Decimal(d) => Ok(Self::Decimal(d.rem(&Decimal::from_bigint(&n.into()))?)),
            Self::Other(tp) => Err(unsupported("%", tp)),
        }
    }

    /// `self - n`.
    fn minus(&self, n: i64) -> Result<Self, Error> {
        match self {
            Self::Int(a) => Ok(Self::Int(a - n)),
            Self::Float(f) => Ok(Self::Float(*f - int_as_f64(n))),
            Self::Decimal(d) => Ok(Self::Decimal(d.sub(&Decimal::from_bigint(&n.into()))?)),
            Self::Other(tp) => Err(unsupported("-", tp)),
        }
    }

    /// `abs(self)`.
    fn abs(&self) -> Result<Self, Error> {
        match self {
            Self::Int(a) => Ok(Self::Int(a.abs())),
            Self::Float(f) => Ok(Self::Float(f.abs())),
            Self::Decimal(d) => Ok(Self::Decimal(d.abs()?)),
            Self::Other(tp) => Err(Error::type_error(format!(
                "bad operand type for abs(): '{tp}'"
            ))),
        }
    }

    /// `bool(self)`; other objects are assumed truthy.
    fn is_true(&self) -> bool {
        match self {
            Self::Int(a) => !a.is_zero(),
            Self::Float(f) => *f != 0.0,
            Self::Decimal(d) => d.is_true(),
            Self::Other(_) => true,
        }
    }

    /// `self != n`.
    fn ne_int(&self, n: i64) -> bool {
        match self {
            Self::Int(a) => *a != BigInt::from(n),
            Self::Float(f) => *f != int_as_f64(n),
            Self::Decimal(d) => !matches!(d.equals(&Decimal::from_bigint(&n.into())), Ok(true)),
            Self::Other(_) => true,
        }
    }

    /// `str(self)`.
    fn to_str<E: From<Error>>(&self, check: DigitCheck<'_, E>) -> Result<String, E> {
        match self {
            Self::Int(a) => int_to_str(a, check),
            Self::Float(f) => Ok(float_repr(*f)),
            Self::Decimal(d) => Ok(d.to_string()),
            Self::Other(tp) => Err(Error::type_error(format!("cannot format {tp}")).into()),
        }
    }

    /// The int an index-taking C function (`date(year, ...)`) accepts.
    fn to_index(&self) -> Result<BigInt, Error> {
        match self {
            Self::Int(a) => Ok(a.clone()),
            Self::Float(_) => Err(not_an_index("float")),
            Self::Decimal(_) => Err(not_an_index("decimal.Decimal")),
            Self::Other(tp) => Err(not_an_index(tp)),
        }
    }
}

fn not_an_index(tp: &str) -> Error {
    Error::type_error(format!("'{tp}' object cannot be interpreted as an integer"))
}

fn unsupported(op: &str, tp: &str) -> Error {
    Error::type_error(format!(
        "unsupported operand type(s) for {op}: '{tp}' and 'int'"
    ))
}

/// Small ints only (divisors here are 7, 60, 100).
fn int_as_f64(n: i64) -> f64 {
    i32::try_from(n).map(f64::from).unwrap_or(f64::NAN)
}

/// `int(f)`.
fn float_to_int(f: f64) -> Result<BigInt, Error> {
    if f.is_nan() {
        return Err(Error::value("cannot convert float NaN to integer"));
    }
    if f.is_infinite() {
        return Err(Error::overflow("cannot convert float infinity to integer"));
    }
    num_traits::FromPrimitive::from_f64(f.trunc())
        .ok_or_else(|| Error::overflow("cannot convert float infinity to integer"))
}

/// Python `int / int` for a small divisor: exact operands give a correctly rounded quotient.
fn int_true_div(a: &BigInt, n: i64) -> Result<f64, Error> {
    const EXACT: u64 = 1 << 53;
    let divisor = int_as_f64(n);
    // `unsigned_abs`: `i64::MIN.abs()` overflows.
    if let Some(small) = a.to_i64().filter(|v| v.unsigned_abs() <= EXACT) {
        // |small| <= 2**53 converts exactly.
        let numerator = small.to_f64().unwrap_or(f64::NAN);
        return Ok(numerator / divisor);
    }
    let quotient = a.to_f64().map_or(f64::INFINITY, |v| v / divisor);
    if quotient.is_infinite() {
        return Err(Error::overflow(
            "integer division result too large for a float",
        ));
    }
    Ok(quotient)
}

/// CPython's `_float_div_mod`: `(vx // wx, vx % wx)` for floats.
fn float_div_mod(vx: f64, wx: f64) -> (f64, f64) {
    let mut m = vx % wx;
    let mut div = (vx - m) / wx;
    if m != 0.0 {
        if (wx < 0.0) != (m < 0.0) {
            m += wx;
            div -= 1.0;
        }
    } else {
        m = 0.0_f64.copysign(wx);
    }
    let floordiv = if div != 0.0 {
        let mut f = div.floor();
        if div - f > 0.5 {
            f += 1.0;
        }
        f
    } else {
        0.0_f64.copysign(vx / wx)
    };
    (floordiv, m)
}

/// `str(n)`, after `check` applies the interpreter's limit on digits.
fn int_to_str<E: From<Error>>(n: &BigInt, check: DigitCheck<'_, E>) -> Result<String, E> {
    check(n)?;
    Ok(n.to_string())
}

// ------------------------------------------------------------------ "%d"

/// The flags and width of a `%d` conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct IntSpec {
    left: bool,
    zero: bool,
    width: usize,
}

impl IntSpec {
    const fn zero_padded(width: usize) -> Self {
        Self {
            left: false,
            zero: true,
            width,
        }
    }

    /// The spec of `"%%0%dd" % n`: `%0Nd`, or `%0-Nd` (left-justified) for negative `n`.
    fn year<E: From<Error>>(n: &BigInt, check: DigitCheck<'_, E>) -> Result<Self, E> {
        // Building the format string prints `n`, so the digit limit applies.
        check(n)?;
        let width = n
            .magnitude()
            .to_usize()
            .filter(|w| isize::try_from(*w).is_ok())
            .ok_or_else(|| Error::value("width too big"))?;
        Ok(Self {
            left: n.is_negative(),
            zero: true,
            width,
        })
    }

    /// `spec % value` for an int.
    fn format<E: From<Error>>(self, value: &BigInt, check: DigitCheck<'_, E>) -> Result<String, E> {
        let digits = int_to_str(&value.abs(), check)?;
        let sign = if value.is_negative() { "-" } else { "" };
        let len = sign.len() + digits.len();
        let fill = self.width.saturating_sub(len);
        let total = len.saturating_add(fill);
        if total > MAX_PADDED_LEN {
            return Err(Error::overflow("width too big").into());
        }
        let mut out = String::new();
        out.try_reserve(total)
            .map_err(|_| Error::overflow("width too big"))?;
        if self.left {
            out.push_str(sign);
            out.push_str(&digits);
            out.extend(std::iter::repeat_n(' ', fill));
        } else if self.zero {
            out.push_str(sign);
            out.extend(std::iter::repeat_n('0', fill));
            out.push_str(&digits);
        } else {
            out.extend(std::iter::repeat_n(' ', fill));
            out.push_str(sign);
            out.push_str(&digits);
        }
        Ok(out)
    }
}

/// `"%0<width>d" % value`.
fn pad<E: From<Error>>(width: usize, value: &Num, check: DigitCheck<'_, E>) -> Result<String, E> {
    IntSpec::zero_padded(width).format(&value.to_format_int()?, check)
}

// ------------------------------------------------------------------ source

/// What `strftime` formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Anything that isn't a `timedelta` or `Duration`: the date/time directives.
    DateTime,
    /// A `timedelta` that isn't a `Duration`.
    TimeDelta,
    /// A `Duration` (its `%P` also prints years and months).
    Duration,
}

/// A method of a date-like object that `strftime` calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateMethod {
    /// `obj.toordinal()`.
    ToOrdinal,
    /// `obj.isoweekday()`.
    IsoWeekday,
    /// `obj.isocalendar()[1]`.
    IsoWeek,
}

/// Lazy access to the object being formatted. Each call does exactly one Python
/// operation, so errors surface in upstream order.
pub trait StrfSource {
    /// Errors of the underlying object (and core errors, through `From`).
    type Error: From<Error>;

    /// `getattr(obj, name)` as a number.
    ///
    /// # Errors
    /// Whatever the attribute lookup raises.
    fn attr(&self, name: &'static str) -> Result<Num, Self::Error>;

    /// Calls `method` on the object.
    ///
    /// # Errors
    /// Whatever the lookup or call raises.
    fn call(&self, method: DateMethod) -> Result<Num, Self::Error>;

    /// `tz_isoformat(obj, format)`.
    ///
    /// # Errors
    /// Whatever `tz_isoformat` raises.
    fn tz_isoformat(&self, format: OffsetFormat) -> Result<PyStr, Self::Error>;

    /// Fails as `str(n)` would under the interpreter's current int→str digit limit.
    /// The default has no limit.
    ///
    /// # Errors
    /// The `ValueError` `str(n)` raises.
    fn check_int_str(&self, n: &BigInt) -> Result<(), Self::Error> {
        let _ = n;
        Ok(())
    }
}

/// The `strftime` code a directive maps to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Directive {
    Day,
    Micro,
    Hour,
    YearDay,
    Month,
    Minute,
    Second,
    Weekday,
    Week,
    Year,
    Century,
    Tz(OffsetFormat),
    Percent,
    Period,
    Weeks,
}

impl Directive {
    /// The directive `%<c>` names for `target`, if its regex alternation has it.
    fn lookup(target: Target, c: u32) -> Option<Self> {
        let c = char::from_u32(c)?;
        let common = match c {
            'd' => Some(Self::Day),
            'f' => Some(Self::Micro),
            'H' => Some(Self::Hour),
            'm' => Some(Self::Month),
            'M' => Some(Self::Minute),
            'S' => Some(Self::Second),
            'W' => Some(Self::Week),
            'Y' => Some(Self::Year),
            'C' => Some(Self::Century),
            '%' => Some(Self::Percent),
            _ => None,
        };
        if common.is_some() {
            return common;
        }
        match (target, c) {
            (Target::DateTime, 'j') => Some(Self::YearDay),
            (Target::DateTime, 'w') => Some(Self::Weekday),
            (Target::DateTime, 'z') => Some(Self::Tz(OffsetFormat::Basic)),
            (Target::DateTime, 'Z') => Some(Self::Tz(OffsetFormat::Extended)),
            (Target::DateTime, 'h') => Some(Self::Tz(OffsetFormat::Hours)),
            (Target::TimeDelta | Target::Duration, 'P') => Some(Self::Period),
            (Target::TimeDelta | Target::Duration, 'p') => Some(Self::Weeks),
            _ => None,
        }
    }
}

/// `isostrf.strftime(tdt, format, yeardigits)` for an object of kind `target`.
///
/// # Errors
/// The first failing directive's error.
pub fn strftime<S: StrfSource>(
    source: &S,
    target: Target,
    format: &[u32],
    yeardigits: &Num,
) -> Result<Vec<u32>, S::Error> {
    let digit_check = |n: &BigInt| source.check_int_str(n);
    let formatter = Formatter {
        source,
        target,
        yeardigits,
        digit_check: &digit_check,
    };
    let mut out = Vec::with_capacity(format.len());
    let mut i = 0;
    while i < format.len() {
        let directive = match (format.get(i), format.get(i + 1)) {
            (Some(0x25), Some(&c)) => Directive::lookup(target, c),
            _ => None,
        };
        match directive {
            Some(directive) => {
                let text = formatter.expand(directive)?;
                out.extend(text);
                i += 2;
            }
            None => {
                out.extend(format.get(i));
                i += 1;
            }
        }
    }
    Ok(out)
}

struct Formatter<'a, S: StrfSource> {
    source: &'a S,
    target: Target,
    yeardigits: &'a Num,
    digit_check: DigitCheck<'a, S::Error>,
}

fn cps(text: &str) -> Vec<u32> {
    text.chars().map(u32::from).collect()
}

impl<S: StrfSource> Formatter<'_, S> {
    fn expand(&self, directive: Directive) -> Result<Vec<u32>, S::Error> {
        if self.target == Target::DateTime {
            self.expand_date(directive)
        } else {
            self.expand_delta(directive)
        }
    }

    fn attr(&self, name: &'static str) -> Result<Num, S::Error> {
        self.source.attr(name)
    }

    fn check(&self) -> DigitCheck<'_, S::Error> {
        self.digit_check
    }

    /// `((yds != 4) and "+" or "") + ("%%0%dd" % (yds - shift)) % value()`.
    fn year(
        &self,
        shift: i64,
        value: impl FnOnce() -> Result<Num, S::Error>,
    ) -> Result<Vec<u32>, S::Error> {
        let plus = if self.yeardigits.ne_int(4) { "+" } else { "" };
        let digits = if shift == 0 {
            self.yeardigits.clone()
        } else {
            self.yeardigits.minus(shift)?
        };
        let spec = IntSpec::year(&digits.to_format_int()?, self.check())?;
        let value = value()?;
        let text = spec.format(&value.to_format_int()?, self.check())?;
        Ok(cps(&format!("{plus}{text}")))
    }

    fn expand_date(&self, directive: Directive) -> Result<Vec<u32>, S::Error> {
        let text = match directive {
            Directive::Day => pad(2, &self.attr("day")?, self.check())?,
            Directive::Micro => pad(6, &self.attr("microsecond")?, self.check())?,
            Directive::Hour => pad(2, &self.attr("hour")?, self.check())?,
            Directive::Month => pad(2, &self.attr("month")?, self.check())?,
            Directive::Minute => pad(2, &self.attr("minute")?, self.check())?,
            Directive::Second => pad(2, &self.attr("second")?, self.check())?,
            Directive::YearDay => {
                let ordinal = self.source.call(DateMethod::ToOrdinal)?;
                let year = self.attr("year")?;
                pad(3, &year_day(&ordinal, &year)?, self.check())?
            }
            Directive::Weekday => {
                let weekday = self.source.call(DateMethod::IsoWeekday)?;
                IntSpec {
                    left: false,
                    zero: false,
                    width: 1,
                }
                .format(&weekday.to_format_int()?, self.check())?
            }
            Directive::Week => pad(2, &self.source.call(DateMethod::IsoWeek)?, self.check())?,
            Directive::Year => return self.year(0, || self.attr("year")),
            Directive::Century => {
                return self.year(2, || Ok(self.attr("year")?.true_div(100)?));
            }
            Directive::Tz(format) => {
                return Ok(self.source.tz_isoformat(format)?.code_points().to_vec());
            }
            Directive::Percent => "%".to_owned(),
            Directive::Period | Directive::Weeks => return Ok(Vec::new()),
        };
        Ok(cps(&text))
    }

    fn expand_delta(&self, directive: Directive) -> Result<Vec<u32>, S::Error> {
        let text = match directive {
            Directive::Day => pad(2, &self.attr("days")?, self.check())?,
            Directive::Micro => pad(6, &self.attr("microseconds")?, self.check())?,
            Directive::Hour => pad(
                2,
                &self.attr("seconds")?.true_div(60)?.true_div(60)?,
                self.check(),
            )?,
            Directive::Month => pad(2, &self.attr("months")?, self.check())?,
            Directive::Minute => pad(
                2,
                &self.attr("seconds")?.true_div(60)?.modulo(60)?,
                self.check(),
            )?,
            Directive::Second => pad(2, &self.attr("seconds")?.modulo(60)?, self.check())?,
            Directive::Week => pad(2, &self.attr("days")?.true_div(7)?.abs()?, self.check())?,
            Directive::Year => return self.year(0, || self.attr("years")),
            Directive::Century => {
                return self.year(2, || Ok(self.attr("years")?.true_div(100)?));
            }
            Directive::Percent => "%".to_owned(),
            Directive::Period => self.period()?,
            Directive::Weeks => {
                let weeks = self.attr("days")?.floor_div(7)?.abs()?;
                format!("{}W", weeks.to_str(self.check())?)
            }
            Directive::YearDay | Directive::Weekday | Directive::Tz(_) => {
                return Ok(Vec::new());
            }
        };
        Ok(cps(&text))
    }

    /// `%P`: the ISO 8601 duration body.
    fn period(&self) -> Result<String, S::Error> {
        let mut ret = String::new();
        if self.target == Target::Duration {
            for (name, unit) in [("years", 'Y'), ("months", 'M')] {
                let value = self.attr(name)?;
                if value.is_true() {
                    ret.push_str(&value.abs()?.to_str(self.check())?);
                    ret.push(unit);
                }
            }
        }
        let days = int_field(self.attr("days")?)?;
        let seconds = int_field(self.attr("seconds")?)?;
        let micros = int_field(self.attr("microseconds")?)?;
        let total: BigInt =
            (days * BigInt::from(86_400) + seconds) * BigInt::from(1_000_000) + micros;
        let total = total.abs();
        let (seconds, usecs) = total.div_mod_floor(&BigInt::from(1_000_000));
        let (minutes, seconds) = seconds.div_mod_floor(&BigInt::from(60));
        let (hours, minutes) = minutes.div_mod_floor(&BigInt::from(60));
        let (days, hours) = hours.div_mod_floor(&BigInt::from(24));
        if !days.is_zero() {
            ret.push_str(&int_to_str(&days, self.check())?);
            ret.push('D');
        }
        if !(hours.is_zero() && minutes.is_zero() && seconds.is_zero() && usecs.is_zero()) {
            ret.push('T');
            if !hours.is_zero() {
                ret.push_str(&format!("{hours}H"));
            }
            if !minutes.is_zero() {
                ret.push_str(&format!("{minutes}M"));
            }
            if !(seconds.is_zero() && usecs.is_zero()) {
                if usecs.is_zero() {
                    ret.push_str(&seconds.to_string());
                } else {
                    let text = format!("{seconds}.{usecs:06}");
                    ret.push_str(text.trim_end_matches('0'));
                }
                ret.push('S');
            }
        }
        if ret.is_empty() {
            ret.push_str("0D");
        }
        Ok(ret)
    }
}

/// A `timedelta` field in `%P`'s integer arithmetic.
fn int_field(value: Num) -> Result<BigInt, Error> {
    match value {
        Num::Int(n) => Ok(n),
        Num::Float(_) => Err(Error::type_error(
            "%P needs integer timedelta fields, got float",
        )),
        Num::Decimal(_) => Err(Error::type_error(
            "%P needs integer timedelta fields, got decimal.Decimal",
        )),
        Num::Other(tp) => Err(Error::type_error(format!(
            "%P needs integer timedelta fields, got {tp}"
        ))),
    }
}

/// `tdt.toordinal() - date(tdt.year, 1, 1).toordinal() + 1`.
fn year_day(ordinal: &Num, year: &Num) -> Result<Num, Error> {
    let year = year.to_index()?;
    let year = match year.to_i32() {
        Some(y) => i64::from(y),
        None if year.is_negative() => {
            return Err(Error::overflow("signed integer is less than minimum"))
        }
        None => return Err(Error::overflow("signed integer is greater than maximum")),
    };
    let first = Date::new(year, 1, 1)?.to_ordinal();
    match ordinal {
        Num::Int(o) => Ok(Num::Int(o - first + 1)),
        Num::Float(f) => Ok(Num::Float(*f - (first as f64) + 1.0)),
        Num::Decimal(d) => Ok(Num::Decimal(
            d.sub(&Decimal::from_bigint(&BigInt::from(first - 1)))?,
        )),
        Num::Other(tp) => Err(Error::type_error(format!(
            "unsupported operand type(s) for -: '{tp}' and 'int'"
        ))),
    }
}

/// Whether a `Duration`'s `years`/`months` is negative (`value < 0`).
///
/// # Errors
/// `InvalidOperation` for a `Decimal` NaN; `TypeError` for non-numbers.
pub fn is_negative(value: &Num) -> Result<bool, Error> {
    match value {
        Num::Int(n) => Ok(n.is_negative()),
        Num::Float(f) => Ok(*f < 0.0),
        Num::Decimal(d) => Ok(d.compare(&Decimal::zero())? == Ordering::Less),
        Num::Other(tp) => Err(Error::type_error(format!(
            "'<' not supported between instances of '{tp}' and 'int'"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl Num {
        fn int(n: i64) -> Self {
            Self::Int(BigInt::from(n))
        }
    }

    struct Fields(Vec<(&'static str, Num)>);

    impl StrfSource for Fields {
        type Error = Error;

        fn attr(&self, name: &'static str) -> Result<Num, Error> {
            self.0
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| v.clone())
                .ok_or_else(|| Error::attribute(name))
        }

        fn call(&self, _method: DateMethod) -> Result<Num, Error> {
            Ok(Num::int(3))
        }

        fn tz_isoformat(&self, _format: OffsetFormat) -> Result<PyStr, Error> {
            Ok(PyStr::from_code_points(cps("Z")))
        }
    }

    fn run(fields: &Fields, target: Target, fmt: &str, yds: i64) -> Result<String, Error> {
        let out = strftime(fields, target, &cps(fmt), &Num::int(yds))?;
        Ok(out.iter().filter_map(|&c| char::from_u32(c)).collect())
    }

    #[test]
    fn century_width_near_isize_max_fails_before_allocating() {
        // `%C` uses `yeardigits - 2`, so 2**63 becomes a width just under `isize::MAX`.
        let f = Fields(vec![("year", Num::int(1985)), ("years", Num::int(3))]);
        let yds = Num::Int(BigInt::from(1u64 << 63));
        for target in [Target::DateTime, Target::Duration] {
            for fmt in ["%C", "%Y"] {
                assert!(strftime(&f, target, &cps(fmt), &yds).is_err());
            }
        }
        let wide =
            IntSpec::zero_padded(usize::MAX).format(&BigInt::from(5), &|_| Ok::<(), Error>(()));
        assert!(wide.is_err());
        let wide = IntSpec::zero_padded(MAX_PADDED_LEN + 1)
            .format(&BigInt::from(5), &|_| Ok::<(), Error>(()));
        assert!(wide.is_err());
    }

    #[test]
    fn true_div_of_i64_min() -> Result<(), Error> {
        let min = BigInt::from(i64::MIN);
        assert_eq!(int_true_div(&min, 100)?, -92_233_720_368_547_758.08);
        let f = Fields(vec![("year", Num::Int(min))]);
        assert!(run(&f, Target::DateTime, "%C", 4)?.starts_with("-922337203685477"));
        Ok(())
    }

    #[test]
    fn year_widths() -> Result<(), Error> {
        let f = Fields(vec![("year", Num::int(1985)), ("month", Num::int(4))]);
        assert_eq!(run(&f, Target::DateTime, "%Y-%m", 4)?, "1985-04");
        assert_eq!(run(&f, Target::DateTime, "%Y", 6)?, "+001985");
        assert_eq!(run(&f, Target::DateTime, "%Y|", -6)?, "+1985  |");
        assert_eq!(run(&f, Target::DateTime, "%C", 4)?, "19");
        assert_eq!(run(&f, Target::DateTime, "%C|", 0)?, "+19|");
        assert_eq!(run(&f, Target::DateTime, "%%Y %q %", 4)?, "%Y %q %");
        Ok(())
    }

    #[test]
    fn period() -> Result<(), Error> {
        let f = Fields(vec![
            (
                "years",
                Num::Decimal(Decimal::from_bigint(&BigInt::from(0))),
            ),
            (
                "months",
                Num::Decimal(Decimal::from_bigint(&BigInt::from(-2))),
            ),
            ("days", Num::int(-1)),
            ("seconds", Num::int(86_399)),
            ("microseconds", Num::int(500_000)),
        ]);
        assert_eq!(run(&f, Target::Duration, "P%P", 4)?, "P2MT0.5S");
        assert_eq!(run(&f, Target::Duration, "%p %H %M", 4)?, "1W 23 59");
        Ok(())
    }
}
