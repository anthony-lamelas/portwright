//! Converters from upstream `parse`: `int_convert`, `percentage`,
//! `date_convert` and the `FixedTzOffset` helpers they rely on.
//!
//! Every function reproduces upstream's evaluation order, so the first error
//! raised is the same one Python would raise.

use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{ToPrimitive, Zero};

use crate::error::{Error, RegexError, Result};
use crate::pyre::unicode15 as pystr;
use crate::types::{current_year, DateTime, FixedTzOffset, Temporal, Time, TimeDelta, TzInfo};

/// `int_convert.CHARS`.
pub const CHARS: &str = "0123456789abcdefghijklmnopqrstuvwxyz";

fn index_error() -> Error {
    Error::Index("string index out of range".into())
}

/// `CHARS[:base]` with Python slice semantics (negative counts from the end).
fn chars_prefix(base: i64) -> &'static str {
    let len = CHARS.len();
    let end = if base < 0 {
        usize::try_from(base.unsigned_abs()).map_or(0, |n| len.saturating_sub(n))
    } else {
        usize::try_from(base).map_or(len, |n| n.min(len))
    };
    // CHARS is ASCII, so byte and code-point indexes agree.
    CHARS.get(..end).unwrap_or("")
}

/// `int_convert(base)(string, match)`.
///
/// `base` is `None` for automatic detection. Out-of-`i64` bases may be
/// saturated by the caller: the result is the same.
pub fn int_convert(string: &str, base: Option<i64>) -> Result<BigInt> {
    // Only the first three code points (and, for a leading `0`, the count)
    // are ever looked at.
    let head: [Option<char>; 3] = {
        let mut it = string.chars();
        [it.next(), it.next(), it.next()]
    };
    let at = |i: usize| head.get(i).copied().flatten();
    let (negative, number_start) = match at(0) {
        None => return Err(index_error()),
        Some('-') => (true, 1),
        Some('+') => (false, 1),
        Some(_) => (false, 0),
    };
    let base = match base {
        Some(b) => b,
        None => {
            let first = at(number_start).ok_or_else(index_error)?;
            let mut b = 10;
            if first == '0' && string.chars().count() - number_start > 2 {
                match at(number_start + 1) {
                    Some('b' | 'B') => b = 2,
                    Some('o' | 'O') => b = 8,
                    Some('x' | 'X') => b = 16,
                    _ => {}
                }
            }
            b
        }
    };
    let allowed = chars_prefix(base);
    if allowed.is_empty() {
        // upstream compiles "[^]" here
        return Err(Error::Regex(RegexError::new(
            "unterminated character set",
            "[^]",
            Some(0),
        )));
    }
    let kept: String = if string.is_ascii() {
        // `lower` of ASCII text is `to_ascii_lowercase`, char by char.
        string
            .bytes()
            .map(|b| char::from(b.to_ascii_lowercase()))
            .filter(|c| allowed.contains(*c))
            .collect()
    } else {
        pystr::lower(string)
            .chars()
            .filter(|c| allowed.contains(*c))
            .collect()
    };
    let base = u32::try_from(base)
        .ok()
        .filter(|b| *b >= 2)
        .ok_or_else(|| Error::Value("int() base must be >= 2 and <= 36, or 0".into()))?;
    let value = pystr::py_int(&kept, base)?;
    Ok(if negative { -value } else { value })
}

/// `percentage(string, match)`: `float(string[:-1]) / 100.0`.
pub fn percentage(string: &str) -> Result<f64> {
    let mut chars = string.chars();
    chars.next_back();
    Ok(pystr::py_float(chars.as_str())? / 100.0)
}

/// A tuple index as given by the caller. `Err` holds the error Python raises
/// when the object is used as an index (e.g. a `str` index).
pub type Index = Result<i64>;

/// The keyword arguments of upstream `date_convert`.
#[derive(Debug, Clone, Default)]
pub struct DateArgs {
    /// `(mm, dd)`, set only when `mm and dd` is truthy.
    pub mm_dd: Option<(Index, Index)>,
    /// `ymd` when not `None`.
    pub ymd: Option<Index>,
    /// `mdy` when not `None`.
    pub mdy: Option<Index>,
    /// `dmy` when not `None`.
    pub dmy: Option<Index>,
    /// `d_m_y` when not `None`: the unpacked items, or the unpacking error.
    pub d_m_y: Option<Result<Vec<Index>>>,
    /// `hms` when not `None`.
    pub hms: Option<Index>,
    /// `am` when not `None`.
    pub am: Option<Index>,
    /// `tz` when not `None`.
    pub tz: Option<Index>,
}

fn get<'a, S: AsRef<str>>(groups: &'a [Option<S>], idx: &Index) -> Result<Option<&'a str>> {
    let i = idx.clone()?;
    let len = i64::try_from(groups.len()).unwrap_or(i64::MAX);
    let j = if i < 0 { i + len } else { i };
    let group = usize::try_from(j)
        .ok()
        .and_then(|j| groups.get(j))
        .ok_or_else(|| Error::Index("tuple index out of range".into()))?;
    Ok(group.as_ref().map(AsRef::as_ref))
}

fn unpack<T>(items: Vec<T>, n: usize) -> Result<Vec<T>> {
    match items.len() {
        k if k < n => Err(Error::Value(format!(
            "not enough values to unpack (expected {n}, got {k})"
        ))),
        k if k > n => Err(Error::Value(format!(
            "too many values to unpack (expected {n})"
        ))),
        _ => Ok(items),
    }
}

fn none_type_error(what: &str) -> Error {
    Error::Type(format!(
        "expected string or bytes-like object, got '{what}'"
    ))
}

/// `re.split(r"[-/\s]", s)` unpacked into three strings.
fn split3(s: Option<&str>) -> Result<[&str; 3]> {
    let s = s.ok_or_else(|| none_type_error("NoneType"))?;
    let parts: Vec<&str> = s
        .split(|c: char| c == '-' || c == '/' || pystr::is_space(c))
        .collect();
    let mut it = unpack(parts, 3)?.into_iter();
    Ok([
        it.next().unwrap_or_default(),
        it.next().unwrap_or_default(),
        it.next().unwrap_or_default(),
    ])
}

/// `int(x)` where `x` is a `str` or `None`.
fn int_of(s: Option<&str>) -> Result<BigInt> {
    match s {
        Some(s) => pystr::py_int(s, 10),
        None => Err(Error::Type(
            "int() argument must be a string, a bytes-like object or a real number, not 'NoneType'"
                .into(),
        )),
    }
}

/// `str.strip()` with no argument.
fn strip(s: &str) -> &str {
    s.trim_matches(pystr::is_space)
}

/// The year of the date being built.
enum Year<'a> {
    Int(BigInt),
    Str(Option<&'a str>),
}

/// The `tz` value just before `datetime()` / `time()` is called.
enum TzArg {
    None,
    /// A string that upstream leaves unconverted (empty or all upper case).
    Str,
    Fixed(FixedTzOffset),
}

/// `timedelta(minutes=offset)` for an int offset, with CPython's C-int check.
fn minutes_delta(offset: &BigInt) -> Result<TimeDelta> {
    let days = offset.div_floor(&BigInt::from(1440));
    if days.to_i32().is_none() {
        return Err(Error::Overflow(
            "Python int too large to convert to C int".into(),
        ));
    }
    TimeDelta::from_minutes_int(offset)
}

fn parse_tz(raw: Option<&str>) -> Result<TzArg> {
    let Some(raw) = raw else {
        return Ok(TzArg::None);
    };
    if raw.is_empty() {
        return Ok(TzArg::Str);
    }
    let stripped = strip(raw);
    // `upper` of ASCII is `to_ascii_uppercase`; only other text needs the
    // full mapping.
    let is_z = if stripped.is_ascii() {
        stripped.eq_ignore_ascii_case("Z")
    } else {
        pystr::upper(stripped) == "Z"
    };
    if is_z {
        return Ok(TzArg::Fixed(FixedTzOffset {
            offset: TimeDelta::ZERO,
            name: "UTC".into(),
        }));
    }
    let tz = stripped;
    if pystr::is_upper(tz) {
        return Ok(TzArg::Str);
    }
    if let Some(r) = parse_tz_ascii(tz) {
        return r;
    }
    let chars: Vec<char> = tz.chars().collect();
    let sign = *chars.first().ok_or_else(index_error)?;
    let slice = |a: usize, b: usize| -> String {
        let b = b.min(chars.len());
        chars
            .get(a.min(b)..b)
            .map(|s| s.iter().collect())
            .unwrap_or_default()
    };
    let (tzh, tzm) = if tz.contains(':') {
        let rest = slice(1, chars.len());
        let parts: Vec<&str> = rest.split(':').collect();
        let mut it = unpack(parts, 2)?.into_iter();
        (
            it.next().unwrap_or_default().to_owned(),
            it.next().unwrap_or_default().to_owned(),
        )
    } else if chars.len() == 4 {
        (slice(1, 2), slice(2, 4))
    } else {
        (slice(1, 3), slice(3, 5))
    };
    // `int(tzm)` is pure, so its first evaluation stands in for the second.
    let minutes = pystr::py_int(&tzm, 10)?;
    if minutes >= BigInt::from(60) {
        return Err(Error::Value("timezone minute must be in 0..59".into()));
    }
    let mut offset: BigInt = minutes + pystr::py_int(&tzh, 10)? * 60;
    if sign == '-' {
        offset = -offset;
    }
    Ok(TzArg::Fixed(FixedTzOffset {
        offset: minutes_delta(&offset)?,
        name: tz.to_owned(),
    }))
}

/// `s` as a small number when it is only ASCII digits, the case where
/// `int(s)` is plain decimal reading.
fn ascii_small_int(s: &str) -> Option<i64> {
    if s.is_empty() || s.len() > 9 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(s.bytes().fold(0, |acc, b| acc * 10 + i64::from(b - b'0')))
}

/// The common case of the offset part of [`parse_tz`]: ASCII text whose
/// hour and minute parts are plain digits. `None` means "take the general
/// path", which then gives the same result (or error) as before.
fn parse_tz_ascii(tz: &str) -> Option<Result<TzArg>> {
    if !tz.is_ascii() {
        return None;
    }
    // ASCII: byte and code-point slices agree.
    let len = tz.len();
    let slice = |a: usize, b: usize| {
        let b = b.min(len);
        tz.get(a.min(b)..b)
    };
    let sign = *tz.as_bytes().first()?;
    let (tzh, tzm) = if tz.contains(':') {
        let (h, m) = slice(1, len)?.split_once(':')?;
        if m.contains(':') {
            return None;
        }
        (h, m)
    } else if len == 4 {
        (slice(1, 2)?, slice(2, 4)?)
    } else {
        (slice(1, 3)?, slice(3, 5)?)
    };
    // Same order as the general path: minutes, their range, then hours.
    let minutes = ascii_small_int(tzm)?;
    if minutes >= 60 {
        return Some(Err(Error::Value("timezone minute must be in 0..59".into())));
    }
    let mut offset = minutes + ascii_small_int(tzh)? * 60;
    if sign == b'-' {
        offset = -offset;
    }
    Some(minutes_delta(&BigInt::from(offset)).map(|offset| {
        TzArg::Fixed(FixedTzOffset {
            offset,
            name: tz.to_owned(),
        })
    }))
}

/// Argument parsing of `datetime()`/`time()`: a Python int into a C `int`.
fn c_int(v: &BigInt) -> Result<i64> {
    let Some(v) = v.to_i64() else {
        return Err(Error::Overflow(
            "Python int too large to convert to C long".into(),
        ));
    };
    if i32::try_from(v).is_err() {
        return Err(Error::Overflow(if v > 0 {
            "signed integer is greater than maximum".into()
        } else {
            "signed integer is less than minimum".into()
        }));
    }
    Ok(v)
}

fn tz_type_error() -> Error {
    Error::Type("tzinfo argument must be None or of a tzinfo subclass, not type 'str'".into())
}

/// `re.split(r"[-/\s]", s)` into exactly three parts, without allocating.
fn split3_fast(s: &str) -> Option<[&str; 3]> {
    let mut it = s.split(|c: char| c == '-' || c == '/' || pystr::is_space(c));
    let parts = [it.next()?, it.next()?, it.next()?];
    it.next().is_none().then_some(parts)
}

/// The fractional-second digits as `int(frac.ljust(6, "0")[:6])`, for ASCII digits only.
fn micro_fast(frac: &str) -> Option<i64> {
    if !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut v = 0;
    let mut bytes = frac.bytes();
    for _ in 0..6 {
        v = v * 10 + bytes.next().map_or(0, |b| i64::from(b - b'0'));
    }
    Some(v)
}

/// The usual `tz` text: ASCII, not all upper case, with plain digit parts.
fn tz_fast(raw: Option<&str>) -> Option<Option<TzInfo>> {
    let Some(raw) = raw else {
        return Some(None);
    };
    let tz = strip(raw);
    if raw.is_empty() || !tz.is_ascii() {
        return None;
    }
    if tz.eq_ignore_ascii_case("Z") {
        return Some(Some(TzInfo::Fixed(FixedTzOffset {
            offset: TimeDelta::ZERO,
            name: "UTC".into(),
        })));
    }
    if pystr::is_upper(tz) {
        return None;
    }
    match parse_tz_ascii(tz)? {
        Ok(TzArg::Fixed(f)) => Some(Some(TzInfo::Fixed(f))),
        _ => None,
    }
}

/// The year (when already known as a number), year text, month and day texts.
type FastYmd<'a> = (
    Option<i64>,
    Option<&'a str>,
    Option<&'a str>,
    Option<&'a str>,
);

/// [`date_convert`] for the common inputs: ASCII text and plain digit
/// numbers of at most nine digits, so every `int()` is exact in an `i64`.
/// `None` means "take the general path", which also reports every error,
/// so this only ever returns a value the general path would return.
fn date_convert_fast<S: AsRef<str>>(groups: &[Option<S>], args: &DateArgs) -> Option<Temporal> {
    let int = |s: Option<&str>| ascii_small_int(s?);
    // (y, m, d) texts, the year already read when it comes from `current_year`.
    let ymd: Option<FastYmd<'_>> = if let Some((mm, dd)) = &args.mm_dd {
        let m = get(groups, mm).ok()?;
        let d = get(groups, dd).ok()?;
        Some((Some(current_year()), None, m, d))
    } else if let Some(i) = &args.ymd {
        let [y, m, d] = split3_fast(get(groups, i).ok()??)?;
        Some((None, Some(y), Some(m), Some(d)))
    } else if let Some(i) = &args.mdy {
        let [m, d, y] = split3_fast(get(groups, i).ok()??)?;
        Some((None, Some(y), Some(m), Some(d)))
    } else if let Some(i) = &args.dmy {
        let [d, m, y] = split3_fast(get(groups, i).ok()??)?;
        Some((None, Some(y), Some(m), Some(d)))
    } else if let Some(items) = &args.d_m_y {
        let [di, mi, yi] = items.as_ref().ok()?.as_slice() else {
            return None;
        };
        let d = get(groups, di).ok()?;
        let m = get(groups, mi).ok()?;
        let y = get(groups, yi).ok()?;
        Some((None, y, m, d))
    } else {
        None
    };

    let (mut hour, mut minute, mut second, mut micro) = (0, 0, 0, 0);
    if let Some(i) = &args.hms {
        if let Some(text) = get(groups, i).ok()?.filter(|s| !s.is_empty()) {
            let mut t = text.split(':');
            let (h, m) = (t.next()?, t.next()?);
            if let Some(s) = t.next() {
                if t.next().is_some() {
                    return None;
                }
                let s = match s.split_once('.') {
                    Some((s, frac)) => {
                        if frac.contains('.') {
                            return None;
                        }
                        micro = micro_fast(frac)?;
                        s
                    }
                    None => s,
                };
                second = ascii_small_int(s)?;
            }
            hour = ascii_small_int(h)?;
            minute = ascii_small_int(m)?;
        }
    }

    if let Some(i) = &args.am {
        if let Some(s) = get(groups, i).ok()?.filter(|s| !s.is_empty()) {
            let am = strip(s);
            if !am.is_ascii() {
                return None;
            }
            if am.eq_ignore_ascii_case("AM") && hour == 12 {
                hour -= 12;
            } else if am.eq_ignore_ascii_case("PM") && hour != 12 {
                hour += 12;
            }
        }
    }

    let tz = match &args.tz {
        Some(i) => tz_fast(get(groups, i).ok()?)?,
        None => None,
    };

    let Some((year, y, m, d)) = ymd else {
        return Time::new(hour, minute, second, micro, tz)
            .ok()
            .map(Temporal::Time);
    };
    let year = match year {
        Some(v) => v,
        None => int(y)?,
    };
    let m = m?;
    if !m.is_ascii() {
        return None;
    }
    let month = match ascii_small_int(m) {
        Some(v) => v,
        // ASCII `capitalize(m) == key`, as every key is capitalized.
        None if m.is_empty() || !m.bytes().all(|b| b.is_ascii_digit()) => i64::from(
            MONTHS_MAP
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(m))?
                .1,
        ),
        None => return None,
    };
    let day = int(d)?;
    DateTime::new(year, month, day, hour, minute, second, micro, tz)
        .ok()
        .map(Temporal::DateTime)
}

/// `date_convert(string, match, **args)` where `groups` is `match.groups()`.
pub fn date_convert<S: AsRef<str>>(groups: &[Option<S>], args: &DateArgs) -> Result<Temporal> {
    match date_convert_fast(groups, args) {
        Some(v) => Ok(v),
        None => date_convert_general(groups, args),
    }
}

/// [`date_convert`] for any input.
fn date_convert_general<S: AsRef<str>>(groups: &[Option<S>], args: &DateArgs) -> Result<Temporal> {
    // (y, m, d), or None for time only
    let ymd: Option<(Year<'_>, Option<&str>, Option<&str>)> = if let Some((mm, dd)) = &args.mm_dd {
        let y = Year::Int(BigInt::from(current_year()));
        let m = get(groups, mm)?;
        let d = get(groups, dd)?;
        Some((y, m, d))
    } else if let Some(i) = &args.ymd {
        let [y, m, d] = split3(get(groups, i)?)?;
        Some((Year::Str(Some(y)), Some(m), Some(d)))
    } else if let Some(i) = &args.mdy {
        let [m, d, y] = split3(get(groups, i)?)?;
        Some((Year::Str(Some(y)), Some(m), Some(d)))
    } else if let Some(i) = &args.dmy {
        let [d, m, y] = split3(get(groups, i)?)?;
        Some((Year::Str(Some(y)), Some(m), Some(d)))
    } else if let Some(items) = &args.d_m_y {
        let mut it = unpack(items.clone()?, 3)?.into_iter();
        let (di, mi, yi) = (
            it.next().unwrap_or(Ok(0)),
            it.next().unwrap_or(Ok(0)),
            it.next().unwrap_or(Ok(0)),
        );
        let d = get(groups, &di)?;
        let m = get(groups, &mi)?;
        let y = get(groups, &yi)?;
        Some((Year::Str(y), m, d))
    } else {
        None
    };

    let zero = BigInt::zero;
    let (mut hour, mut minute, mut second, mut micro) = (zero(), zero(), zero(), zero());
    if let Some(i) = &args.hms {
        if let Some(text) = get(groups, i)?.filter(|s| !s.is_empty()) {
            let t: Vec<&str> = text.split(':').collect();
            let (h, m) = if t.len() == 2 {
                (t[0], t[1])
            } else {
                let t = unpack(t, 3)?;
                let mut s = t[2];
                if s.contains('.') {
                    let parts = unpack(s.split('.').collect(), 2)?;
                    s = parts[0];
                    let mut frac: String = parts[1].chars().collect();
                    let len = frac.chars().count();
                    if len < 6 {
                        frac.extend(std::iter::repeat_n('0', 6 - len));
                    }
                    let frac: String = frac.chars().take(6).collect();
                    micro = pystr::py_int(&frac, 10)?;
                }
                second = pystr::py_int(s, 10)?;
                (t[0], t[1])
            };
            hour = pystr::py_int(h, 10)?;
            minute = pystr::py_int(m, 10)?;
        }
    }

    if let Some(i) = &args.am {
        let am = get(groups, i)?
            .filter(|s| !s.is_empty())
            .map(|s| pystr::upper(strip(s)));
        let twelve = BigInt::from(12);
        match am.as_deref() {
            Some("AM") if hour == twelve => hour -= 12,
            Some("PM") if hour == twelve => {}
            Some("PM") => hour += 12,
            _ => {}
        }
    }

    let tz = match &args.tz {
        Some(i) => parse_tz(get(groups, i)?)?,
        None => TzArg::None,
    };

    let Some((y, m, d)) = ymd else {
        let (h, mi, s, us) = (
            c_int(&hour)?,
            c_int(&minute)?,
            c_int(&second)?,
            c_int(&micro)?,
        );
        let mut time = Time::new(h, mi, s, us, None)?;
        time.tz = match tz {
            TzArg::None => None,
            TzArg::Str => return Err(tz_type_error()),
            TzArg::Fixed(f) => Some(TzInfo::Fixed(f)),
        };
        return Ok(Temporal::Time(time));
    };

    let year = match y {
        Year::Int(v) => v,
        Year::Str(s) => int_of(s)?,
    };
    let m =
        m.ok_or_else(|| Error::Attribute("'NoneType' object has no attribute 'isdigit'".into()))?;
    let month = if !m.is_empty() && m.chars().all(pystr::is_digit) {
        pystr::py_int(m, 10)?
    } else {
        let key = pystr::capitalize(m);
        BigInt::from(month_number(&key).ok_or(Error::Key(key))?)
    };
    let day = int_of(d)?;
    let vals = [&year, &month, &day, &hour, &minute, &second, &micro];
    let mut c = [0i64; 7];
    for (slot, v) in c.iter_mut().zip(vals) {
        *slot = c_int(v)?;
    }
    let mut dt = DateTime::new(c[0], c[1], c[2], c[3], c[4], c[5], c[6], None)?;
    dt.time.tz = match tz {
        TzArg::None => None,
        TzArg::Str => return Err(tz_type_error()),
        TzArg::Fixed(f) => Some(TzInfo::Fixed(f)),
    };
    Ok(Temporal::DateTime(dt))
}

/// Upstream `MONTHS_MAP` (keys are case-sensitive).
pub const MONTHS_MAP: [(&str, u32); 23] = [
    ("Jan", 1),
    ("January", 1),
    ("Feb", 2),
    ("February", 2),
    ("Mar", 3),
    ("March", 3),
    ("Apr", 4),
    ("April", 4),
    ("May", 5),
    ("Jun", 6),
    ("June", 6),
    ("Jul", 7),
    ("July", 7),
    ("Aug", 8),
    ("August", 8),
    ("Sep", 9),
    ("September", 9),
    ("Oct", 10),
    ("October", 10),
    ("Nov", 11),
    ("November", 11),
    ("Dec", 12),
    ("December", 12),
];

/// `MONTHS_MAP[name]`.
pub fn month_number(name: &str) -> Option<u32> {
    MONTHS_MAP.iter().find(|(k, _)| *k == name).map(|(_, v)| *v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(v: &[Option<&str>]) -> Vec<Option<String>> {
        v.iter().map(|s| s.map(str::to_owned)).collect()
    }

    #[test]
    fn int_convert_basics() {
        assert_eq!(int_convert("-0x1f", None).unwrap(), BigInt::from(-31));
        assert_eq!(int_convert("1,234", Some(10)).unwrap(), BigInt::from(1234));
        assert_eq!(int_convert("", Some(10)).unwrap_err(), index_error());
        assert_eq!(int_convert("-", None).unwrap_err(), index_error());
        assert!(matches!(int_convert("1", Some(0)), Err(Error::Regex(_))));
        assert!(matches!(int_convert("1", Some(1)), Err(Error::Value(_))));
        assert_eq!(chars_prefix(-35), "0");
        assert_eq!(chars_prefix(100), CHARS);
    }

    #[test]
    fn date_convert_ymd() {
        let args = DateArgs {
            ymd: Some(Ok(1)),
            hms: Some(Ok(2)),
            tz: Some(Ok(3)),
            ..DateArgs::default()
        };
        let r = date_convert(
            &g(&[
                Some("x"),
                Some("2020-01-02"),
                Some("10:11:12.5"),
                Some("+01:30"),
            ]),
            &args,
        )
        .unwrap();
        let Temporal::DateTime(dt) = r else {
            panic!("not a datetime")
        };
        assert_eq!(dt.time.microsecond, 500_000);
        assert!(matches!(dt.time.tz, Some(TzInfo::Fixed(_))));
    }

    /// Wherever the fast path answers, it equals the general path.
    #[test]
    fn date_fast_matches_general() {
        let dates = [
            "2020-01-02",
            "21/Nov/2011",
            "21/nOV/2011",
            "1/2/3",
            "2020-13-01",
            "2020-02-30",
            "0-1-1",
            "2020-1",
            "2020-01-02-03",
            "x-y-z",
            "2020-Sept-01",
            "2020-May-01",
            "2020 01 02",
            "2020\x1c01\x1c02",
            "١٢-01-01",
            "2020-١-01",
            "+2020-01-02",
            "2020--02",
            "",
            "9999999999-1-1",
            "2020-0x1-01",
            "2020-01-0_2",
        ];
        let times = [
            "10:11:12.5",
            "10:11",
            "10:11:12",
            "23:59:59.9999999",
            "24:00",
            "1:2:3.",
            "1:2:3.x",
            "1:2:3.1.2",
            "1:2",
            "1",
            "1:2:3:4",
            "12:00:00",
            "00:00",
            "10:60",
            " 10:11",
            "10:11:12.1234567x",
            "",
            "1:2:3.١",
        ];
        let tzs = [
            "+01:30",
            "-0130",
            "+1000",
            "Z",
            "z",
            " Z ",
            "",
            "UTC",
            "+01:60",
            "+0",
            "+01:3:0",
            "-1",
            "+999999999:00",
            "x12:34",
            "+01:٣٠",
            "+abcd",
        ];
        let ams = ["AM", "pm", " PM ", "", "XM", "ａｍ"];
        let mut checked = 0;
        for d in dates {
            for t in times {
                for z in tzs {
                    for am in ams {
                        let groups = g(&[Some(d), Some(t), Some(z), Some(am), None]);
                        for args in [
                            DateArgs {
                                ymd: Some(Ok(0)),
                                hms: Some(Ok(1)),
                                tz: Some(Ok(2)),
                                am: Some(Ok(3)),
                                ..DateArgs::default()
                            },
                            DateArgs {
                                dmy: Some(Ok(0)),
                                hms: Some(Ok(1)),
                                tz: Some(Ok(2)),
                                ..DateArgs::default()
                            },
                            DateArgs {
                                mdy: Some(Ok(0)),
                                tz: Some(Ok(-3)),
                                ..DateArgs::default()
                            },
                            DateArgs {
                                hms: Some(Ok(1)),
                                am: Some(Ok(3)),
                                tz: Some(Ok(2)),
                                ..DateArgs::default()
                            },
                            DateArgs {
                                hms: Some(Ok(1)),
                                tz: Some(Ok(4)),
                                ..DateArgs::default()
                            },
                            DateArgs {
                                mm_dd: Some((Ok(1), Ok(0))),
                                ..DateArgs::default()
                            },
                            DateArgs {
                                d_m_y: Some(Ok(vec![Ok(0), Ok(1), Ok(2)])),
                                ..DateArgs::default()
                            },
                            DateArgs {
                                d_m_y: Some(Ok(vec![Ok(0), Ok(4), Ok(2)])),
                                ..DateArgs::default()
                            },
                            DateArgs {
                                ymd: Some(Ok(9)),
                                ..DateArgs::default()
                            },
                        ] {
                            if let Some(fast) = date_convert_fast(&groups, &args) {
                                assert_eq!(
                                    Ok(fast),
                                    date_convert_general(&groups, &args),
                                    "{groups:?} {args:?}"
                                );
                                checked += 1;
                            }
                        }
                    }
                }
            }
        }
        assert!(checked > 1000, "{checked}");
    }

    #[test]
    fn misc() {
        assert_eq!(percentage("50%").unwrap(), 0.5);
    }
}
