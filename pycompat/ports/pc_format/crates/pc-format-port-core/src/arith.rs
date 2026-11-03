//! Python's numeric operators: `//`, `%`, `divmod`, `/` and `round`.
//!
//! Ints and bools follow CPython's `long` semantics (floor division, correctly
//! rounded true division); floats follow `float_divmod` / `double_round`.

use num_bigint::{BigInt, Sign};
use num_integer::Integer;
use num_traits::{One, Signed, ToPrimitive, Zero};

use crate::numbers::decimal::scaled_round_half_even;
use crate::numbers::int_to_float;
use crate::value::{check_builtins, PcResult, PyError, Value};

/// A numeric operand, after Python's bool-is-int coercion.
enum Num {
    Int(BigInt),
    Float(f64),
}

fn as_num(v: &Value) -> Option<Num> {
    match v {
        Value::Bool(b) => Some(Num::Int(BigInt::from(u8::from(*b)))),
        Value::Int(n) => Some(Num::Int(n.clone())),
        Value::Float(f) => Some(Num::Float(*f)),
        _ => None,
    }
}

/// Both operands as numbers: `Ok(Int, Int)` stays integral, otherwise both
/// are converted to floats (CPython's `CONVERT_TO_DOUBLE`).
enum Pair {
    Ints(BigInt, BigInt),
    Floats(f64, f64),
}

fn as_pair(a: &Value, b: &Value) -> Option<PcResult<Pair>> {
    let (x, y) = (as_num(a)?, as_num(b)?);
    Some(match (x, y) {
        (Num::Int(x), Num::Int(y)) => Ok(Pair::Ints(x, y)),
        (x, y) => to_f64(x).and_then(|x| to_f64(y).map(|y| Pair::Floats(x, y))),
    })
}

fn to_f64(n: Num) -> PcResult<f64> {
    match n {
        Num::Int(i) => int_to_float(&i),
        Num::Float(f) => Ok(f),
    }
}

fn unsupported(op: &str, a: &Value, b: &Value) -> PyError {
    PyError::type_error(format!(
        "unsupported operand type(s) for {op}: '{}' and '{}'",
        a.type_name(),
        b.type_name()
    ))
}

/// CPython's `float_rem`, without the zero check.
fn float_mod(vx: f64, wx: f64) -> f64 {
    let m = vx % wx;
    if m != 0.0 {
        if (wx < 0.0) != (m < 0.0) {
            m + wx
        } else {
            m
        }
    } else {
        0.0_f64.copysign(wx)
    }
}

/// CPython's `_float_div_mod`, without the zero check.
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

/// Python's `a % b` when both operands are int/bool/float; `None` otherwise.
///
/// # Errors
/// `ZeroDivisionError` (`integer modulo by zero` / `float modulo`), or
/// `OverflowError` when an int operand can't be converted to float.
#[must_use]
pub fn numeric_mod(a: &Value, b: &Value) -> Option<PcResult<Value>> {
    Some(as_pair(a, b)?.and_then(|pair| match pair {
        Pair::Ints(x, y) => {
            if y.is_zero() {
                Err(PyError::zero_division_error("integer modulo by zero"))
            } else {
                Ok(Value::Int(x.mod_floor(&y)))
            }
        }
        Pair::Floats(x, y) => {
            if y == 0.0 {
                Err(PyError::zero_division_error("float modulo"))
            } else {
                Ok(Value::Float(float_mod(x, y)))
            }
        }
    }))
}

/// `pc_format.floor_div(a, b)`: Python's `a // b`.
///
/// # Errors
/// As the Python built-in it wraps.
pub fn floor_div_api(a: &Value, b: &Value) -> PcResult<Value> {
    check_builtins(&[a, b])?;
    match as_pair(a, b).ok_or_else(|| unsupported("//", a, b))?? {
        Pair::Ints(x, y) => {
            if y.is_zero() {
                Err(PyError::zero_division_error(
                    "integer division or modulo by zero",
                ))
            } else {
                Ok(Value::Int(x.div_floor(&y)))
            }
        }
        Pair::Floats(x, y) => {
            if y == 0.0 {
                Err(PyError::zero_division_error("float floor division by zero"))
            } else {
                Ok(Value::Float(float_div_mod(x, y).0))
            }
        }
    }
}

/// `pc_format.div_mod(a, b)`: Python's `divmod(a, b)`.
///
/// # Errors
/// As the Python built-in it wraps.
pub fn div_mod_api(a: &Value, b: &Value) -> PcResult<Value> {
    check_builtins(&[a, b])?;
    let pair = as_pair(a, b).ok_or_else(|| unsupported("divmod()", a, b))??;
    // Zero divisors raise divmod's own message, before numeric_mod sees them.
    let (quotient, remainder) = match &pair {
        Pair::Ints(x, y) => {
            if y.is_zero() {
                return Err(PyError::zero_division_error(
                    "integer division or modulo by zero",
                ));
            }
            let (q, r) = x.div_mod_floor(y);
            (Value::Int(q), Value::Int(r))
        }
        Pair::Floats(x, y) => {
            if *y == 0.0 {
                return Err(PyError::zero_division_error("float divmod()"));
            }
            // `_float_div_mod` computes the remainder exactly as `float_rem`.
            let (q, r) = float_div_mod(*x, *y);
            (Value::Float(q), Value::Float(r))
        }
    };
    Ok(Value::Tuple(vec![quotient, remainder]))
}

const DBL_MANT_DIG: i64 = 53;
const DBL_MIN_EXP: i64 = -1021;
const DBL_MAX_EXP: i64 = 1024;

/// `x * 2**exp` for a value whose result is exactly representable (or
/// overflows to infinity, which callers rule out beforehand).
fn ldexp(mut x: f64, mut exp: i64) -> f64 {
    while exp > 1000 {
        x *= 2f64.powi(1000);
        exp -= 1000;
    }
    while exp < -1000 {
        x *= 2f64.powi(-1000);
        exp += 1000;
    }
    // |exp| <= 1000 here, so the cast is lossless.
    x * 2f64.powi(i32::try_from(exp).unwrap_or(0))
}

fn bit_len(n: &BigInt) -> i64 {
    i64::try_from(n.bits()).unwrap_or(i64::MAX)
}

/// `a / b` for ints, correctly rounded (CPython's `long_true_divide`).
/// `overflow_msg` is the `OverflowError` message when the result is too big.
///
/// # Errors
/// `ZeroDivisionError("division by zero")` or `OverflowError(overflow_msg)`.
pub fn int_true_divide(a: &BigInt, b: &BigInt, overflow_msg: &str) -> PcResult<f64> {
    if b.is_zero() {
        return Err(PyError::zero_division_error("division by zero"));
    }
    let negate = (a.sign() == Sign::Minus) != (b.sign() == Sign::Minus);
    let zero = if negate { -0.0 } else { 0.0 };
    let limit = BigInt::one() << 53;
    let (aa, bb) = (a.abs(), b.abs());
    if aa <= limit && bb <= limit {
        // Both exact as doubles: one IEEE division is correctly rounded.
        let (x, y) = (aa.to_f64().unwrap_or(0.0), bb.to_f64().unwrap_or(1.0));
        let r = x / y;
        return Ok(if negate { -r } else { r });
    }
    if aa.is_zero() {
        return Ok(zero);
    }
    let diff = bit_len(&aa) - bit_len(&bb);
    if diff > DBL_MAX_EXP {
        return Err(PyError::overflow_error(overflow_msg));
    }
    if diff < DBL_MIN_EXP - DBL_MANT_DIG - 1 {
        return Ok(zero);
    }
    let shift = diff.max(DBL_MIN_EXP) - DBL_MANT_DIG - 2;
    let mut inexact = false;
    let x = if shift <= 0 {
        aa << usize::try_from(-shift).unwrap_or(0)
    } else {
        let s = usize::try_from(shift).unwrap_or(0);
        let shifted = &aa >> s;
        if (&shifted << s) != aa {
            inexact = true;
        }
        shifted
    };
    let (q, r) = x.div_rem(&bb);
    if !r.is_zero() {
        inexact = true;
    }
    // q < 2**(DBL_MANT_DIG + 3), so it fits in a u64.
    let mut q = q.to_u64().unwrap_or(0);
    let x_bits = 64 - i64::from(q.leading_zeros());
    let extra_bits = x_bits.max(DBL_MIN_EXP - shift) - DBL_MANT_DIG;
    let mask = 1u64 << (extra_bits - 1);
    let low = q | u64::from(inexact);
    let mut rounded = low;
    if (low & mask) != 0 && (low & (3 * mask - 1)) != 0 {
        rounded = low.wrapping_add(mask);
    }
    q = rounded & !(2 * mask - 1);
    // Exact: q has at most DBL_MANT_DIG + 1 significant bits.
    let dx = q as f64;
    if shift + x_bits >= DBL_MAX_EXP && (shift + x_bits > DBL_MAX_EXP || dx == ldexp(1.0, x_bits)) {
        return Err(PyError::overflow_error(overflow_msg));
    }
    let result = ldexp(dx, shift);
    Ok(if negate { -result } else { result })
}

/// `pc_format.true_div(a, b)`: Python's `a / b`.
///
/// # Errors
/// As the Python built-in it wraps.
pub fn true_div_api(a: &Value, b: &Value) -> PcResult<Value> {
    check_builtins(&[a, b])?;
    match as_pair(a, b).ok_or_else(|| unsupported("/", a, b))?? {
        Pair::Ints(x, y) => Ok(Value::Float(int_true_divide(
            &x,
            &y,
            "integer division result too large for a float",
        )?)),
        Pair::Floats(x, y) => {
            if y == 0.0 {
                Err(PyError::zero_division_error("float division by zero"))
            } else {
                Ok(Value::Float(x / y))
            }
        }
    }
}

/// `operator.index(v)` for `round`'s `ndigits`.
fn index(v: &Value) -> PcResult<BigInt> {
    match v {
        Value::Bool(b) => Ok(BigInt::from(u8::from(*b))),
        Value::Int(n) => Ok(n.clone()),
        other => Err(PyError::type_error(format!(
            "'{}' object cannot be interpreted as an integer",
            other.type_name()
        ))),
    }
}

/// `num / den` rounded to the nearest integer, ties to even (`den > 0`).
fn div_round_half_even(num: &BigInt, den: &BigInt) -> BigInt {
    let (q, r) = num.div_mod_floor(den);
    let twice = &r << 1usize;
    match twice.cmp(den) {
        std::cmp::Ordering::Greater => q + 1,
        std::cmp::Ordering::Equal if q.is_odd() => q + 1,
        _ => q,
    }
}

/// The exact value of a finite float as `(mantissa, exponent)`: `m * 2**e`.
fn float_parts(x: f64) -> (BigInt, i64) {
    let bits = x.to_bits();
    let exp_bits = i64::try_from((bits >> 52) & 0x7ff).unwrap_or(0);
    let frac = bits & ((1u64 << 52) - 1);
    let (m, e) = if exp_bits == 0 {
        (frac, -1074)
    } else {
        (frac | (1u64 << 52), exp_bits - 1075)
    };
    let m = BigInt::from(m);
    (if x.is_sign_negative() { -m } else { m }, e)
}

/// `10**i` for every `i` whose power of ten is an exact double.
const EXACT_POW10: [f64; 23] = [
    1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16,
    1e17, 1e18, 1e19, 1e20, 1e21, 1e22,
];

/// CPython's `double_round`: `x` rounded half-even to `ndigits` decimals,
/// for a finite `x` and `ndigits` in `NDIGITS_MIN..=NDIGITS_MAX`.
fn double_round(x: f64, ndigits: i64) -> PcResult<f64> {
    // Fast path: the rounded scaled value and the power of ten are both
    // exact doubles, so one IEEE division is the correctly rounded quotient.
    if let Some(nd) = u32::try_from(ndigits).ok().filter(|&nd| nd <= 22) {
        if let Some(q) = scaled_round_half_even(x.abs(), nd) {
            if q == 0 {
                return Ok(0.0_f64.copysign(x));
            }
            let pow10 = usize::try_from(nd).ok().and_then(|i| EXACT_POW10.get(i));
            if let (true, Some(&pow10)) = (q < (1_u128 << 53), pow10) {
                // `q < 2**53` converts exactly.
                return Ok((q as f64 / pow10).copysign(x));
            }
        }
    }
    let (m, e) = float_parts(x);
    let ten = BigInt::from(10);
    let pow10 = |n: i64| num_traits::pow(ten.clone(), usize::try_from(n).unwrap_or(0));
    let mut num = m;
    let mut den = BigInt::one();
    if e >= 0 {
        num <<= usize::try_from(e).unwrap_or(0);
    } else {
        den <<= usize::try_from(-e).unwrap_or(0);
    }
    if ndigits >= 0 {
        num *= pow10(ndigits);
    } else {
        den *= pow10(-ndigits);
    }
    let q = div_round_half_even(&num, &den);
    if q.is_zero() {
        return Ok(0.0_f64.copysign(x));
    }
    let msg = "rounded value too large to represent";
    if ndigits >= 0 {
        int_true_divide(&q, &pow10(ndigits), msg)
    } else {
        int_true_divide(&(q * pow10(-ndigits)), &BigInt::one(), msg)
    }
}

/// `round(n, ndigits)` for an int: half-even at `10**-ndigits`.
///
/// When `k = -ndigits` exceeds `bits(n) + 1`, `10**k > 2**k > 2*|n|`, so the
/// exact answer is 0 and is returned without building `10**k` (CPython
/// builds it and hangs; see the known difference). Otherwise `10**k` is no
/// bigger than about 3.3 times `n`.
fn round_int(n: BigInt, ndigits: &BigInt) -> BigInt {
    let Some(k) = (-ndigits).to_biguint().filter(|k| !k.is_zero()) else {
        return n;
    };
    // A `k` past `u64` (or `usize`) is far beyond `bits(n) + 1`.
    let small_k = u64::try_from(&k)
        .ok()
        .filter(|&k| k <= n.bits().saturating_add(1))
        .and_then(|k| usize::try_from(k).ok());
    match small_k {
        Some(k) => {
            let p = num_traits::pow(BigInt::from(10u8), k);
            div_round_half_even(&n, &p) * p
        }
        _ => BigInt::zero(),
    }
}

const NDIGITS_MAX: i64 = 323;
const NDIGITS_MIN: i64 = -308;

/// `pc_format.round_number(x, ndigits)`: Python's `round(x[, ndigits])`.
///
/// # Errors
/// As the Python built-in it wraps.
pub fn round_number_api(x: &Value, ndigits: &Value) -> PcResult<Value> {
    check_builtins(&[x, ndigits])?;
    let num = as_num(x).ok_or_else(|| {
        PyError::type_error(format!(
            "type {} doesn't define __round__ method",
            x.type_name()
        ))
    })?;
    match (num, ndigits) {
        (Num::Int(n), Value::None) => Ok(Value::Int(n)),
        (Num::Float(f), Value::None) => {
            if f.is_nan() {
                return Err(PyError::value_error("cannot convert float NaN to integer"));
            }
            if f.is_infinite() {
                return Err(PyError::overflow_error(
                    "cannot convert float infinity to integer",
                ));
            }
            let (m, e) = float_parts(f.round_ties_even());
            Ok(Value::Int(if e >= 0 {
                m << usize::try_from(e).unwrap_or(0)
            } else {
                m >> usize::try_from(-e).unwrap_or(0)
            }))
        }
        (Num::Int(n), nd) => Ok(Value::Int(round_int(n, &index(nd)?))),
        (Num::Float(f), nd) => {
            // PyNumber_AsSsize_t(o, NULL) clamps out-of-range ints.
            let nd = match nd {
                Value::Bool(b) => i64::from(*b),
                Value::Int(n) => {
                    n.to_i64()
                        .unwrap_or(if n.is_negative() { i64::MIN } else { i64::MAX })
                }
                other => return index(other).map(|_| Value::None),
            };
            if !f.is_finite() || nd > NDIGITS_MAX {
                Ok(Value::Float(f))
            } else if nd < NDIGITS_MIN {
                Ok(Value::Float(0.0 * f))
            } else {
                Ok(Value::Float(double_round(f, nd)?))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(v: PcResult<Value>) -> f64 {
        match v {
            Ok(Value::Float(x)) => x,
            _ => f64::NAN,
        }
    }

    #[test]
    fn true_div_huge() {
        let big = num_traits::pow(BigInt::from(10), 400);
        let big2 = num_traits::pow(BigInt::from(10), 399);
        assert_eq!(
            f(true_div_api(&Value::Int(big.clone()), &Value::Int(big2))),
            10.0
        );
        assert_eq!(f(true_div_api(&Value::int(1), &Value::Int(big))), 0.0);
    }

    #[test]
    fn rounding() {
        assert_eq!(
            f(round_number_api(&Value::Float(2.675), &Value::int(2))),
            2.67
        );
        assert_eq!(
            f(round_number_api(&Value::Float(0.125), &Value::int(2))),
            0.12
        );
        assert_eq!(
            round_number_api(&Value::Float(-2.5), &Value::None),
            Ok(Value::int(-2))
        );
        assert_eq!(
            round_number_api(&Value::int(25), &Value::int(-1)),
            Ok(Value::int(20))
        );
    }
}
