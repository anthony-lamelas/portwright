//! Task `strftime`: `datetime.strftime(fmt)` as CPython 3.12.14 runs it on glibc with
//! TZ=UTC and the C locale.
//!
//! Three layers, each ported separately:
//! 1. `_datetime`'s `wrap_strftime` replaces `%z`, `%:z`, `%Z` and `%f` itself.
//! 2. `time.strftime` splits the result into runs of ASCII non-NUL text, which go to the
//!    C library, and other text, which it copies. It grows one output buffer from 1024
//!    bytes, doubling while a run's result is empty and the buffer is below 256 times the
//!    run's length; a run whose result doesn't fit gives `''`.
//! 3. glibc's `strftime` in the C locale, with its flags, widths and `E`/`O` modifiers.

use crate::args::Param;
use crate::date::{days_before_month, push_padded};
use crate::datetime::{dt_in, split_micros, DateTime};
use crate::error::{PyError, PyResult};
use crate::pystr::PyStr;
use crate::timedelta::FixedOffset;
use crate::{Kwargs, Value};

const PARAMS: [Param; 2] = [Param::required("d"), Param::required("fmt")];

/// `time.strftime`'s first buffer size, in bytes.
const INITIAL_BUFFER: usize = 1024;

/// `time.strftime` stops growing its buffer once it reaches this many bytes per format byte.
const BUFFER_PER_FORMAT_BYTE: usize = 256;

/// Ordinal of 1970-01-01.
const EPOCH_ORDINAL: i64 = 719_163;

/// `pc_datetime.strftime`: `datetime(...).strftime(fmt)`.
///
/// # Errors
/// Input errors from building the datetime, then TypeError for a non-str `fmt`.
pub fn strftime(args: &[Value], kwargs: &Kwargs) -> PyResult<Value> {
    let v = crate::timedelta::bind_ref("strftime", &PARAMS, args, kwargs)?;
    let d = dt_in(&v[0])?;
    let Value::Str(fmt) = &*v[1] else {
        let name = match &*v[1] {
            Value::None => "None",
            other => other.type_name(),
        };
        return Err(PyError::type_error(format!(
            "strftime() argument 1 must be str, not {name}"
        )));
    };
    let wrapped = wrap_strftime(d, fmt.code_points());
    let tm = Tm::new(d);
    Ok(Value::Str(PyStr::from_code_points(time_strftime(
        &wrapped, &tm,
    ))))
}

// ------------------------------------------------------------------ wrap_strftime

/// `_datetime`'s `format_utcoffset` with separator `sep` (empty or `:`), appended to `out`.
fn push_utcoffset(out: &mut Vec<u32>, tz: FixedOffset, sep: Option<char>) {
    let offset = tz.utcoffset();
    let total = offset.total_microseconds();
    let (sign, total) = if offset.days() < 0 {
        ('-', -total)
    } else {
        ('+', total)
    };
    // |offset| < 1 day, so every part fits an i64.
    let (microseconds, seconds) = split_micros(total);
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    let mut text = String::with_capacity(16);
    text.push(sign);
    push_padded(&mut text, hours, 2);
    text.extend(sep);
    push_padded(&mut text, minutes, 2);
    if microseconds != 0 || seconds != 0 {
        text.extend(sep);
        push_padded(&mut text, seconds, 2);
    }
    if microseconds != 0 {
        text.push('.');
        push_padded(&mut text, microseconds, 6);
    }
    out.extend(text.bytes().map(u32::from));
}

/// `wrap_strftime`: replaces `%z`, `%:z`, `%Z` and `%f`; copies everything else,
/// consuming `%` pairs so `%%z` stays literal.
fn wrap_strftime(d: DateTime, fmt: &[u32]) -> Vec<u32> {
    let tz = d.time().tz();
    let mut out: Vec<u32> = Vec::with_capacity(fmt.len());
    let push_str = |out: &mut Vec<u32>, s: &str| out.extend(s.chars().map(u32::from));
    let mut i = 0;
    while let Some(&c) = fmt.get(i) {
        i += 1;
        if c != u32::from('%') {
            out.push(c);
            continue;
        }
        let Some(&next) = fmt.get(i) else {
            out.push(c);
            break;
        };
        i += 1;
        match char::from_u32(next) {
            Some('z') => {
                if let Some(tz) = tz {
                    push_utcoffset(&mut out, tz, None);
                }
            }
            Some(':') if fmt.get(i) == Some(&u32::from('z')) => {
                i += 1;
                if let Some(tz) = tz {
                    push_utcoffset(&mut out, tz, Some(':'));
                }
            }
            Some('Z') => {
                if let Some(tz) = tz {
                    push_str(&mut out, &tz.tzname().replace('%', "%%"));
                }
            }
            Some('f') => {
                let mut text = String::with_capacity(6);
                push_padded(&mut text, d.time().microsecond(), 6);
                push_str(&mut out, &text);
            }
            _ => {
                out.push(c);
                out.push(next);
            }
        }
    }
    out
}

// ------------------------------------------------------------------ time.strftime

/// Whether `time.strftime` hands this code point to the C library.
fn is_c_text(c: u32) -> bool {
    c != 0 && c < 128
}

/// `time.strftime(fmt, timetuple)`: C runs are formatted, other text is copied.
fn time_strftime(fmt: &[u32], tm: &Tm) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::with_capacity(fmt.len());
    let mut bufsize = INITIAL_BUFFER;
    let mut i = 0;
    while i < fmt.len() {
        let start = i;
        while fmt.get(i).is_some_and(|&c| is_c_text(c)) {
            i += 1;
        }
        if let Some(run) = fmt.get(start..i).filter(|run| !run.is_empty()) {
            // Every code point in the run is ASCII, so the conversion is lossless.
            let bytes: Vec<u8> = run.iter().filter_map(|&c| u8::try_from(c).ok()).collect();
            let formatted = format_run(&bytes, tm, &mut bufsize);
            out.extend(formatted.iter().map(|&b| u32::from(b)));
        }
        let start = i;
        while fmt.get(i).is_some_and(|&c| c != u32::from('%')) {
            i += 1;
        }
        out.extend_from_slice(fmt.get(start..i).unwrap_or_default());
    }
    out
}

/// One C call with `time.strftime`'s buffer growth; updates the shared buffer size.
fn format_run(run: &[u8], tm: &Tm, bufsize: &mut usize) -> Vec<u8> {
    let cap = BUFFER_PER_FORMAT_BYTE.saturating_mul(run.len());
    let mut largest = *bufsize;
    while largest < cap {
        largest = largest.saturating_mul(2);
    }
    let formatted = c_strftime(run, tm, largest).filter(|s| !s.is_empty());
    match formatted {
        Some(s) => {
            // The smallest tried buffer that holds the result and its NUL.
            while *bufsize <= s.len() {
                *bufsize = bufsize.saturating_mul(2);
            }
            s
        }
        None => {
            *bufsize = largest;
            Vec::new()
        }
    }
}

// ------------------------------------------------------------------ glibc strftime

/// The `struct tm` that `timetuple()` produces (`tm_isdst` is always -1 here).
#[derive(Debug, Clone, Copy)]
struct Tm {
    year: i64,
    /// 1..=12.
    month: i64,
    mday: i64,
    hour: i64,
    minute: i64,
    second: i64,
    /// 0 is Sunday.
    wday: i64,
    /// 0-based day of the year.
    yday: i64,
    /// `mktime` with TZ=UTC.
    epoch: i64,
}

impl Tm {
    fn new(d: DateTime) -> Self {
        let date = d.date();
        let time = d.time();
        let (year, month, mday) = (
            i64::from(date.year()),
            i64::from(date.month()),
            i64::from(date.day()),
        );
        let (hour, minute, second) = (
            i64::from(time.hour()),
            i64::from(time.minute()),
            i64::from(time.second()),
        );
        Self {
            year,
            month,
            mday,
            hour,
            minute,
            second,
            wday: (date.weekday() + 1) % 7,
            yday: days_before_month(year, month) + mday - 1,
            epoch: (date.to_ordinal() - EPOCH_ORDINAL) * 86_400
                + hour * 3600
                + minute * 60
                + second,
        }
    }

    fn hour12(&self) -> i64 {
        match self.hour % 12 {
            0 => 12,
            h => h,
        }
    }

    /// glibc's ISO 8601 week-based year and week number.
    fn iso_year_week(&self) -> (i64, i64) {
        fn iso_week_days(yday: i64, wday: i64) -> i64 {
            const BIG_ENOUGH_MULTIPLE_OF_7: i64 = (366 / 7 + 2) * 7;
            yday - (yday - wday + 4 + BIG_ENOUGH_MULTIPLE_OF_7) % 7 + 4 - 1
        }
        let leap = |y: i64| i64::from(crate::date::is_leap(y));
        let mut year = self.year;
        let mut days = iso_week_days(self.yday, self.wday);
        if days < 0 {
            year -= 1;
            days = iso_week_days(self.yday + 365 + leap(year), self.wday);
        } else {
            let next = iso_week_days(self.yday - (365 + leap(year)), self.wday);
            if next >= 0 {
                year += 1;
                days = next;
            }
        }
        (year, days / 7 + 1)
    }
}

const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// The output no longer fits the buffer: the C call returns 0.
#[derive(Debug)]
struct Full;

/// Padding flag: `_`, `-`, `0`, or none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pad {
    Default,
    Space,
    None,
    Zero,
}

/// Case conversion applied when copying text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Case {
    Keep,
    Upper,
    Lower,
}

/// glibc's output buffer of `maxsize` bytes: the result and its NUL must fit.
struct Out {
    buf: Vec<u8>,
    maxsize: usize,
}

impl Out {
    fn reserve(&self, n: usize) -> Result<(), Full> {
        if n >= self.maxsize.saturating_sub(self.buf.len()) {
            Err(Full)
        } else {
            Ok(())
        }
    }

    fn fill(&mut self, byte: u8, n: usize) -> Result<(), Full> {
        self.reserve(n)?;
        self.buf.resize(self.buf.len() + n, byte);
        Ok(())
    }

    /// glibc's `cpy`: `text` right-aligned in `width`, padded with zeros for the `0`
    /// flag and spaces otherwise.
    fn cpy(&mut self, text: &[u8], width: usize, pad: Pad, case: Case) -> Result<(), Full> {
        let delta = width.saturating_sub(text.len());
        self.reserve(text.len().saturating_add(delta))?;
        let fill = if pad == Pad::Zero { b'0' } else { b' ' };
        self.buf.resize(self.buf.len() + delta, fill);
        self.buf.extend(text.iter().map(|&b| match case {
            Case::Keep => b,
            Case::Upper => b.to_ascii_uppercase(),
            Case::Lower => b.to_ascii_lowercase(),
        }));
        Ok(())
    }
}

/// glibc's `strftime(buf, maxsize, fmt, tm)` in the C locale; `None` when it returns 0
/// because the output doesn't fit.
fn c_strftime(fmt: &[u8], tm: &Tm, maxsize: usize) -> Option<Vec<u8>> {
    let mut out = Out {
        // Capacity only; `maxsize` alone decides what fits.
        buf: Vec::with_capacity(fmt.len().saturating_mul(2).min(maxsize)),
        maxsize,
    };
    render(fmt, tm, &mut out).ok()?;
    Some(out.buf)
}

/// A numeric conversion: minimum digits, value, and whether it space-pads by default.
struct Number {
    digits: usize,
    value: i64,
    space_pad: bool,
}

/// What a conversion produces before padding.
enum Conversion {
    Text(&'static str),
    Number(Number),
    /// `%s`: its digits are always 1, so only the field width pads it.
    Seconds(i64),
    Sub(&'static str),
    /// Nothing at all, not even padding (`%z` when `tm_isdst` is negative).
    Nothing,
    Bad,
}

fn render(fmt: &[u8], tm: &Tm, out: &mut Out) -> Result<(), Full> {
    let mut f = 0;
    while let Some(&c) = fmt.get(f) {
        if c != b'%' {
            out.cpy(&[c], 0, Pad::Default, Case::Keep)?;
            f += 1;
            continue;
        }
        let percent = f;
        let mut pad = Pad::Default;
        let mut upper = false;
        let mut change_case = false;
        loop {
            f += 1;
            match fmt.get(f) {
                Some(b'_') => pad = Pad::Space,
                Some(b'-') => pad = Pad::None,
                Some(b'0') => pad = Pad::Zero,
                Some(b'^') => upper = true,
                Some(b'#') => change_case = true,
                _ => break,
            }
        }
        let mut width: usize = 0;
        while let Some(&digit) = fmt.get(f).filter(|b| b.is_ascii_digit()) {
            let max = usize::try_from(i32::MAX).unwrap_or(usize::MAX);
            width = width
                .checked_mul(10)
                .and_then(|w| w.checked_add(usize::from(digit - b'0')))
                .filter(|&w| w <= max)
                .unwrap_or(max);
            f += 1;
        }
        let modifier = match fmt.get(f) {
            Some(&m @ (b'E' | b'O')) => {
                f += 1;
                Some(m)
            }
            _ => None,
        };
        let format_char = fmt.get(f).copied();
        let (conversion, case) = convert(format_char, modifier, tm, upper, change_case);

        match conversion {
            Conversion::Text(text) => out.cpy(text.as_bytes(), width, pad, case)?,
            Conversion::Number(n) => {
                let pad = match pad {
                    Pad::Default | Pad::Space if n.space_pad => Pad::Space,
                    other => other,
                };
                do_number(out, n.value, n.digits.max(width), width, pad)?;
            }
            Conversion::Seconds(t) => do_number(out, t, 1, width, pad)?,
            Conversion::Sub(subfmt) => {
                let mut inner = Out {
                    buf: Vec::new(),
                    maxsize: out.maxsize.saturating_sub(out.buf.len()),
                };
                render(subfmt.as_bytes(), tm, &mut inner)?;
                out.cpy(&inner.buf, width, pad, case)?;
            }
            Conversion::Nothing => {}
            Conversion::Bad => {
                // Copy from the '%' through the offending byte; at the end of the
                // format, through the last byte.
                let end = (f + 1).min(fmt.len());
                out.cpy(fmt.get(percent..end).unwrap_or_default(), width, pad, case)?;
                f = end;
                continue;
            }
        }
        f += 1;
    }
    Ok(())
}

/// glibc's `do_number` and `do_number_sign_and_padding`.
fn do_number(out: &mut Out, value: i64, digits: usize, width: usize, pad: Pad) -> Result<(), Full> {
    let mut digit_buf = [0u8; 20];
    let magnitude = decimal_digits(value.unsigned_abs(), &mut digit_buf);
    let negative = value < 0;
    let len = magnitude.len() + usize::from(negative);
    let mut width = width;
    let mut sign_written = false;
    if pad != Pad::None && digits > len {
        let padding = digits - len;
        if pad == Pad::Space {
            out.fill(b' ', padding)?;
            width = width.saturating_sub(padding);
        } else {
            out.reserve(digits)?;
            if negative {
                out.buf.push(b'-');
                sign_written = true;
            }
            out.fill(b'0', padding)?;
            width = 0;
        }
    }
    let mut text_buf = [0u8; 21];
    let mut text_len = 0;
    if negative && !sign_written {
        text_buf[0] = b'-';
        text_len = 1;
    }
    text_buf[text_len..text_len + magnitude.len()].copy_from_slice(magnitude);
    text_len += magnitude.len();
    out.cpy(&text_buf[..text_len], width, pad, Case::Keep)
}

/// The decimal digits of `n` (at least one), written at the end of `buf`.
fn decimal_digits(mut n: u64, buf: &mut [u8; 20]) -> &[u8] {
    let mut start = buf.len();
    loop {
        start -= 1;
        buf[start] = b'0' + u8::try_from(n % 10).unwrap_or(0);
        n /= 10;
        if n == 0 {
            break;
        }
    }
    &buf[start..]
}

/// The conversion for `%<modifier><format_char>` and the case it copies text in.
fn convert(
    format_char: Option<u8>,
    modifier: Option<u8>,
    tm: &Tm,
    upper: bool,
    change_case: bool,
) -> (Conversion, Case) {
    let base_case = if upper { Case::Upper } else { Case::Keep };
    let e = modifier == Some(b'E');
    let o = modifier == Some(b'O');
    let number = |digits: usize, value: i64| {
        Conversion::Number(Number {
            digits,
            value,
            space_pad: false,
        })
    };
    let space_number = |digits: usize, value: i64| {
        Conversion::Number(Number {
            digits,
            value,
            space_pad: true,
        })
    };
    // Weekday and month names: `#` swaps to upper case.
    let name_case = if change_case { Case::Upper } else { base_case };
    let wday = usize::try_from(tm.wday).unwrap_or(0);
    let month = usize::try_from(tm.month - 1).unwrap_or(0);
    let weekday_name = WEEKDAYS.get(wday).copied().unwrap_or("");
    let month_name = MONTHS.get(month).copied().unwrap_or("");
    let bad = (Conversion::Bad, base_case);

    let Some(fc) = format_char else {
        return bad;
    };
    let conversion = match fc {
        b'%' => Conversion::Text("%"),
        b'a' | b'A' if modifier.is_some() => return bad,
        b'a' => {
            return (
                Conversion::Text(weekday_name.get(..3).unwrap_or("")),
                name_case,
            )
        }
        b'A' => return (Conversion::Text(weekday_name), name_case),
        b'b' | b'h' | b'B' if e => return bad,
        b'b' | b'h' => {
            return (
                Conversion::Text(month_name.get(..3).unwrap_or("")),
                name_case,
            )
        }
        b'B' => return (Conversion::Text(month_name), name_case),
        b'c' | b'x' | b'X' if o => return bad,
        b'D' | b'F' if modifier.is_some() => return bad,
        b'c' => Conversion::Sub("%a %b %e %H:%M:%S %Y"),
        b'x' | b'D' => Conversion::Sub("%m/%d/%y"),
        b'X' | b'T' => Conversion::Sub("%H:%M:%S"),
        b'F' => Conversion::Sub("%Y-%m-%d"),
        b'r' => Conversion::Sub("%I:%M:%S %p"),
        b'R' => Conversion::Sub("%H:%M"),
        b'C' => number(1, tm.year.div_euclid(100)),
        b'd' | b'e' | b'H' | b'I' | b'k' | b'l' | b'j' | b'M' | b'm' | b'S' | b'U' | b'V'
        | b'g' | b'G' | b'W' | b'w'
            if e =>
        {
            return bad
        }
        b'd' => number(2, tm.mday),
        b'e' => space_number(2, tm.mday),
        b'H' => number(2, tm.hour),
        b'I' => number(2, tm.hour12()),
        b'k' => space_number(2, tm.hour),
        b'l' => space_number(2, tm.hour12()),
        b'j' => number(3, tm.yday + 1),
        b'M' => number(2, tm.minute),
        b'm' => number(2, tm.month),
        b'S' => number(2, tm.second),
        b'U' => number(2, (tm.yday - tm.wday + 7) / 7),
        b'W' => number(2, (tm.yday - (tm.wday - 1 + 7) % 7 + 7) / 7),
        b'w' => number(1, tm.wday),
        b'V' => number(2, tm.iso_year_week().1),
        b'g' => number(2, tm.iso_year_week().0.rem_euclid(100)),
        b'G' => number(1, tm.iso_year_week().0),
        b'u' => number(1, (tm.wday - 1 + 7) % 7 + 1),
        b'Y' if o => return bad,
        b'Y' => number(1, tm.year),
        b'y' => number(2, tm.year.rem_euclid(100)),
        b's' => Conversion::Seconds(tm.epoch),
        b'n' => Conversion::Text("\n"),
        b't' => Conversion::Text("\t"),
        b'p' | b'P' => {
            let text = if tm.hour > 11 { "PM" } else { "AM" };
            let case = if fc == b'P' || change_case {
                Case::Lower
            } else {
                base_case
            };
            return (Conversion::Text(text), case);
        }
        // `tm_zone` is unset: the zone name is empty.
        b'Z' => {
            let case = if change_case { Case::Lower } else { base_case };
            return (Conversion::Text(""), case);
        }
        b'z' => Conversion::Nothing,
        _ => return bad,
    };
    (conversion, base_case)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(d: [i64; 7], offset: Option<i64>, f: &str) -> PyResult<Value> {
        let mut items: Vec<Value> = d.iter().map(|&n| Value::int(n)).collect();
        items.push(offset.map_or(Value::None, Value::int));
        items.push(Value::int(0));
        strftime(&[Value::tuple(items), Value::str(f)], &[])
    }

    #[test]
    fn basics() {
        let d = [5, 1, 2, 3, 4, 5, 6];
        assert_eq!(
            fmt(d, None, "%Y|%G|%C|%F|%y|%Q|%E|%"),
            Ok(Value::str("5|4|0|5-01-02|05|%Q|%E|%"))
        );
        assert_eq!(fmt(d, None, "%s"), Ok(Value::str("-62009268955")));
        assert_eq!(fmt(d, None, "%015s"), Ok(Value::str("000-62009268955")));
        assert_eq!(fmt(d, None, "%5:z"), Ok(Value::str("  %5:z")));
        assert_eq!(
            fmt(
                [2024, 12, 30, 13, 4, 5, 6],
                Some(19800),
                "%Z|%z|%:z|%f|%%%z"
            ),
            Ok(Value::str("UTC+05:30|+0530|+05:30|000006|%+0530"))
        );
        assert_eq!(fmt(d, None, "%3000Y"), Ok(Value::str("")));
    }

    #[test]
    fn flags_modifiers_and_zone() {
        let d = [1969, 12, 31, 23, 59, 59, 7];
        assert_eq!(
            fmt(
                d,
                Some(-3601),
                "%5\u{e9}|%^10q|%-10d|%_10d|%#p|%^P|%010s|%#a|%^#p|%_0e|%-k|%010A|%Ob|%OB|%Ey|\
                 %EC|%Ou|%G-%V-%g|%U|%W|%j|%I%l|%5Z|%-5z|%:%z|%Z|%f|%z|%:z"
            ),
            Ok(Value::str(
                "   %5\u{e9}|     %^10Q|        31|        31|pm|pm|00000000-1|WED|pm|31|23|\
                 0Wednesday|Dec|December|69|19|3|1970-01-70|52|52|365|1111|     ||%:-010001|\
                 UTC-01:00:01|000007|-010001|-01:00:01"
            ))
        );
    }

    #[test]
    fn runs_share_one_growing_buffer() {
        let d = [1969, 12, 31, 23, 59, 59, 7];
        let len = |f: &str| match fmt(d, None, f) {
            Ok(Value::Str(s)) => s.len(),
            _ => usize::MAX,
        };
        assert_eq!(len(&format!("{}\0%3000Y", "x".repeat(5000))), 8001);
        assert_eq!(len(&format!("\u{e9}{}%3000Y", "x".repeat(1100))), 1101);
        assert_eq!(len(&format!("{}\0%3000Y", "%-z".repeat(10))), 3001);
        assert_eq!(len("%Y\0abc%3000Y"), 8);
        assert_eq!(len("%2047Y"), 2047);
        assert_eq!(len("%2048Y"), 0);
    }
}
