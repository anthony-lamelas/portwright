//! Number conversions as Python 3.12 performs them: `repr(float)`,
//! `str(int)`, `float(int)`, `int(text, base)` and `float(text)`, plus the
//! `repr` of strings and bytes that their error messages need.

pub(crate) mod decimal;
mod ryu;
mod unicode_data;

use std::fmt::Write as _;

use num_bigint::BigInt;
use num_traits::{ToPrimitive, Zero};

use crate::repr::value_str;
use crate::value::{check_builtins, PcResult, PyError, PyStr, Value};
use unicode_data::{DECIMAL_ZEROS, PRINTABLE_RANGES, WHITESPACE};

/// `sys.get_int_max_str_digits()` default: the int <-> decimal text limit.
pub const MAX_STR_DIGITS: usize = 4300;

/// CPython only checks the digit limit above this many digits
/// (`_PY_LONG_MAX_STR_DIGITS_THRESHOLD`).
const STR_DIGITS_THRESHOLD: usize = 640;

/// Any int with more bits than this has more than [`MAX_STR_DIGITS`]
/// decimal digits (log2(10) * 4300 is about 14284.5).
const SURELY_TOO_MANY_BITS: u64 = 14_300;

/// `repr`/`%.200R` truncation used by `int()` error messages.
const INT_ERROR_REPR_LIMIT: usize = 200;

const INT_TOO_LARGE_FOR_FLOAT: &str = "int too large to convert to float";

// ---------------------------------------------------------------- Unicode

/// `str.isprintable()` for one character (Unicode 15.0).
#[must_use]
pub fn is_printable(c: char) -> bool {
    let cp = u32::from(c);
    let idx = PRINTABLE_RANGES.partition_point(|&(start, _)| start <= cp);
    idx > 0
        && PRINTABLE_RANGES
            .get(idx - 1)
            .is_some_and(|&(_, end)| cp <= end)
}

/// `str.isspace()` for one character (Unicode 15.0).
#[must_use]
pub fn is_py_whitespace(c: char) -> bool {
    WHITESPACE.binary_search(&u32::from(c)).is_ok()
}

/// The decimal value of a Unicode `Nd` digit (`unicodedata.decimal`).
#[must_use]
pub fn decimal_digit_value(c: char) -> Option<u32> {
    let cp = u32::from(c);
    if cp < 0x80 {
        // The only ASCII digits are 0-9 (the next zero is U+0660).
        return c.to_digit(10);
    }
    let idx = DECIMAL_ZEROS.partition_point(|&zero| zero <= cp);
    let zero = *DECIMAL_ZEROS.get(idx.checked_sub(1)?)?;
    let value = cp - zero;
    (value < 10).then_some(value)
}

/// CPython's `Py_ISSPACE`: ASCII whitespace only.
fn is_c_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// `_PyUnicode_TransformDecimalAndSpaceToASCII`: Unicode whitespace becomes
/// a space and `Nd` digits become ASCII digits. The first other non-ASCII
/// character becomes `?` and ends the text.
fn transform_to_ascii(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for c in s.chars() {
        if let Some(b) = u8::try_from(u32::from(c)).ok().filter(|&b| b < 127) {
            out.push(b);
        } else if is_py_whitespace(c) {
            out.push(b' ');
        } else if let Some(d) = decimal_digit_value(c).and_then(|d| u8::try_from(d).ok()) {
            out.push(b'0' + d);
        } else {
            out.push(b'?');
            break;
        }
    }
    out
}

/// The text as bytes when every code point is below 127, which is exactly
/// when [`transform_to_ascii`] would return it unchanged.
/// [`ascii_code_points`] into `buf`, for text that fits it.
fn narrow_short_ascii<'a>(text: &[u32], buf: &'a mut [u8]) -> Option<&'a [u8]> {
    let buf = buf.get_mut(..text.len())?;
    // One pass: copy the low bytes and collect the high bits.
    let mut high = 0_u32;
    for (dst, &cp) in buf.iter_mut().zip(text) {
        high |= cp | cp.wrapping_add(1);
        *dst = cp.to_le_bytes()[0];
    }
    // Every code point is below 127 exactly when no `cp` or `cp + 1`
    // reaches bit 7 or above (`cp + 1 <= 127`).
    (high < 0x80).then_some(&*buf)
}

fn ascii_code_points(text: &[u32]) -> Option<Vec<u8>> {
    if !text.iter().all(|&cp| cp < 127) {
        return None;
    }
    let mut out = Vec::with_capacity(text.len());
    out.extend(text.iter().map(|&cp| u8::try_from(cp).unwrap_or(b'?')));
    Some(out)
}

// ---------------------------------------------------------------- repr

/// Picks the quote `repr` uses: `"` only when the text has `'` and no `"`.
fn repr_quote(has_single: bool, has_double: bool) -> char {
    if has_single && !has_double {
        '"'
    } else {
        '\''
    }
}

/// Python's `repr()` of a `str`.
#[must_use]
pub fn str_repr(s: &str) -> String {
    str_repr_code_points(&s.chars().map(u32::from).collect::<Vec<_>>())
}

/// Python's `repr()` of a `str` given as code points, which may include
/// lone surrogates (shown as `\\udXXX`, like any non-printable).
#[must_use]
pub fn str_repr_code_points(text: &[u32]) -> String {
    let has = |c: char| text.contains(&u32::from(c));
    let quote = repr_quote(has('\''), has('"'));
    let mut out = String::with_capacity(text.len() + 2);
    out.push(quote);
    for &cp in text {
        let printable = char::from_u32(cp).filter(|&c| cp < 0x7f || is_printable(c));
        match char::from_u32(cp) {
            Some('\\') => out.push_str("\\\\"),
            Some('\t') => out.push_str("\\t"),
            Some('\n') => out.push_str("\\n"),
            Some('\r') => out.push_str("\\r"),
            Some(c) if c == quote => {
                out.push('\\');
                out.push(c);
            }
            _ if cp < 0x20 || cp == 0x7f => {
                let _ = write!(out, "\\x{cp:02x}");
            }
            _ if printable.is_some() => out.extend(printable),
            _ if cp <= 0xff => {
                let _ = write!(out, "\\x{cp:02x}");
            }
            _ if cp <= 0xffff => {
                let _ = write!(out, "\\u{cp:04x}");
            }
            _ => {
                let _ = write!(out, "\\U{cp:08x}");
            }
        }
    }
    out.push(quote);
    out
}

/// Code points as a `String`, with lone surrogates replaced by U+FFFD
/// (neither whitespace nor a digit, so number parsing treats it the same).
fn lossy_code_points(text: &[u32]) -> String {
    text.iter()
        .map(|&cp| char::from_u32(cp).unwrap_or('\u{fffd}'))
        .collect()
}

/// Which builtin's repr of a byte string to reproduce.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BytesFlavor {
    Bytes,
    /// `bytearray_repr` escapes `'` even inside `"` quotes.
    ByteArray,
}

/// Python's `repr()` of a `bytes` object.
#[must_use]
pub fn bytes_repr(b: &[u8]) -> String {
    bytes_repr_flavored(b, BytesFlavor::Bytes)
}

fn bytes_repr_flavored(b: &[u8], flavor: BytesFlavor) -> String {
    let quote = repr_quote(b.contains(&b'\''), b.contains(&b'"'));
    let mut out = String::with_capacity(b.len() + 3);
    out.push('b');
    out.push(quote);
    for &byte in b {
        let c = char::from(byte);
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ if c == quote || (c == '\'' && flavor == BytesFlavor::ByteArray) => {
                out.push('\\');
                out.push(c);
            }
            _ if !(0x20..0x7f).contains(&byte) => {
                let _ = write!(out, "\\x{byte:02x}");
            }
            _ => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// Python's `repr()` of a `bytearray`.
#[must_use]
pub fn bytearray_repr(b: &[u8]) -> String {
    format!(
        "bytearray({})",
        bytes_repr_flavored(b, BytesFlavor::ByteArray)
    )
}

// ---------------------------------------------------------------- float -> text

/// Python's `repr()` of a float: the shortest text that reads back as `x`,
/// in exponent form iff the decimal exponent is below -4 or at least 16.
#[must_use]
pub fn float_repr(x: f64) -> String {
    float_repr_ascii(x).as_str().to_owned()
}

/// [`float_repr`] as code points.
#[must_use]
pub fn float_repr_py_str(x: f64) -> PyStr {
    ascii_py_str(float_repr_ascii(x).as_bytes())
}

/// A `PyStr` from ASCII bytes, in one exact-size allocation.
pub(crate) fn ascii_py_str(bytes: &[u8]) -> PyStr {
    PyStr::from_code_points(bytes.iter().map(|&b| u32::from(b)).collect())
}

/// [`float_repr`] on the stack (a float repr has at most 24 bytes).
pub(crate) fn float_repr_ascii(x: f64) -> ShortAscii {
    let mut out = ShortAscii::default();
    if x.is_nan() {
        out.extend(b"nan");
        return out;
    }
    if x.is_infinite() {
        out.extend(if x < 0.0 { b"-inf" } else { b"inf" });
        return out;
    }
    let negative = x.is_sign_negative();
    if negative {
        out.push(b'-');
    }
    if x == 0.0 {
        out.extend(b"0.0");
        return out;
    }
    let (digits, exp) = shortest_digits(x.abs());
    layout_repr_digits(&mut out, &digits, exp);
    out
}

/// ASCII text of at most 32 bytes, kept on the stack. Writes past the end
/// are dropped (callers stay well below it).
#[derive(Clone, Copy, Default)]
pub(crate) struct ShortAscii {
    buf: [u8; 32],
    len: usize,
}

impl ShortAscii {
    fn push(&mut self, b: u8) {
        if let Some(slot) = self.buf.get_mut(self.len) {
            *slot = b;
            self.len += 1;
        }
    }

    fn extend(&mut self, bytes: &[u8]) {
        let end = (self.len + bytes.len()).min(self.buf.len());
        if let (Some(dst), Some(src)) =
            (self.buf.get_mut(self.len..end), bytes.get(..end - self.len))
        {
            dst.copy_from_slice(src);
            self.len = end;
        }
    }

    fn zeros(&mut self, n: usize) {
        let end = (self.len + n).min(self.buf.len());
        if let Some(dst) = self.buf.get_mut(self.len..end) {
            dst.fill(b'0');
            self.len = end;
        }
    }

    /// The bytes written so far.
    pub(crate) fn as_bytes(&self) -> &[u8] {
        self.buf.get(..self.len).unwrap_or_default()
    }

    /// The text written so far (always ASCII).
    pub(crate) fn as_str(&self) -> &str {
        std::str::from_utf8(self.as_bytes()).unwrap_or("")
    }
}

/// Short ASCII text kept on the stack: Rust's `{:e}` text for an `f64`
/// with at most 17 significant digits (22 bytes at most).
pub(crate) struct SciText {
    buf: [u8; 64],
    len: usize,
}

impl SciText {
    fn new() -> Self {
        SciText {
            buf: [0; 64],
            len: 0,
        }
    }

    /// The text written so far.
    pub(crate) fn as_str(&self) -> &str {
        std::str::from_utf8(self.buf.get(..self.len).unwrap_or_default()).unwrap_or("")
    }
}

impl std::fmt::Write for SciText {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        let end = self.len.checked_add(s.len()).ok_or(std::fmt::Error)?;
        let dst = self.buf.get_mut(self.len..end).ok_or(std::fmt::Error)?;
        dst.copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}

/// Significant digits on the stack (a shortest repr has at most 17).
pub(crate) struct SmallDigits {
    buf: [u8; 64],
    len: usize,
}

impl std::ops::Deref for SmallDigits {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        self.buf.get(..self.len).unwrap_or_default()
    }
}

/// Splits Rust `{:e}` text (`d.ddde-x`) into its significant digits,
/// without trailing zeros (at least one digit), and its exponent.
fn split_sci(sci: &str) -> Option<(SmallDigits, i64)> {
    let (mantissa, exp_text) = sci.split_once('e')?;
    let exp = exp_text.parse::<i64>().ok()?;
    let mut digits = SmallDigits {
        buf: [0; 64],
        len: 0,
    };
    for b in mantissa.bytes().filter(u8::is_ascii_digit) {
        *digits.buf.get_mut(digits.len)? = b;
        digits.len += 1;
    }
    while digits.len > 1 && digits.buf.get(digits.len - 1) == Some(&b'0') {
        digits.len -= 1;
    }
    Some((digits, exp))
}

/// The digits of [`shortest_sci`] (no trailing zeros, at least one) and
/// its decimal exponent, for a finite `x > 0`.
pub(crate) fn shortest_digits(x: f64) -> (SmallDigits, i64) {
    let (mut d, e) = ryu::shortest(x);
    let mut digits = SmallDigits {
        buf: [0; 64],
        len: 0,
    };
    if d > 0 {
        let mut e = i64::from(e);
        while d % 10 == 0 {
            d /= 10;
            e += 1;
        }
        let mut tmp = [0_u8; 20];
        let src = decimal::u64_to_ascii(d, &mut tmp);
        let n = src.len();
        if let Some(dst) = digits.buf.get_mut(..n) {
            dst.copy_from_slice(src);
            digits.len = n;
            // Ryu's digits are Rust's except maybe on an exact tie, which
            // the slower path below resolves the way `_Py_dg_dtoa` does.
            if !may_be_decimal_tie(x, n) {
                return (digits, e + i64::try_from(n).unwrap_or(0) - 1);
            }
        }
    }
    let sci = shortest_sci(x);
    split_sci(sci.as_str()).unwrap_or((digits, 0))
}

/// Shortest round-trip digits of a finite `x` in `{:e}` form, as
/// `_Py_dg_dtoa` mode 0 picks them: among the shortest strings that round
/// to `x`, the one nearest to `x`, ties to even. Rust's `{:e}` finds the
/// shortest length but may pick a different candidate on ties (it gives
/// `7.036874417766463e13` for `70368744177664.625`); its precision
/// formatting is exact with ties to even, so redo the digits at that length.
pub(crate) fn shortest_sci(x: f64) -> SciText {
    let mut sci = SciText::new();
    let _ = write!(sci, "{x:e}");
    let text = sci.as_str();
    let mantissa = text.split_once('e').map_or(text, |(m, _)| m);
    let n_digits = mantissa.bytes().filter(u8::is_ascii_digit).count();
    if !may_be_decimal_tie(x, n_digits) {
        // Rust's shortest digits are the nearest ones except on an exact
        // tie, so redoing them at the same length would change nothing.
        return sci;
    }
    let mut nearest = SciText::new();
    let _ = write!(nearest, "{:.*e}", n_digits.saturating_sub(1), x);
    if nearest.as_str() != sci.as_str() && nearest.as_str().parse::<f64>().is_ok_and(|y| y == x) {
        nearest
    } else {
        sci
    }
}

/// Whether `x` (finite, `>= 0`) can lie exactly halfway between two
/// `n_digits`-digit decimals: its exact decimal expansion must then have
/// exactly `n_digits + 1` significant digits, the last one a 5.
///
/// Writing `x = m * 2**e` with `m` odd: for `e < 0` the significant digits
/// are those of `m * 5**-e` (odd, so no trailing zeros, and it ends in 5
/// when `e < 0`); for `e >= 0`, `x = d * 10**e` with `d = m / 5**e` odd, so a
/// tie needs `5**e <= m < 2**53`, i.e. `e <= 22`. Either way, ruling a tie
/// out needs only `u128` arithmetic.
fn may_be_decimal_tie(x: f64, n_digits: usize) -> bool {
    const MANTISSA_BITS: u32 = 52;
    let bits = x.to_bits();
    let biased = i32::try_from((bits >> MANTISSA_BITS) & 0x7ff).unwrap_or(0);
    let fraction = bits & ((1_u64 << MANTISSA_BITS) - 1);
    let (mut m, mut e) = if biased == 0 {
        (fraction, -1074)
    } else {
        (fraction | (1_u64 << MANTISSA_BITS), biased - 1075)
    };
    if m == 0 {
        return false;
    }
    let tz = m.trailing_zeros();
    m >>= tz;
    e += i32::try_from(tz).unwrap_or(0);
    // A tie has at most 18 significant digits (shortest reprs have <= 17).
    // `n_digits + 1` significant digits means `10**n <= v < 10**(n+1)`.
    let has_digits = |v: u128| -> bool {
        let Ok(n) = u32::try_from(n_digits) else {
            return false;
        };
        match 10_u128.checked_pow(n) {
            Some(low) if v >= low => 10_u128.checked_pow(n + 1).is_none_or(|high| v < high),
            _ => false,
        }
    };
    if e < 0 {
        let k = e.unsigned_abs();
        if k > 26 {
            // 5**27 > 10**18: too many significant digits.
            return false;
        }
        // `m * 5**k` is odd and a multiple of 5 (k >= 1): it ends in 5.
        return has_digits(u128::from(m) * 5_u128.pow(k));
    }
    if e > 22 {
        return false;
    }
    // `v = m * 2**e` with `m` odd loses min(e, fives(m)) trailing zeros;
    // what is left ends in 5 exactly when `m` has more than `e` factors of
    // 5, and it is then `m / 5**e`.
    let e = e.unsigned_abs();
    let p = 5_u64.pow(e);
    if m % p != 0 || (m / p) % 5 != 0 {
        return false;
    }
    has_digits(u128::from(m / p))
}

/// Appends significant ASCII `digits` (value `d.ddd * 10**exp`) laid out
/// the way `repr(float)` does.
fn layout_repr_digits(out: &mut ShortAscii, digits: &[u8], exp: i64) {
    if !(-4..16).contains(&exp) {
        let (first, rest) = digits.split_at(1.min(digits.len()));
        out.extend(first);
        if !rest.is_empty() {
            out.push(b'.');
            out.extend(rest);
        }
        out.push(b'e');
        out.push(if exp < 0 { b'-' } else { b'+' });
        // `%02d`: |exp| is at most 324.
        let e = exp.unsigned_abs();
        if e >= 100 {
            out.push(b'0' + u8::try_from(e / 100 % 10).unwrap_or(0));
        }
        out.push(b'0' + u8::try_from(e / 10 % 10).unwrap_or(0));
        out.push(b'0' + u8::try_from(e % 10).unwrap_or(0));
        return;
    }
    let len = digits.len();
    match usize::try_from(exp) {
        Ok(int_exp) => {
            let int_len = int_exp + 1;
            if len <= int_len {
                out.extend(digits);
                out.zeros(int_len - len);
                out.extend(b".0");
            } else {
                let (int_part, frac) = digits.split_at(int_len);
                out.extend(int_part);
                out.push(b'.');
                out.extend(frac);
            }
        }
        Err(_) => {
            out.extend(b"0.");
            out.zeros(usize::try_from(exp.unsigned_abs() - 1).unwrap_or(0));
            out.extend(digits);
        }
    }
}

// ---------------------------------------------------------------- int -> text / float

fn int_str_limit_error() -> PyError {
    PyError::value_error(format!(
        "Exceeds the limit ({MAX_STR_DIGITS} digits) for integer string conversion; \
         use sys.set_int_max_str_digits() to increase the limit"
    ))
}

/// Python's `str()` of an int, including the 4300-digit limit (the sign
/// doesn't count).
///
/// # Errors
/// `ValueError` when `n` has more than [`MAX_STR_DIGITS`] digits.
pub fn int_str(n: &BigInt) -> PcResult<String> {
    let mut text = String::new();
    write_int_str(n, &mut text, |t| t.len())?;
    Ok(text)
}

/// [`int_str`] as code points.
///
/// # Errors
/// As [`int_str`].
pub fn int_str_py_str(n: &BigInt) -> PcResult<PyStr> {
    let mut text = Vec::new();
    write_int_str(n, &mut text, Vec::len)?;
    Ok(PyStr::from_code_points(text))
}

/// Appends [`int_str`] to `out` as code points. On error, `out` may hold
/// part of the text.
///
/// # Errors
/// As [`int_str`].
pub(crate) fn push_int_str(n: &BigInt, out: &mut Vec<u32>) -> PcResult<()> {
    let start = out.len();
    write_int_str(n, out, |o| o.len() - start)
}

/// Writes `str(n)` to `out`; `len` reads how much of `out` it has written.
fn write_int_str<S: decimal::AsciiSink>(
    n: &BigInt,
    out: &mut S,
    len: impl Fn(&S) -> usize,
) -> PcResult<()> {
    if n.bits() > SURELY_TOO_MANY_BITS {
        return Err(int_str_limit_error());
    }
    let negative = n.sign() == num_bigint::Sign::Minus;
    if negative {
        out.push_ascii(b"-");
    }
    decimal::magnitude_to_decimal(n.magnitude(), out);
    let digits = len(out) - usize::from(negative);
    if digits > MAX_STR_DIGITS {
        return Err(int_str_limit_error());
    }
    Ok(())
}

/// `2**k` as an exact f64, for `k` in `0..=1023`.
fn pow2(k: u64) -> f64 {
    f64::from_bits((k + 1023) << 52)
}

/// Python's `float()` of an int: correctly rounded, ties to even.
///
/// # Errors
/// `OverflowError` when the rounded value is at least `2**1024`.
pub fn int_to_float(n: &BigInt) -> PcResult<f64> {
    let mag = n.magnitude();
    let bits = mag.bits();
    let abs = if bits <= 64 {
        // `as` rounds to nearest, ties to even.
        mag.to_u64().map_or(0.0, |v| v as f64)
    } else if bits > 1024 {
        return Err(PyError::overflow_error(INT_TOO_LARGE_FOR_FLOAT));
    } else {
        // Keep the top 64 bits and fold every lower bit into a sticky bit;
        // 64 > 53 + 2, so rounding the top bits rounds the whole value.
        let shift = bits - 64;
        let top = (mag >> shift).to_u64().unwrap_or(u64::MAX);
        let sticky = mag.trailing_zeros().is_some_and(|tz| tz < shift);
        let top = top | u64::from(sticky);
        (top as f64) * pow2(shift)
    };
    if abs.is_infinite() {
        return Err(PyError::overflow_error(INT_TOO_LARGE_FOR_FLOAT));
    }
    Ok(if n.sign() == num_bigint::Sign::Minus {
        -abs
    } else {
        abs
    })
}

// ---------------------------------------------------------------- text -> int

/// Why CPython's `PyLong_FromString` failed.
enum IntSyntaxError {
    /// Not a valid literal.
    Invalid,
    /// Too many digits for a non-power-of-two base; carries the count.
    TooManyDigits(usize),
}

/// The digit value of an ASCII byte (`_PyLong_DigitValue`), 37 if none.
fn ascii_digit_value(b: u8) -> u32 {
    match b {
        b'0'..=b'9' => u32::from(b - b'0'),
        b'a'..=b'z' => u32::from(b - b'a') + 10,
        b'A'..=b'Z' => u32::from(b - b'A') + 10,
        _ => 37,
    }
}

/// A NUL-terminated view of a byte buffer, as CPython's C parsers see it.
struct CStr<'a>(&'a [u8]);

impl CStr<'_> {
    fn at(&self, i: usize) -> u8 {
        self.0.get(i).copied().unwrap_or(0)
    }
}

/// Port of CPython 3.12 `PyLong_FromString`: returns the value and the
/// index where parsing stopped.
fn long_from_c_string(buf: &[u8], base: u32) -> Result<(BigInt, usize), IntSyntaxError> {
    let s = CStr(buf);
    let mut p = 0;
    while s.at(p) != 0 && is_c_space(s.at(p)) {
        p += 1;
    }
    let mut negative = false;
    if s.at(p) == b'+' {
        p += 1;
    } else if s.at(p) == b'-' {
        p += 1;
        negative = true;
    }
    let mut base = base;
    let mut error_if_nonzero = false;
    let next = s.at(p + 1);
    if base == 0 {
        base = if s.at(p) != b'0' {
            10
        } else if matches!(next, b'x' | b'X') {
            16
        } else if matches!(next, b'o' | b'O') {
            8
        } else if matches!(next, b'b' | b'B') {
            2
        } else {
            error_if_nonzero = true;
            10
        };
    }
    if s.at(p) == b'0'
        && ((base == 16 && matches!(next, b'x' | b'X'))
            || (base == 8 && matches!(next, b'o' | b'O'))
            || (base == 2 && matches!(next, b'b' | b'B')))
    {
        p += 2;
        if s.at(p) == b'_' {
            p += 1;
        }
    }

    // long_from_string_base
    let start = p;
    if s.at(start) == b'_' {
        return Err(IntSyntaxError::Invalid);
    }
    let mut prev = 0u8;
    let mut digits = 0usize;
    loop {
        if base >= 10 {
            // A run of decimal digits, all valid here, in one scan.
            let run = buf
                .get(p..)
                .map_or(0, |r| r.iter().take_while(|b| b.is_ascii_digit()).count());
            if run > 0 {
                digits += run;
                p += run;
                prev = s.at(p - 1);
            }
        }
        let c = s.at(p);
        if c == b'_' {
            if prev == b'_' {
                return Err(IntSyntaxError::Invalid);
            }
        } else if ascii_digit_value(c) < base {
            digits += 1;
        } else {
            break;
        }
        prev = c;
        p += 1;
    }
    if prev == b'_' || p == start {
        return Err(IntSyntaxError::Invalid);
    }
    let end = p;
    while s.at(p) != 0 && is_c_space(s.at(p)) {
        p += 1;
    }
    if s.at(p) != 0 {
        return Err(IntSyntaxError::Invalid);
    }
    let is_binary_base = base & (base - 1) == 0;
    if !is_binary_base && digits > STR_DIGITS_THRESHOLD && digits > MAX_STR_DIGITS {
        return Err(IntSyntaxError::TooManyDigits(digits));
    }
    let literal = buf.get(start..end).unwrap_or_default();
    let magnitude = if base == 10 {
        // Every byte is a decimal digit or `_` (checked above).
        BigInt::from(decimal::decimal_to_magnitude(literal))
    } else {
        let clean: Vec<u8> = literal.iter().copied().filter(|&b| b != b'_').collect();
        BigInt::parse_bytes(&clean, base).ok_or(IntSyntaxError::Invalid)?
    };
    if error_if_nonzero && !magnitude.is_zero() {
        return Err(IntSyntaxError::Invalid);
    }
    Ok((if negative { -magnitude } else { magnitude }, p))
}

/// Parses `buf` as `int()` does, building the "invalid literal" message
/// from `literal_repr` only when needed.
fn int_from_buffer(
    buf: &[u8],
    base: u32,
    literal_repr: impl FnOnce() -> String,
) -> PcResult<BigInt> {
    match long_from_c_string(buf, base) {
        Ok((value, end)) if end == buf.len() => Ok(value),
        Err(IntSyntaxError::TooManyDigits(digits)) => Err(PyError::value_error(format!(
            "Exceeds the limit ({MAX_STR_DIGITS} digits) for integer string conversion: \
             value has {digits} digits; use sys.set_int_max_str_digits() to increase the limit"
        ))),
        Ok(_) | Err(IntSyntaxError::Invalid) => {
            let shown: String = literal_repr().chars().take(INT_ERROR_REPR_LIMIT).collect();
            Err(PyError::value_error(format!(
                "invalid literal for int() with base {base}: {shown}"
            )))
        }
    }
}

/// Python's `int(text, base)` for `str` text.
///
/// # Errors
/// `ValueError` for an invalid literal or too many digits.
pub fn parse_int_str(text: &str, base: u32) -> PcResult<BigInt> {
    int_from_buffer(&transform_to_ascii(text), base, || str_repr(text))
}

/// Python's `int(text, base)` for `bytes` or `bytearray` text.
///
/// # Errors
/// `ValueError` for an invalid literal or too many digits.
pub fn parse_int_bytes(text: &[u8], base: u32) -> PcResult<BigInt> {
    int_from_buffer(text, base, || {
        bytes_repr(text.get(..INT_ERROR_REPR_LIMIT).unwrap_or(text))
    })
}

/// Python's `int(text, base)` with an explicit base, for any [`Value`].
///
/// # Errors
/// `TypeError` for a non-integer base or non-string text, `ValueError` for
/// a base outside 0 and 2..=36, an invalid literal or too many digits.
pub fn parse_int(text: &Value, base: &Value) -> PcResult<Value> {
    let base = int_base(base)?;
    match text {
        Value::Str(s) => parse_int_code_points(s.code_points(), base).map(Value::Int),
        Value::Bytes(b) | Value::ByteArray(b) => parse_int_bytes(b, base).map(Value::Int),
        _ => Err(PyError::type_error(
            "int() can't convert non-string with explicit base",
        )),
    }
}

/// Python's `int(text, base)` for `str` text given as code points (which
/// may include lone surrogates, so can't be a Rust `String`).
///
/// # Errors
/// As [`parse_int`].
pub fn parse_int_code_points(text: &[u32], base: u32) -> PcResult<BigInt> {
    let mut small = [0_u8; 64];
    if let Some(buf) = narrow_short_ascii(text, &mut small) {
        return int_from_buffer(buf, base, || str_repr_code_points(text));
    }
    if let Some(ascii) = ascii_code_points(text) {
        return int_from_buffer(&ascii, base, || str_repr_code_points(text));
    }
    int_from_buffer(&transform_to_ascii(&lossy_code_points(text)), base, || {
        str_repr_code_points(text)
    })
}

/// Validates the `base` argument of `int()`.
fn int_base(base: &Value) -> PcResult<u32> {
    let base = match base {
        Value::Int(b) => b.to_u32(),
        Value::Bool(b) => Some(u32::from(*b)),
        other => {
            return Err(PyError::type_error(format!(
                "'{}' object cannot be interpreted as an integer",
                other.type_name()
            )))
        }
    };
    base.filter(|&b| b == 0 || (2..=36).contains(&b))
        .ok_or_else(|| PyError::value_error("int() base must be >= 2 and <= 36, or 0"))
}

// ---------------------------------------------------------------- text -> float

/// `_Py_parse_inf_or_nan` followed by a full-match check.
fn parse_inf_or_nan(t: &[u8]) -> Option<f64> {
    let (negative, body) = match t.split_first() {
        Some((b'-', rest)) => (true, rest),
        Some((b'+', rest)) => (false, rest),
        _ => (false, t),
    };
    let value = if body.eq_ignore_ascii_case(b"inf") || body.eq_ignore_ascii_case(b"infinity") {
        f64::INFINITY
    } else if body.eq_ignore_ascii_case(b"nan") {
        f64::NAN
    } else {
        return None;
    };
    Some(if negative { -value } else { value })
}

/// Whether `t` fully matches `[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?`, the
/// part of `_Py_dg_strtod`'s grammar that must consume the whole text.
fn is_decimal_float_literal(t: &[u8]) -> bool {
    fn skip_digits(t: &[u8], mut i: usize) -> usize {
        while t.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        i
    }
    let mut i = usize::from(matches!(t.first(), Some(b'+' | b'-')));
    let int_start = i;
    i = skip_digits(t, i);
    let mut mantissa_digits = i - int_start;
    if t.get(i) == Some(&b'.') {
        let frac_start = i + 1;
        i = skip_digits(t, frac_start);
        mantissa_digits += i - frac_start;
    }
    if mantissa_digits == 0 {
        return false;
    }
    if matches!(t.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(t.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let exp_start = i;
        i = skip_digits(t, i);
        if i == exp_start {
            return false;
        }
    }
    i == t.len()
}

/// `float_from_string_inner`: strips ASCII whitespace and parses the rest
/// completely, or returns `None`.
fn float_from_string_inner(s: &[u8]) -> Option<f64> {
    let mut a = 0;
    let mut b = s.len();
    while a < b && s.get(a).copied().is_some_and(is_c_space) {
        a += 1;
    }
    if a == b {
        return None;
    }
    while a + 1 < b && s.get(b - 1).copied().is_some_and(is_c_space) {
        b -= 1;
    }
    let t = s.get(a..b)?;
    if is_decimal_float_literal(t) {
        std::str::from_utf8(t).ok()?.parse::<f64>().ok()
    } else {
        parse_inf_or_nan(t)
    }
}

/// `_Py_string_to_number_with_underscores` around [`float_from_string_inner`].
fn float_from_buffer(s: &[u8]) -> Option<f64> {
    if !s.iter().any(|&b| b == 0 || b == b'_') {
        // No NUL and no underscores: the text is used as it is.
        return float_from_string_inner(s);
    }
    let c_len = s.iter().position(|&b| b == 0).unwrap_or(s.len());
    let c_text = s.get(..c_len)?;
    if !c_text.contains(&b'_') {
        return float_from_string_inner(s);
    }
    let mut dup = Vec::with_capacity(c_len);
    let mut prev = 0u8;
    for &c in c_text {
        if c == b'_' {
            if !prev.is_ascii_digit() {
                return None;
            }
        } else {
            dup.push(c);
            if prev == b'_' && !c.is_ascii_digit() {
                return None;
            }
        }
        prev = c;
    }
    if prev == b'_' || c_len != s.len() {
        return None;
    }
    float_from_string_inner(&dup)
}

fn float_parse_error(literal_repr: &str) -> PyError {
    PyError::value_error(format!("could not convert string to float: {literal_repr}"))
}

/// Python's `float(text)` for `str` text.
///
/// # Errors
/// `ValueError` when the text isn't a float literal.
pub fn parse_float_str(text: &str) -> PcResult<f64> {
    float_from_buffer(&transform_to_ascii(text)).ok_or_else(|| float_parse_error(&str_repr(text)))
}

/// Python's `float(text)` for `str` text given as code points (which may
/// include lone surrogates).
///
/// # Errors
/// `ValueError` when the text isn't a float literal.
pub fn parse_float_code_points(text: &[u32]) -> PcResult<f64> {
    let mut small = [0_u8; 64];
    if let Some(buf) = narrow_short_ascii(text, &mut small) {
        return float_from_buffer(buf)
            .ok_or_else(|| float_parse_error(&str_repr_code_points(text)));
    }
    if let Some(ascii) = ascii_code_points(text) {
        return float_from_buffer(&ascii)
            .ok_or_else(|| float_parse_error(&str_repr_code_points(text)));
    }
    float_from_buffer(&transform_to_ascii(&lossy_code_points(text)))
        .ok_or_else(|| float_parse_error(&str_repr_code_points(text)))
}

/// Python's `float(text)` for `bytes` text.
///
/// # Errors
/// `ValueError` when the text isn't a float literal.
pub fn parse_float_bytes(text: &[u8]) -> PcResult<f64> {
    float_from_buffer(text).ok_or_else(|| float_parse_error(&bytes_repr(text)))
}

/// Python's `float(text)` for `bytearray` text.
///
/// # Errors
/// `ValueError` when the text isn't a float literal.
pub fn parse_float_bytearray(text: &[u8]) -> PcResult<f64> {
    float_from_buffer(text).ok_or_else(|| float_parse_error(&bytearray_repr(text)))
}

/// The `TypeError` `float()` raises for an argument of type `type_name`.
#[must_use]
pub fn float_type_error(type_name: &str) -> PyError {
    PyError::type_error(format!(
        "float() argument must be a string or a real number, not '{type_name}'"
    ))
}

/// Python's `float(x)` for any [`Value`].
///
/// # Errors
/// `TypeError` for unsupported types, `ValueError` for invalid text,
/// `OverflowError` for ints of `2**1024` or more.
pub fn parse_float(x: &Value) -> PcResult<f64> {
    match x {
        Value::Float(f) => Ok(*f),
        Value::Bool(b) => Ok(f64::from(u8::from(*b))),
        Value::Int(n) => int_to_float(n),
        Value::Str(s) => parse_float_code_points(s.code_points()),
        Value::Bytes(b) => parse_float_bytes(b),
        Value::ByteArray(b) => parse_float_bytearray(b),
        other => Err(float_type_error(other.type_name())),
    }
}

// ---------------------------------------------------------------- public APIs

/// `pc_format.float_repr(x)`: `repr(x)`.
///
/// # Errors
/// The builtin guard's `TypeError`, or `ValueError` for over-long ints.
pub fn float_repr_api(x: &Value) -> PcResult<Value> {
    check_builtins(&[x])?;
    crate::repr::value_repr_py_str(x).map(Value::Str)
}

/// `pc_format.int_str(n)`: `str(n)`.
///
/// # Errors
/// The builtin guard's `TypeError`, or `ValueError` for over-long ints.
pub fn int_str_api(n: &Value) -> PcResult<Value> {
    check_builtins(&[n])?;
    value_str(n).map(Value::Str)
}

/// `pc_format.parse_int(text, base)`: `int(text, base)`.
///
/// # Errors
/// As `int()`.
pub fn parse_int_api(text: &Value, base: &Value) -> PcResult<Value> {
    check_builtins(&[text, base])?;
    parse_int(text, base)
}

/// `pc_format.parse_float(text)`: `float(text)`.
///
/// # Errors
/// As `float()`.
pub fn parse_float_api(text: &Value) -> PcResult<Value> {
    check_builtins(&[text])?;
    parse_float(text).map(Value::Float)
}

/// `pc_format.int_to_float(n)`: `float(n)`.
///
/// # Errors
/// As `float()`.
pub fn int_to_float_api(n: &Value) -> PcResult<Value> {
    check_builtins(&[n])?;
    parse_float(n).map(Value::Float)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_repr_matches_python() {
        let cases = [
            (1e16, "1e+16"),
            (1e15, "1000000000000000.0"),
            (5e-324, "5e-324"),
            (1e-5, "1e-05"),
            (0.0001, "0.0001"),
            (-0.0, "-0.0"),
            (0.1, "0.1"),
            (1.5e-7, "1.5e-07"),
            (123.456, "123.456"),
            (1.2345678901234568e17, "1.2345678901234568e+17"),
            (70368744177664.62, "70368744177664.62"),
        ];
        for (x, want) in cases {
            assert_eq!(float_repr(x), want, "input {x:?}");
        }
    }

    #[test]
    fn int_to_float_rounds_and_overflows() {
        let two = BigInt::from(2);
        let ok = two.pow(1024) - two.pow(971);
        assert_eq!(int_to_float(&ok).ok(), Some(f64::MAX));
        let bad = two.pow(1024) - two.pow(970);
        assert!(int_to_float(&bad).is_err());
        let tie = two.pow(53) + 1;
        assert_eq!(int_to_float(&tie).ok(), Some(9007199254740992.0));
    }

    #[test]
    fn str_repr_quotes_and_escapes() {
        assert_eq!(str_repr("it's"), "\"it's\"");
        assert_eq!(str_repr_code_points(&[0xd800, 0x27]), "\"\\ud800'\"");
        assert_eq!(str_repr("a\n\u{85}\u{e9}"), "'a\\n\\x85\u{e9}'");
    }

    #[test]
    fn unicode_digits() {
        assert_eq!(decimal_digit_value('\u{663}'), Some(3));
        assert_eq!(decimal_digit_value('\u{b2}'), None);
        assert!(is_py_whitespace('\u{1c}'));
    }

    #[test]
    fn parse_int_traps() {
        let ok = |t: &str, b: u32| parse_int_str(t, b).ok();
        assert_eq!(ok(" -0x_f ", 16), Some(BigInt::from(-15)));
        assert_eq!(ok("0_0", 0), Some(BigInt::from(0)));
        assert_eq!(ok("010", 0), None);
        assert_eq!(ok("1__0", 10), None);
        assert_eq!(ok("\u{663}\u{3000}", 10), Some(BigInt::from(3)));
        // The digit limit is checked before trailing garbage, and only
        // invalid-literal messages say "invalid literal".
        let long = "9".repeat(4301);
        let msg = |r: PcResult<BigInt>| r.err().map(|e| e.arg);
        assert_eq!(
            msg(parse_int_str(&format!("{long} "), 10)),
            Some(Value::str(
                "Exceeds the limit (4300 digits) for integer string conversion: value has \
                 4301 digits; use sys.set_int_max_str_digits() to increase the limit"
            ))
        );
        assert!(parse_int_str(&"1".repeat(5000), 2).is_ok());
        assert!(parse_int(&Value::int(5), &Value::int(10)).is_err());
        assert!(parse_int(&Value::str("1"), &Value::int(1)).is_err());
    }

    #[test]
    fn parse_float_traps() {
        let ok = |t: &str| parse_float_str(t).ok();
        assert_eq!(ok(" 1_0.5 "), Some(10.5));
        assert_eq!(ok("1_e5"), None);
        assert_eq!(ok("1.e5"), Some(1e5));
        assert_eq!(ok(".e5"), None);
        assert_eq!(ok("0x1p3"), None);
        assert_eq!(ok("-iNfinity"), Some(f64::NEG_INFINITY));
        assert_eq!(ok("1e400"), Some(f64::INFINITY));
        assert_eq!(ok("\u{661}.5"), Some(1.5));
        assert_eq!(ok("1\0"), None);
    }

    #[test]
    fn cpython_edge_cases() {
        let int = |t: &str, b: u32| parse_int_str(t, b).ok();
        assert_eq!(int("0_7", 0), None);
        assert_eq!(int("0b_1", 0), Some(BigInt::from(1)));
        assert_eq!(int("0b__1", 0), None);
        assert_eq!(int(" \u{1c}12", 10), None);
        assert_eq!(int("\u{85}12\u{3000}", 10), Some(BigInt::from(12)));
        assert_eq!(int("0x", 16), None);
        assert_eq!(int("-0X_f", 0), Some(BigInt::from(-15)));
        assert_eq!(int("00", 0), Some(BigInt::zero()));
        assert_eq!(int("\u{663}_\u{661}", 10), Some(BigInt::from(31)));
        let zeros = parse_int_str(&"0".repeat(4301), 0).err().map(|e| e.arg);
        assert!(
            matches!(zeros, Some(Value::Str(ref s)) if s.to_string_lossy().contains("4301 digits"))
        );
        // `%.200R` truncates the repr of the first 200 bytes to 200 chars.
        let msg = parse_int_bytes(&[0; 300], 10).err().map(|e| e.arg);
        assert!(matches!(msg, Some(Value::Str(ref s)) if s.len() == 240));

        let flt = |t: &str| parse_float_str(t).ok();
        assert_eq!(flt("1e5_0"), Some(1e50));
        assert_eq!(flt("1_.5"), None);
        assert_eq!(flt("1._5"), None);
        assert!(flt("-nan").is_some_and(|x| x.is_nan() && x.is_sign_negative()));
        assert_eq!(flt("+InFiNiTy"), Some(f64::INFINITY));
        assert_eq!(flt("infinit"), None);
        assert_eq!(flt(" \u{c}1.5\u{b} "), Some(1.5));
        assert_eq!(flt("1e"), None);
        assert_eq!(flt("1e999999999999999999"), Some(f64::INFINITY));
        assert!(flt("-0").is_some_and(|x| x == 0.0 && x.is_sign_negative()));
        assert_eq!(flt("  "), None);
        assert_eq!(flt("_1"), None);
        assert_eq!(flt(".5e-3_0"), Some(5e-31));
    }

    #[test]
    fn container_reprs() {
        let r = |v: &Value| crate::repr::value_repr(v).ok();
        assert_eq!(
            r(&Value::ByteArray(b"'\"".to_vec())),
            Some("bytearray(b'\\'\"')".to_owned())
        );
        assert_eq!(r(&Value::Bytes(b"'".to_vec())), Some("b\"'\"".to_owned()));
        assert_eq!(
            r(&Value::Tuple(vec![Value::Tuple(vec![])])),
            Some("((),)".to_owned())
        );
        assert_eq!(r(&Value::FrozenSet(vec![])), Some("frozenset()".to_owned()));
        assert_eq!(r(&Value::Float(-f64::NAN)), Some("nan".to_owned()));
    }
}

#[cfg(test)]
mod shortest_tests {
    use super::*;

    /// The Rust-`{:e}` path (with its tie fix-up) that Ryu replaces.
    fn reference(x: f64) -> (Vec<u8>, i64) {
        let sci = shortest_sci(x);
        let (digits, exp) = split_sci(sci.as_str()).expect("Rust {:e} text");
        (digits.to_vec(), exp)
    }

    fn check(x: f64) {
        let x = x.abs();
        if !x.is_finite() || x == 0.0 {
            return;
        }
        let (digits, exp) = shortest_digits(x);
        assert_eq!((digits.to_vec(), exp), reference(x), "x = {x:e}");
    }

    #[test]
    fn ryu_digits_match_reference() {
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..20_000 {
            check(f64::from_bits(next()));
            let r = next();
            let p = i32::try_from((r >> 40) % 40).unwrap_or(0) - 20;
            check((r % 100_000_000) as f64 * 10f64.powi(p));
        }
        for e in 0..2047_u64 {
            for m in [0, 1, 2, 3, (1 << 52) - 1, (1 << 52) - 2, 1 << 51, 12_345] {
                check(f64::from_bits((e << 52) | m));
            }
        }
        for i in -330..310 {
            for lead in ["1", "5", "9.999"] {
                check(format!("{lead}e{i}").parse().unwrap_or(1.0));
            }
        }
        for x in [
            0.3,
            2.0 / 3.0,
            70_368_744_177_664.0 + 0.625,
            5e-324,
            f64::MAX,
            1e23,
            9_007_199_254_740_992.0,
        ] {
            check(x);
        }
    }
}
#[cfg(test)]
mod tie_equivalence {
    /// The previous `u128`-division version, kept to check the fast one.
    fn old_tie(x: f64, n_digits: usize) -> bool {
        let bits = x.to_bits();
        let biased = i32::try_from((bits >> 52) & 0x7ff).unwrap_or(0);
        let fraction = bits & ((1_u64 << 52) - 1);
        let (mut m, mut e) = if biased == 0 {
            (fraction, -1074)
        } else {
            (fraction | (1_u64 << 52), biased - 1075)
        };
        if m == 0 {
            return false;
        }
        let tz = m.trailing_zeros();
        m >>= tz;
        e += i32::try_from(tz).unwrap_or(0);
        let mut v = if e < 0 {
            let k = e.unsigned_abs();
            if k > 26 {
                return false;
            }
            u128::from(m) * 5_u128.pow(k)
        } else {
            if e > 22 {
                return false;
            }
            let mut v = u128::from(m) << e.unsigned_abs();
            while v % 10 == 0 {
                v /= 10;
            }
            v
        };
        if v % 10 != 5 {
            return false;
        }
        let mut count = 0;
        while v > 0 {
            v /= 10;
            count += 1;
        }
        count == n_digits + 1
    }

    #[test]
    fn matches_old() {
        let mut state = 0x1234_5678_9abc_def0_u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let mut xs: Vec<f64> = vec![0.5, 2.5, 0.125, 0.375, 5e-324, 1e22, 5e22, 3.0, 25.0, 0.3];
        for _ in 0..200_000 {
            let r = next();
            // Small exponents (where ties live) and arbitrary bit patterns.
            let mant = r & ((1 << 52) - 1);
            let exp = 1023 - 30 + (r >> 58) % 90;
            let lo_bits = (next() % 53) as u32;
            let m2 = mant & !((1_u64 << lo_bits) - 1);
            xs.push(f64::from_bits((exp << 52) | m2));
            xs.push(f64::from_bits(next() & 0x7fef_ffff_ffff_ffff));
            xs.push((next() % 1_000_000) as f64 / 8.0);
            xs.push((next() % 100_000_000_000) as f64 * 5f64.powi((next() % 20) as i32));
        }
        for x in xs {
            if !x.is_finite() {
                continue;
            }
            for n in 0..40 {
                assert_eq!(super::may_be_decimal_tie(x, n), old_tie(x, n), "{x:e} {n}");
            }
        }
    }
}
