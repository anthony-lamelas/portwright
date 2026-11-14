//! Integer arithmetic with Python's semantics: correctly rounded true division and
//! round-half-even integer division.

use num_bigint::{BigInt, Sign};
use num_integer::Integer;
use num_traits::{One, Signed, ToPrimitive, Zero};

use crate::error::{PyError, PyResult};

/// `ZeroDivisionError` raised by integer `//`, `%` and `divmod`.
pub fn int_zero_division() -> PyError {
    PyError::zero_division_error("integer division or modulo by zero")
}

/// Largest magnitude an `f64` holds exactly as an integer: `2**53`.
const EXACT_F64_LIMIT: u64 = 1 << 53;

/// Python's `a / b` for two ints: the correctly rounded (half-even) quotient.
///
/// # Errors
/// `ZeroDivisionError("division by zero")` when `b` is zero.
pub fn true_divide(a: &BigInt, b: &BigInt) -> PyResult<f64> {
    if b.is_zero() {
        return Err(PyError::zero_division_error("division by zero"));
    }
    let negative = (a.sign() == Sign::Minus) != (b.sign() == Sign::Minus);
    if a.is_zero() {
        return Ok(if negative { -0.0 } else { 0.0 });
    }
    let (ua, ub) = (a.magnitude(), b.magnitude());
    let small = |x: &num_bigint::BigUint| x.to_u64().filter(|&v| v <= EXACT_F64_LIMIT);
    let magnitude = if let (Some(x), Some(y)) = (small(ua), small(ub)) {
        // Both convert exactly, so the single IEEE division rounds correctly.
        u64_to_f64(x) / u64_to_f64(y)
    } else {
        // Scale so the integer quotient has 55 or 56 bits, fold any remainder into a
        // sticky low bit, and let the u64 -> f64 conversion round half to even once.
        let bits_a = i64::try_from(ua.bits()).unwrap_or(i64::MAX);
        let bits_b = i64::try_from(ub.bits()).unwrap_or(i64::MAX);
        let shift = bits_a - bits_b - 55;
        let (num, den) = if shift >= 0 {
            (ua.clone(), ub << shift.unsigned_abs())
        } else {
            (ua << shift.unsigned_abs(), ub.clone())
        };
        let (q, r) = num.div_rem(&den);
        let mut q = q.to_u64().unwrap_or(u64::MAX);
        if !r.is_zero() {
            q |= 1;
        }
        scale_by_power_of_two(u64_to_f64(q), shift)
    };
    Ok(if negative { -magnitude } else { magnitude })
}

#[expect(
    clippy::cast_precision_loss,
    reason = "u64 -> f64 rounds half to even, which is the rounding wanted here"
)]
fn u64_to_f64(x: u64) -> f64 {
    x as f64
}

/// `x * 2**exp`, exact while the result stays a normal float.
fn scale_by_power_of_two(mut x: f64, mut exp: i64) -> f64 {
    const STEP: i64 = 512;
    let two_pow = |e: i64| -> f64 {
        // e is in [-512, 512], so the biased exponent is a valid normal exponent.
        let biased = u64::try_from(e + 1023).unwrap_or(1023);
        f64::from_bits(biased << 52)
    };
    while exp > STEP {
        x *= two_pow(STEP);
        exp -= STEP;
    }
    while exp < -STEP {
        x *= two_pow(-STEP);
        exp += STEP;
    }
    x * two_pow(exp)
}

/// CPython's `divide_nearest`: `a / b` rounded to the nearest int, ties to even.
///
/// # Errors
/// `ZeroDivisionError("integer division or modulo by zero")` when `b` is zero.
pub fn divide_nearest(a: &BigInt, b: &BigInt) -> PyResult<BigInt> {
    if b.is_zero() {
        return Err(int_zero_division());
    }
    let (mut q, r) = a.div_mod_floor(b);
    let twice: BigInt = r.abs() << 1usize;
    let bound = b.abs();
    if twice > bound || (twice == bound && q.is_odd()) {
        q += BigInt::one();
    }
    Ok(q)
}

/// Python's `a // b`.
///
/// # Errors
/// `ZeroDivisionError("integer division or modulo by zero")` when `b` is zero.
pub fn floor_divide(a: &BigInt, b: &BigInt) -> PyResult<BigInt> {
    if b.is_zero() {
        return Err(int_zero_division());
    }
    Ok(a.div_floor(b))
}

/// `float.as_integer_ratio()`: `(numerator, denominator)` with a positive power-of-two
/// denominator, in lowest terms.
///
/// # Errors
/// `OverflowError` for infinities and `ValueError` for NaN, with CPython's messages.
pub fn as_integer_ratio(f: f64) -> PyResult<(BigInt, BigInt)> {
    if f.is_nan() {
        return Err(PyError::value_error("cannot convert NaN to integer ratio"));
    }
    if f.is_infinite() {
        return Err(PyError::overflow_error(
            "cannot convert Infinity to integer ratio",
        ));
    }
    let bits = f.to_bits();
    let exp_bits = i64::try_from((bits >> 52) & 0x7FF).unwrap_or(0);
    let frac = bits & ((1u64 << 52) - 1);
    let (mut mantissa, mut exp) = if exp_bits == 0 {
        (frac, -1074)
    } else {
        (frac | (1u64 << 52), exp_bits - 1075)
    };
    if mantissa == 0 {
        return Ok((BigInt::zero(), BigInt::one()));
    }
    let tz = i64::from(mantissa.trailing_zeros());
    if exp < 0 {
        let drop = tz.min(-exp);
        mantissa >>= drop;
        exp += drop;
    }
    let mut num = BigInt::from(mantissa);
    if f.is_sign_negative() {
        num = -num;
    }
    if exp >= 0 {
        Ok((num << exp.unsigned_abs(), BigInt::one()))
    } else {
        Ok((num, BigInt::one() << exp.unsigned_abs()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn true_divide_is_correctly_rounded() {
        let a = BigInt::from(86_399_999_999_999_999_i64);
        let b = BigInt::from(1_000_000);
        assert_eq!(true_divide(&a, &b), Ok(86_400_000_000.0));
        assert_eq!(
            true_divide(&BigInt::from(0), &BigInt::from(-5)).map(f64::is_sign_negative),
            Ok(true)
        );
        assert_eq!(
            true_divide(&BigInt::from(1), &BigInt::from(3)),
            Ok(1.0 / 3.0)
        );
    }

    #[test]
    fn divide_nearest_ties_to_even() {
        let d = |a: i64, b: i64| divide_nearest(&BigInt::from(a), &BigInt::from(b));
        assert_eq!(d(5, 2), Ok(BigInt::from(2)));
        assert_eq!(d(7, 2), Ok(BigInt::from(4)));
        assert_eq!(d(-5, 2), Ok(BigInt::from(-2)));
        assert_eq!(d(5, -2), Ok(BigInt::from(-2)));
        assert_eq!(d(-7, -2), Ok(BigInt::from(4)));
    }

    #[test]
    fn integer_ratio() {
        assert_eq!(
            as_integer_ratio(0.75),
            Ok((BigInt::from(3), BigInt::from(4)))
        );
        assert_eq!(
            as_integer_ratio(-2.0),
            Ok((BigInt::from(-2), BigInt::from(1)))
        );
    }
}
