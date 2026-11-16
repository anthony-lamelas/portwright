//! Python `decimal.Decimal` under the default context (task `duration`).
//!
//! The default context: precision 28, `ROUND_HALF_EVEN`, `Emax` 999999, `Emin`
//! -999999, `clamp` 0, traps on `InvalidOperation`, `DivisionByZero` and `Overflow`.
//! Trapped signals become [`DecimalSignal`] errors; the others are silent, as in Python.
//! The algorithms follow CPython's `_pydecimal.py`; string conversion follows the C
//! `_decimal` module (underscores anywhere, Unicode digits and spaces).

use std::cmp::Ordering;
use std::fmt;

use num_bigint::{BigInt, BigUint, Sign};
use num_integer::Integer;
use num_traits::{One, ToPrimitive, Zero};

use crate::error::{DecimalSignal, Error, Result};
use crate::pc_re::unicode;

/// Context precision.
const PREC: i64 = 28;
/// Context `Emax`.
const EMAX: i64 = 999_999;
/// Context `Emin`.
const EMIN: i64 = -999_999;
/// `Emin - prec + 1`: the smallest exponent a result may have.
const ETINY: i64 = EMIN - PREC + 1;
/// `Emax - prec + 1`.
const ETOP: i64 = EMAX - PREC + 1;

/// `MAX_EMAX` of libmpdec (64-bit), the limit of exact string conversion.
const MAX_EMAX: i128 = 999_999_999_999_999_999;
/// `MIN_ETINY` of libmpdec (64-bit).
const MIN_ETINY: i128 = -1_999_999_999_999_999_997;

/// Largest power of ten `int()` will build; beyond it the conversion reports an overflow
/// instead of exhausting memory.
const MAX_INT_EXPONENT: i64 = 100_000_000;

/// A rounding mode, as `decimal.ROUND_*`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rounding {
    /// `ROUND_DOWN`: toward zero.
    Down,
    /// `ROUND_UP`: away from zero.
    Up,
    /// `ROUND_HALF_UP`.
    HalfUp,
    /// `ROUND_HALF_DOWN`.
    HalfDown,
    /// `ROUND_HALF_EVEN` (the context default).
    HalfEven,
    /// `ROUND_CEILING`: toward +infinity.
    Ceiling,
    /// `ROUND_FLOOR`: toward -infinity.
    Floor,
    /// `ROUND_05UP`.
    Up05,
}

/// What a decimal holds besides its sign.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Kind {
    /// `coefficient * 10**exponent`.
    Finite { coeff: BigUint, exp: i64 },
    /// `Infinity`.
    Infinite,
    /// A quiet NaN with its payload (zero means none).
    Nan(BigUint),
    /// A signaling NaN with its payload.
    SNan(BigUint),
}

/// A Python `decimal.Decimal` value: sign, coefficient, exponent, or a special value.
///
/// Equality of the Rust type is representation equality (`1.0 != 1.00`); use
/// [`Decimal::equals`] for Python's `==`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Decimal {
    negative: bool,
    kind: Kind,
}

/// `Decimal.as_tuple()`'s exponent: a number or one of the special markers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Exponent {
    /// A finite value's exponent.
    Finite(i64),
    /// `'F'`: infinity.
    Infinite,
    /// `'n'`: quiet NaN.
    Nan,
    /// `'N'`: signaling NaN.
    SNan,
}

fn invalid() -> Error {
    Error::Decimal(DecimalSignal::InvalidOperation)
}

fn syntax() -> Error {
    Error::Decimal(DecimalSignal::ConversionSyntax)
}

fn pow10(n: u64) -> BigUint {
    num_traits::Pow::pow(BigUint::from(10u8), n)
}

/// Number of decimal digits of `c` (1 for zero).
fn ndigits(c: &BigUint) -> i64 {
    // Fast path for small values.
    if let Some(v) = c.to_u64() {
        let mut n = 1;
        let mut v = v / 10;
        while v > 0 {
            n += 1;
            v /= 10;
        }
        return n;
    }
    i64::try_from(c.to_str_radix(10).len()).unwrap_or(i64::MAX)
}

/// `n` as a non-negative count, for `pow10`. Callers only pass bounded, non-negative values.
fn count(n: i64) -> u64 {
    u64::try_from(n).unwrap_or(0)
}

/// Rounds a positive coefficient written in `digits` to its first `keep` digits.
/// Returns the kept digits and pydecimal's `changed` flag (1 round up, 0 exact, -1 truncated).
fn round_digits(digits: &[u8], keep: usize, negative: bool, mode: Rounding) -> i8 {
    let rest = digits.get(keep..).unwrap_or(&[]);
    let all_zeros = rest.iter().all(|&d| d == b'0');
    let down = if all_zeros { 0 } else { -1 };
    let half_up = || -> i8 {
        if rest.first().is_some_and(|&d| d >= b'5') {
            1
        } else {
            down
        }
    };
    let exact_half = rest.first() == Some(&b'5') && rest.iter().skip(1).all(|&d| d == b'0');
    match mode {
        Rounding::Down => down,
        Rounding::Up => -down,
        Rounding::HalfUp => half_up(),
        Rounding::HalfDown => {
            if exact_half {
                -1
            } else {
                half_up()
            }
        }
        Rounding::HalfEven => {
            let prev_even = keep == 0
                || digits
                    .get(keep - 1)
                    .is_some_and(|d| matches!(d, b'0' | b'2' | b'4' | b'6' | b'8'));
            if exact_half && prev_even {
                -1
            } else {
                half_up()
            }
        }
        Rounding::Ceiling => {
            if negative {
                down
            } else {
                -down
            }
        }
        Rounding::Floor => {
            if negative {
                -down
            } else {
                down
            }
        }
        Rounding::Up05 => {
            let prev = if keep == 0 {
                None
            } else {
                digits.get(keep - 1)
            };
            if prev.is_some_and(|d| !matches!(d, b'0' | b'5')) {
                down
            } else {
                -down
            }
        }
    }
}

fn coeff_from_digits(digits: &[u8]) -> BigUint {
    if digits.is_empty() {
        return BigUint::zero();
    }
    BigUint::parse_bytes(digits, 10).unwrap_or_default()
}

impl Decimal {
    fn finite(negative: bool, coeff: BigUint, exp: i64) -> Self {
        Self {
            negative,
            kind: Kind::Finite { coeff, exp },
        }
    }

    /// `Decimal(0)`.
    pub fn zero() -> Self {
        Self::finite(false, BigUint::zero(), 0)
    }

    /// Infinity with the given sign.
    pub fn infinity(negative: bool) -> Self {
        Self {
            negative,
            kind: Kind::Infinite,
        }
    }

    /// An exact decimal from an integer, as `Decimal(int)`.
    pub fn from_bigint(n: &BigInt) -> Self {
        Self::finite(n.sign() == Sign::Minus, n.magnitude().clone(), 0)
    }

    /// Builds a decimal from `as_tuple()` parts. Digits must be `0..=9`; for NaNs they
    /// are the payload. Like `Decimal((sign, digits, exp))`, nothing is rounded.
    ///
    /// # Errors
    /// `InvalidOperation` if a digit is out of range.
    pub fn from_parts(negative: bool, digits: &[u8], exponent: Exponent) -> Result<Self> {
        if digits.iter().any(|&d| d > 9) {
            return Err(invalid());
        }
        let ascii: Vec<u8> = digits.iter().map(|d| d + b'0').collect();
        let coeff = coeff_from_digits(&ascii);
        let kind = match exponent {
            Exponent::Finite(exp) => Kind::Finite { coeff, exp },
            Exponent::Infinite => Kind::Infinite,
            Exponent::Nan => Kind::Nan(coeff),
            Exponent::SNan => Kind::SNan(coeff),
        };
        Ok(Self { negative, kind })
    }

    /// The value as an `i64` when it is exactly `Decimal(n)` for that int: finite,
    /// exponent 0, and not a negative zero (`Decimal(-0)` is positive).
    pub fn as_exact_int(&self) -> Option<i64> {
        let Kind::Finite { coeff, exp: 0 } = &self.kind else {
            return None;
        };
        let magnitude = i64::try_from(coeff).ok()?;
        match (self.negative, magnitude) {
            (true, 0) => None,
            (true, m) => Some(-m),
            (false, m) => Some(m),
        }
    }

    /// `as_tuple()`: sign, digits (each `0..=9`) and exponent.
    pub fn to_parts(&self) -> (bool, Vec<u8>, Exponent) {
        let digits =
            |c: &BigUint| -> Vec<u8> { c.to_str_radix(10).bytes().map(|b| b - b'0').collect() };
        match &self.kind {
            Kind::Finite { coeff, exp } => (self.negative, digits(coeff), Exponent::Finite(*exp)),
            Kind::Infinite => (self.negative, vec![0], Exponent::Infinite),
            Kind::Nan(p) => (
                self.negative,
                if p.is_zero() { vec![] } else { digits(p) },
                Exponent::Nan,
            ),
            Kind::SNan(p) => (
                self.negative,
                if p.is_zero() { vec![] } else { digits(p) },
                Exponent::SNan,
            ),
        }
    }

    /// `Decimal(text)`, with the C implementation's leniency: surrounding whitespace,
    /// underscores anywhere, and any Unicode decimal digit.
    ///
    /// # Errors
    /// `InvalidOperation([ConversionSyntax])` for bad syntax, `InvalidOperation` when the
    /// exponent is beyond what an exact conversion can hold.
    pub fn parse(text: &[u32]) -> Result<Self> {
        if let Some(quick) = Self::parse_plain(text) {
            return Ok(quick);
        }
        let mut start = 0;
        let mut end = text.len();
        while end > 0 && text.get(end - 1).is_some_and(|&c| unicode::is_space(c)) {
            end -= 1;
        }
        while start < end && text.get(start).is_some_and(|&c| unicode::is_space(c)) {
            start += 1;
        }
        let mut ascii = Vec::with_capacity(end - start);
        for &ch in text.get(start..end).unwrap_or(&[]) {
            if ch == u32::from(b'_') {
                continue;
            }
            if (1..=127).contains(&ch) {
                ascii.push(u8::try_from(ch).map_err(|_| syntax())?);
            } else if unicode::is_space(ch) {
                ascii.push(b' ');
            } else if let Some(d) = unicode::decimal_value(ch) {
                ascii.push(b'0' + u8::try_from(d).map_err(|_| syntax())?);
            } else {
                return Err(syntax());
            }
        }
        Self::parse_ascii(&ascii)
    }

    /// [`Self::parse`] for `[0-9]*(\.[0-9]*)?` with 1 to 19 digits and nothing else
    /// (no sign, spaces, underscores or exponent): the coefficient fits a `u64` and the
    /// exponent is minus the fraction's length. `None` for any other text.
    fn parse_plain(text: &[u32]) -> Option<Self> {
        let mut coeff: u64 = 0;
        let mut digits = 0_usize;
        let mut fraction: Option<i64> = None;
        for &c in text {
            if c == u32::from('.') && fraction.is_none() {
                fraction = Some(0);
                continue;
            }
            let digit = c.checked_sub(u32::from('0')).filter(|d| *d <= 9)?;
            digits += 1;
            if digits > 19 {
                return None;
            }
            coeff = coeff * 10 + u64::from(digit);
            if let Some(f) = fraction.as_mut() {
                *f += 1;
            }
        }
        if digits == 0 {
            return None;
        }
        Some(Self::finite(
            false,
            BigUint::from(coeff),
            -fraction.unwrap_or(0),
        ))
    }

    /// `Decimal(str)` for an ASCII string, as libmpdec's `mpd_qset_string` reads it.
    fn parse_ascii(s: &[u8]) -> Result<Self> {
        let (negative, rest) = match s.first() {
            Some(b'-') => (true, s.get(1..).unwrap_or(&[])),
            Some(b'+') => (false, s.get(1..).unwrap_or(&[])),
            _ => (false, s),
        };
        let lower: Vec<u8> = rest.iter().map(u8::to_ascii_lowercase).collect();
        if lower == b"inf" || lower == b"infinity" {
            return Ok(Self::infinity(negative));
        }
        for (prefix, signaling) in [(&b"snan"[..], true), (&b"nan"[..], false)] {
            if let Some(payload) = lower.strip_prefix(prefix) {
                if !payload.iter().all(u8::is_ascii_digit) {
                    return Err(syntax());
                }
                let coeff = coeff_from_digits(payload);
                let kind = if signaling {
                    Kind::SNan(coeff)
                } else {
                    Kind::Nan(coeff)
                };
                return Ok(Self { negative, kind });
            }
        }
        // digits [. digits] [e [sign] digits]
        let (mantissa, exponent) = match rest.iter().position(|&b| b == b'e' || b == b'E') {
            Some(i) => (
                rest.get(..i).unwrap_or(&[]),
                Some(rest.get(i + 1..).unwrap_or(&[])),
            ),
            None => (rest, None),
        };
        let (int_part, frac_part) = match mantissa.iter().position(|&b| b == b'.') {
            Some(i) => (
                mantissa.get(..i).unwrap_or(&[]),
                mantissa.get(i + 1..).unwrap_or(&[]),
            ),
            None => (mantissa, &[][..]),
        };
        if int_part.is_empty() && frac_part.is_empty() {
            return Err(syntax());
        }
        if !int_part.iter().chain(frac_part).all(u8::is_ascii_digit) {
            return Err(syntax());
        }
        let mut exp: i128 = 0;
        if let Some(e) = exponent {
            let (eneg, edigits) = match e.first() {
                Some(b'-') => (true, e.get(1..).unwrap_or(&[])),
                Some(b'+') => (false, e.get(1..).unwrap_or(&[])),
                _ => (false, e),
            };
            if edigits.is_empty() || !edigits.iter().all(u8::is_ascii_digit) {
                return Err(syntax());
            }
            // Saturate far beyond any valid exponent; the range check below rejects it.
            const SATURATE: i128 = 1 << 100;
            for &d in edigits {
                exp = (exp * 10 + i128::from(d - b'0')).min(SATURATE);
            }
            if eneg {
                exp = -exp;
            }
        }
        exp -= i128::try_from(frac_part.len()).unwrap_or(i128::MAX);
        let mut all = Vec::with_capacity(int_part.len() + frac_part.len());
        all.extend_from_slice(int_part);
        all.extend_from_slice(frac_part);
        let coeff = coeff_from_digits(&all);
        // Exact conversion under libmpdec's maximum context.
        let in_range = if coeff.is_zero() {
            (MIN_ETINY..=MAX_EMAX).contains(&exp)
        } else {
            exp >= MIN_ETINY && exp + i128::from(ndigits(&coeff)) - 1 <= MAX_EMAX
        };
        if !in_range {
            return Err(invalid());
        }
        let exp = i64::try_from(exp).map_err(|_| invalid())?;
        Ok(Self::finite(negative, coeff, exp))
    }

    /// Whether this is a NaN, quiet or signaling.
    pub fn is_nan(&self) -> bool {
        matches!(self.kind, Kind::Nan(_) | Kind::SNan(_))
    }

    /// Whether this is a signaling NaN.
    pub fn is_snan(&self) -> bool {
        matches!(self.kind, Kind::SNan(_))
    }

    /// Whether this is an infinity.
    pub fn is_infinite(&self) -> bool {
        matches!(self.kind, Kind::Infinite)
    }

    /// Whether the sign bit is set (`-0`, `-NaN` included).
    pub fn is_signed(&self) -> bool {
        self.negative
    }

    /// `bool(d)`: NaNs and infinities are true.
    pub fn is_true(&self) -> bool {
        match &self.kind {
            Kind::Finite { coeff, .. } => !coeff.is_zero(),
            _ => true,
        }
    }

    fn is_zero_finite(&self) -> bool {
        matches!(&self.kind, Kind::Finite { coeff, .. } if coeff.is_zero())
    }

    /// `adjusted()`: the exponent of the most significant digit (0 for specials).
    pub fn adjusted(&self) -> i64 {
        match &self.kind {
            Kind::Finite { coeff, exp } => exp + ndigits(coeff) - 1,
            _ => 0,
        }
    }

    fn copy_negate(&self) -> Self {
        Self {
            negative: !self.negative,
            kind: self.kind.clone(),
        }
    }

    fn copy_abs(&self) -> Self {
        Self {
            negative: false,
            kind: self.kind.clone(),
        }
    }

    /// `_check_nans`: a sNaN operand raises; a quiet NaN is the result.
    fn check_nans(&self, other: Option<&Self>) -> Result<Option<Self>> {
        if self.is_snan() || other.is_some_and(Self::is_snan) {
            return Err(invalid());
        }
        if self.is_nan() {
            return Ok(Some(self.fix_nan()));
        }
        if let Some(o) = other.filter(|o| o.is_nan()) {
            return Ok(Some(o.fix_nan()));
        }
        Ok(None)
    }

    /// `_fix_nan`: keeps the payload's last `prec` digits.
    fn fix_nan(&self) -> Self {
        let payload = match &self.kind {
            Kind::Nan(p) | Kind::SNan(p) => p,
            _ => return self.clone(),
        };
        if ndigits(payload) <= PREC {
            return self.clone();
        }
        let truncated = payload % pow10(count(PREC));
        let kind = match &self.kind {
            Kind::SNan(_) => Kind::SNan(truncated),
            _ => Kind::Nan(truncated),
        };
        Self {
            negative: self.negative,
            kind,
        }
    }

    /// `_fix`: rounds to the context precision and checks the exponent range.
    fn fix(self) -> Result<Self> {
        let (coeff, exp) = match &self.kind {
            Kind::Finite { coeff, exp } => (coeff, *exp),
            Kind::Nan(_) | Kind::SNan(_) => return Ok(self.fix_nan()),
            Kind::Infinite => return Ok(self),
        };
        if coeff.is_zero() {
            let new_exp = exp.clamp(ETINY, EMAX);
            return Ok(Self::finite(self.negative, BigUint::zero(), new_exp));
        }
        let len = ndigits(coeff);
        let mut exp_min = len + exp - PREC;
        if exp_min > ETOP {
            return Err(Error::Decimal(DecimalSignal::Overflow));
        }
        if exp_min < ETINY {
            exp_min = ETINY;
        }
        if exp >= exp_min {
            return Ok(self);
        }
        let mut keep = len + exp - exp_min;
        let mut digits = coeff.to_str_radix(10).into_bytes();
        if keep < 0 {
            digits = b"1".to_vec();
            keep = 0;
        }
        let keep = usize::try_from(keep).unwrap_or(0);
        let changed = round_digits(&digits, keep, self.negative, Rounding::HalfEven);
        let mut new = coeff_from_digits(digits.get(..keep).unwrap_or(&[]));
        if changed > 0 {
            new += 1u8;
            if ndigits(&new) > PREC {
                new /= 10u8;
                exp_min += 1;
            }
        }
        if exp_min > ETOP {
            return Err(Error::Decimal(DecimalSignal::Overflow));
        }
        Ok(Self::finite(self.negative, new, exp_min))
    }

    /// `_rescale`: sets the exponent to `exp`, rounding with `mode`; quiet.
    fn rescale(&self, exp: i64, mode: Rounding) -> Self {
        let (coeff, self_exp) = match &self.kind {
            Kind::Finite { coeff, exp } => (coeff, *exp),
            _ => return self.clone(),
        };
        if coeff.is_zero() {
            return Self::finite(self.negative, BigUint::zero(), exp);
        }
        if self_exp >= exp {
            return Self::finite(self.negative, coeff * pow10(count(self_exp - exp)), exp);
        }
        let mut keep = ndigits(coeff) + self_exp - exp;
        let mut digits = coeff.to_str_radix(10).into_bytes();
        if keep < 0 {
            digits = b"1".to_vec();
            keep = 0;
        }
        let keep = usize::try_from(keep).unwrap_or(0);
        let changed = round_digits(&digits, keep, self.negative, mode);
        let mut new = coeff_from_digits(digits.get(..keep).unwrap_or(&[]));
        if changed == 1 {
            new += 1u8;
        }
        Self::finite(self.negative, new, exp)
    }

    /// `-d`: rounds in context; `-Decimal(0)` is `Decimal(0)`.
    ///
    /// # Errors
    /// `InvalidOperation` for a sNaN, `Overflow` past `Emax`.
    pub fn neg(&self) -> Result<Self> {
        if let Some(nan) = self.check_nans(None)? {
            return Ok(nan);
        }
        if self.is_zero_finite() {
            self.copy_abs().fix()
        } else {
            self.copy_negate().fix()
        }
    }

    /// `+d`: rounds in context; `+Decimal(-0)` is `Decimal(0)`.
    ///
    /// # Errors
    /// As [`Decimal::neg`].
    pub fn plus(&self) -> Result<Self> {
        if let Some(nan) = self.check_nans(None)? {
            return Ok(nan);
        }
        if self.is_zero_finite() {
            self.copy_abs().fix()
        } else {
            self.clone().fix()
        }
    }

    /// `abs(d)`: rounds in context.
    ///
    /// # Errors
    /// As [`Decimal::neg`].
    pub fn abs(&self) -> Result<Self> {
        if let Some(nan) = self.check_nans(None)? {
            return Ok(nan);
        }
        if self.negative {
            self.neg()
        } else {
            self.plus()
        }
    }

    /// `self + other`.
    ///
    /// # Errors
    /// `InvalidOperation` for a sNaN or `-Inf + Inf`, `Overflow` past `Emax`.
    pub fn add(&self, other: &Self) -> Result<Self> {
        if let Some(nan) = self.check_nans(Some(other))? {
            return Ok(nan);
        }
        if self.is_infinite() {
            if other.is_infinite() && self.negative != other.negative {
                return Err(invalid());
            }
            return Ok(self.clone());
        }
        if other.is_infinite() {
            return Ok(other.clone());
        }
        let (Kind::Finite { coeff: c1, exp: e1 }, Kind::Finite { coeff: c2, exp: e2 }) =
            (&self.kind, &other.kind)
        else {
            return Err(invalid());
        };
        let exp = (*e1).min(*e2);
        if c1.is_zero() && c2.is_zero() {
            let negative = self.negative && other.negative;
            return Self::finite(negative, BigUint::zero(), exp).fix();
        }
        if c1.is_zero() {
            let exp = exp.max(e2 - PREC - 1);
            return other.rescale(exp, Rounding::HalfEven).fix();
        }
        if c2.is_zero() {
            let exp = exp.max(e1 - PREC - 1);
            return self.rescale(exp, Rounding::HalfEven).fix();
        }
        // _normalize: align exponents, replacing a negligible operand by 10**exp.
        let mut op1 = (self.negative, c1.clone(), *e1);
        let mut op2 = (other.negative, c2.clone(), *e2);
        {
            let (tmp, small) = if op1.2 < op2.2 {
                (&mut op2, &mut op1)
            } else {
                (&mut op1, &mut op2)
            };
            let tmp_len = ndigits(&tmp.1);
            let small_len = ndigits(&small.1);
            let floor_exp = tmp.2 + (-1i64).min(tmp_len - PREC - 2);
            if small_len + small.2 - 1 < floor_exp {
                small.1 = BigUint::one();
                small.2 = floor_exp;
            }
            tmp.1 = &tmp.1 * pow10(count(tmp.2 - small.2));
            tmp.2 = small.2;
        }
        let result_exp = op1.2;
        let (negative, coeff) = if op1.0 == op2.0 {
            (op1.0, op1.1 + op2.1)
        } else {
            match op1.1.cmp(&op2.1) {
                Ordering::Equal => {
                    return Self::finite(false, BigUint::zero(), exp).fix();
                }
                Ordering::Greater => (op1.0, op1.1 - op2.1),
                Ordering::Less => (op2.0, op2.1 - op1.1),
            }
        };
        Self::finite(negative, coeff, result_exp).fix()
    }

    /// `self - other`.
    ///
    /// # Errors
    /// As [`Decimal::add`].
    pub fn sub(&self, other: &Self) -> Result<Self> {
        if let Some(nan) = self.check_nans(Some(other))? {
            return Ok(nan);
        }
        self.add(&other.copy_negate())
    }

    /// `self * other`.
    ///
    /// # Errors
    /// `InvalidOperation` for a sNaN or `Inf * 0`, `Overflow` past `Emax`.
    pub fn mul(&self, other: &Self) -> Result<Self> {
        let negative = self.negative != other.negative;
        if let Some(nan) = self.check_nans(Some(other))? {
            return Ok(nan);
        }
        if self.is_infinite() {
            if !other.is_true() {
                return Err(invalid());
            }
            return Ok(Self::infinity(negative));
        }
        if other.is_infinite() {
            if !self.is_true() {
                return Err(invalid());
            }
            return Ok(Self::infinity(negative));
        }
        let (Kind::Finite { coeff: c1, exp: e1 }, Kind::Finite { coeff: c2, exp: e2 }) =
            (&self.kind, &other.kind)
        else {
            return Err(invalid());
        };
        Self::finite(negative, c1 * c2, e1 + e2).fix()
    }

    /// `self / other`.
    ///
    /// # Errors
    /// `InvalidOperation([DivisionUndefined])` for `0 / 0`, `DivisionByZero` for `x / 0`,
    /// `InvalidOperation` for a sNaN or `Inf / Inf`, `Overflow` past `Emax`.
    pub fn div(&self, other: &Self) -> Result<Self> {
        let negative = self.negative != other.negative;
        if let Some(nan) = self.check_nans(Some(other))? {
            return Ok(nan);
        }
        if self.is_infinite() && other.is_infinite() {
            return Err(invalid());
        }
        if self.is_infinite() {
            return Ok(Self::infinity(negative));
        }
        if other.is_infinite() {
            return Ok(Self::finite(negative, BigUint::zero(), ETINY));
        }
        let (Kind::Finite { coeff: c1, exp: e1 }, Kind::Finite { coeff: c2, exp: e2 }) =
            (&self.kind, &other.kind)
        else {
            return Err(invalid());
        };
        if c2.is_zero() {
            if c1.is_zero() {
                return Err(Error::Decimal(DecimalSignal::DivisionUndefined));
            }
            return Err(Error::Decimal(DecimalSignal::DivisionByZero));
        }
        let (coeff, exp) = if c1.is_zero() {
            (BigUint::zero(), e1 - e2)
        } else {
            let shift = ndigits(c2) - ndigits(c1) + PREC + 1;
            let mut exp = e1 - e2 - shift;
            let (mut q, r) = if shift >= 0 {
                (c1 * pow10(count(shift))).div_rem(c2)
            } else {
                c1.div_rem(&(c2 * pow10(count(-shift))))
            };
            if r.is_zero() {
                let ideal = e1 - e2;
                let ten = BigUint::from(10u8);
                while exp < ideal && (&q % &ten).is_zero() {
                    q /= 10u8;
                    exp += 1;
                }
            } else if (&q % BigUint::from(5u8)).is_zero() {
                q += 1u8;
            }
            (q, exp)
        };
        Self::finite(negative, coeff, exp).fix()
    }

    /// `self % other`: the remainder of truncating division, with the dividend's sign.
    ///
    /// # Errors
    /// `InvalidOperation` for a sNaN, `Inf % x`, `x % 0` or a quotient over 28 digits
    /// (Python names the last `DivisionImpossible`), `DivisionUndefined` for `0 % 0`.
    pub fn rem(&self, other: &Self) -> Result<Self> {
        if let Some(nan) = self.check_nans(Some(other))? {
            return Ok(nan);
        }
        if self.is_infinite() {
            return Err(invalid());
        }
        if !other.is_true() {
            if self.is_true() {
                return Err(invalid());
            }
            return Err(Error::Decimal(DecimalSignal::DivisionUndefined));
        }
        let Kind::Finite { coeff: c1, exp: e1 } = &self.kind else {
            return Err(invalid());
        };
        // _divide
        let (ideal_exp, other_inf) = match &other.kind {
            Kind::Finite { exp, .. } => ((*e1).min(*exp), false),
            _ => (*e1, true),
        };
        let expdiff = self.adjusted() - other.adjusted();
        if c1.is_zero() || other_inf || expdiff <= -2 {
            return self.rescale(ideal_exp, Rounding::HalfEven).fix();
        }
        if expdiff <= PREC {
            if let Kind::Finite { coeff: c2, exp: e2 } = &other.kind {
                let (a, b) = if e1 >= e2 {
                    (c1 * pow10(count(e1 - e2)), c2.clone())
                } else {
                    (c1.clone(), c2 * pow10(count(e2 - e1)))
                };
                let (q, r) = a.div_rem(&b);
                if q < pow10(count(PREC)) {
                    return Self::finite(self.negative, r, ideal_exp).fix();
                }
            }
        }
        // DivisionImpossible, a subclass of InvalidOperation.
        Err(invalid())
    }

    /// `to_integral_value(rounding)`: rounds to an integer, without context precision.
    ///
    /// # Errors
    /// `InvalidOperation` for a sNaN.
    pub fn to_integral(&self, mode: Rounding) -> Result<Self> {
        if let Some(nan) = self.check_nans(None)? {
            return Ok(nan);
        }
        match &self.kind {
            Kind::Finite { exp, .. } if *exp < 0 => Ok(self.rescale(0, mode)),
            _ => Ok(self.clone()),
        }
    }

    /// `quantize(exp, rounding)`: rescales to `exp`'s exponent.
    ///
    /// # Errors
    /// `InvalidOperation` for a sNaN, one infinite operand, a target exponent out of
    /// range, or a result needing more than 28 digits; `Overflow` is not possible.
    pub fn quantize(&self, exp: &Self, mode: Rounding) -> Result<Self> {
        if let Some(nan) = self.check_nans(Some(exp))? {
            return Ok(nan);
        }
        if exp.is_infinite() || self.is_infinite() {
            if exp.is_infinite() && self.is_infinite() {
                return Ok(self.clone());
            }
            return Err(invalid());
        }
        let Kind::Finite { exp: target, .. } = exp.kind else {
            return Err(invalid());
        };
        if !(ETINY..=EMAX).contains(&target) {
            return Err(invalid());
        }
        if self.is_zero_finite() {
            return Self::finite(self.negative, BigUint::zero(), target).fix();
        }
        let adjusted = self.adjusted();
        if adjusted > EMAX || adjusted - target + 1 > PREC {
            return Err(invalid());
        }
        let ans = self.rescale(target, mode);
        if ans.adjusted() > EMAX {
            return Err(invalid());
        }
        if let Kind::Finite { coeff, .. } = &ans.kind {
            if ndigits(coeff) > PREC {
                return Err(invalid());
            }
        }
        ans.fix()
    }

    /// `_cmp` for two non-NaN values.
    fn cmp_numbers(&self, other: &Self) -> Ordering {
        let inf_rank = |d: &Self| -> i8 {
            match (&d.kind, d.negative) {
                (Kind::Infinite, false) => 1,
                (Kind::Infinite, true) => -1,
                _ => 0,
            }
        };
        if self.is_infinite() || other.is_infinite() {
            return inf_rank(self).cmp(&inf_rank(other));
        }
        let (Kind::Finite { coeff: c1, exp: e1 }, Kind::Finite { coeff: c2, exp: e2 }) =
            (&self.kind, &other.kind)
        else {
            return Ordering::Equal;
        };
        let signed = |negative: bool, o: Ordering| if negative { o.reverse() } else { o };
        match (c1.is_zero(), c2.is_zero()) {
            (true, true) => return Ordering::Equal,
            (true, false) => return signed(other.negative, Ordering::Less),
            (false, true) => return signed(self.negative, Ordering::Greater),
            (false, false) => {}
        }
        if self.negative != other.negative {
            return if self.negative {
                Ordering::Less
            } else {
                Ordering::Greater
            };
        }
        let (a1, a2) = (self.adjusted(), other.adjusted());
        let magnitude = if a1 == a2 {
            // Same number of integer digits, so the padding stays within the operands' lengths.
            if e1 >= e2 {
                (c1 * pow10(count(e1 - e2))).cmp(c2)
            } else {
                c1.cmp(&(c2 * pow10(count(e2 - e1))))
            }
        } else {
            a1.cmp(&a2)
        };
        signed(self.negative, magnitude)
    }

    /// Python `self == other`: quiet NaNs compare unequal.
    ///
    /// # Errors
    /// `InvalidOperation` if either operand is a sNaN.
    pub fn equals(&self, other: &Self) -> Result<bool> {
        if self.check_nans(Some(other))?.is_some() {
            return Ok(false);
        }
        Ok(self.cmp_numbers(other) == Ordering::Equal)
    }

    /// The ordering used by `<`, `<=`, `>`, `>=`.
    ///
    /// # Errors
    /// `InvalidOperation` if either operand is a NaN (quiet or signaling).
    pub fn compare(&self, other: &Self) -> Result<Ordering> {
        if self.is_nan() || other.is_nan() {
            return Err(invalid());
        }
        Ok(self.cmp_numbers(other))
    }

    /// `int(d)`: truncates toward zero.
    ///
    /// # Errors
    /// `ValueError` for NaNs, `OverflowError` for infinities (C `_decimal` messages).
    pub fn to_bigint(&self) -> Result<BigInt> {
        let (coeff, exp) = match &self.kind {
            Kind::Finite { coeff, exp } => (coeff, *exp),
            Kind::Nan(_) | Kind::SNan(_) => {
                return Err(Error::value("cannot convert NaN to integer"))
            }
            Kind::Infinite => return Err(Error::overflow("cannot convert Infinity to integer")),
        };
        let magnitude = if coeff.is_zero() {
            BigUint::zero()
        } else if exp >= 0 {
            if exp > MAX_INT_EXPONENT {
                return Err(Error::overflow("cannot convert Infinity to integer"));
            }
            coeff * pow10(count(exp))
        } else if -exp >= ndigits(coeff) {
            BigUint::zero()
        } else {
            coeff / pow10(count(-exp))
        };
        let sign = if self.negative {
            Sign::Minus
        } else {
            Sign::Plus
        };
        Ok(BigInt::from_biguint(sign, magnitude))
    }

    /// `float(d)`.
    ///
    /// # Errors
    /// `ValueError` for a signaling NaN.
    pub fn to_f64(&self) -> Result<f64> {
        match &self.kind {
            Kind::SNan(_) => Err(Error::value("cannot convert signaling NaN to float")),
            Kind::Nan(_) => Ok(if self.negative { -f64::NAN } else { f64::NAN }),
            Kind::Infinite => Ok(if self.negative {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            }),
            Kind::Finite { coeff, exp } => {
                let sign = if self.negative { "-" } else { "" };
                let text = format!("{sign}{coeff}e{exp}");
                text.parse::<f64>()
                    .map_err(|_| Error::value("could not convert string to float"))
            }
        }
    }

    /// Whether this is a finite integer value (`d == d.to_integral()`).
    pub fn is_integer(&self) -> bool {
        match &self.kind {
            Kind::Finite { coeff, exp } => {
                *exp >= 0 || coeff.is_zero() || {
                    let n = -exp;
                    n < ndigits(coeff) && (coeff % pow10(count(n))).is_zero()
                }
            }
            _ => false,
        }
    }
}

/// `str(d)`: scientific notation as Python prints it.
impl fmt::Display for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.negative { "-" } else { "" };
        let (coeff, exp) = match &self.kind {
            Kind::Infinite => return write!(f, "{sign}Infinity"),
            Kind::Nan(p) | Kind::SNan(p) => {
                let name = if self.is_snan() { "sNaN" } else { "NaN" };
                return if p.is_zero() {
                    write!(f, "{sign}{name}")
                } else {
                    write!(f, "{sign}{name}{p}")
                };
            }
            Kind::Finite { coeff, exp } => (coeff.to_str_radix(10), *exp),
        };
        let len = i64::try_from(coeff.len()).unwrap_or(i64::MAX);
        let leftdigits = exp + len;
        let dotplace = if exp <= 0 && leftdigits > -6 {
            leftdigits
        } else {
            1
        };
        f.write_str(sign)?;
        if dotplace <= 0 {
            f.write_str("0.")?;
            for _ in 0..(-dotplace) {
                f.write_str("0")?;
            }
            f.write_str(&coeff)?;
        } else if dotplace >= len {
            f.write_str(&coeff)?;
            for _ in 0..(dotplace - len) {
                f.write_str("0")?;
            }
        } else {
            let split = usize::try_from(dotplace).unwrap_or(0);
            let (int_part, frac_part) = coeff.split_at(split.min(coeff.len()));
            write!(f, "{int_part}.{frac_part}")?;
        }
        if leftdigits != dotplace {
            write!(f, "E{:+}", leftdigits - dotplace)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Decimal {
        match Decimal::parse(&s.chars().map(u32::from).collect::<Vec<_>>()) {
            Ok(v) => v,
            Err(e) => unreachable!("parse {s}: {e}"),
        }
    }

    #[test]
    fn parse_plain_is_parse_ascii() {
        let mut texts: Vec<String> = [
            "0",
            "00",
            "7",
            "1.5",
            ".5",
            "5.",
            ".",
            "",
            "1..2",
            "12.34.5",
            "0.000",
            "1234567890123456789",
            "12345678901234567890",
            "1.234567890123456789",
            "0.1234567890123456789",
            "9999999999999999999",
            "99999999999999999999.",
            "1,5",
            "-1",
            "+1",
            "1e5",
            " 1",
            "1_0",
            "١",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
        for a in 0..25 {
            for b in 0..25 {
                texts.push(format!("{}.{}", "9".repeat(a), "8".repeat(b)));
                texts.push(format!("{}{}", "7".repeat(a), "0".repeat(b)));
            }
        }
        for text in texts {
            let ascii_ok = text.is_ascii();
            let quick = Decimal::parse_plain(&text.chars().map(u32::from).collect::<Vec<_>>());
            if let Some(quick) = quick {
                assert!(ascii_ok, "{text}");
                let slow = Decimal::parse_ascii(text.as_bytes());
                assert_eq!(Some(quick), slow.ok(), "{text}");
            }
        }
    }

    #[test]
    fn as_exact_int_is_from_bigint() {
        for text in [
            "0",
            "7",
            "-7",
            "9223372036854775807",
            "-9223372036854775807",
        ] {
            let value = d(text);
            let n = value.as_exact_int();
            assert!(n.is_some(), "{text}");
            let back = Decimal::from_bigint(&BigInt::from(n.unwrap_or_default()));
            assert_eq!(back, value, "{text}");
        }
        for text in [
            "-0",
            "1.0",
            "1E+1",
            "9223372036854775808",
            "nan",
            "inf",
            "0E-3",
        ] {
            assert_eq!(d(text).as_exact_int(), None, "{text}");
        }
    }

    #[test]
    fn parse_and_str() {
        for (input, out) in [
            ("1", "1"),
            ("-0", "-0"),
            ("1E+1", "1E+1"),
            ("0.000001", "0.000001"),
            ("0.0000001", "1E-7"),
            ("1_000", "1000"),
            (" 12.50 ", "12.50"),
            ("inf", "Infinity"),
            ("-nan12", "-NaN12"),
            ("snan", "sNaN"),
            ("1e20", "1E+20"),
            (".5", "0.5"),
        ] {
            assert_eq!(d(input).to_string(), out, "input {input}");
        }
        assert!(Decimal::parse(&[u32::from(b'.')]).is_err());
    }

    #[test]
    fn arithmetic() -> Result<()> {
        assert_eq!(d("1.1").add(&d("2.20"))?.to_string(), "3.30");
        assert_eq!(d("1").sub(&d("1"))?.to_string(), "0");
        assert_eq!(d("-0").add(&d("-0"))?.to_string(), "-0");
        assert_eq!(
            d("1").div(&d("3"))?.to_string(),
            "0.3333333333333333333333333333"
        );
        assert_eq!(d("12").div(&d("4"))?.to_string(), "3");
        assert_eq!(d("-7").rem(&d("4"))?.to_string(), "-3");
        assert_eq!(d("-0.5").to_integral(Rounding::Floor)?.to_string(), "-1");
        assert_eq!(d("0").neg()?.to_string(), "0");
        assert_eq!(
            d("1111111111111111111111111111111").neg()?.to_string(),
            "-1.111111111111111111111111111E+30"
        );
        assert!(d("0").div(&d("0")).is_err());
        assert!(d("NaN").compare(&d("1")).is_err());
        Ok(())
    }
}
