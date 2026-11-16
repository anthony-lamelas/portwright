//! `isodate.duration`: `fquotmod`, `max_days_in_month` and the year/month arithmetic
//! of `Duration` (task `duration`).
//!
//! The algorithms are generic over [`Operand`], so the PyO3 layer can run them on any
//! Python objects (duck typing, Python's own errors) while ints and `Decimal`s are
//! computed here with [`Number`].

use std::cmp::Ordering;

use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{ToPrimitive, Zero};

use crate::decimal::{Decimal, Rounding};
use crate::error::{Error, Result};
use crate::pc_datetime::timedelta::num::true_divide;

/// A number the core computes with: a Python `int` (or `bool`) or a `Decimal`.
#[derive(Debug, Clone, PartialEq)]
pub enum Number {
    /// `int`, unbounded.
    Int(BigInt),
    /// `decimal.Decimal`.
    Decimal(Decimal),
}

/// The result of `/`: ints divide to a `float`.
#[derive(Debug, Clone, PartialEq)]
pub enum Quotient {
    /// A `Decimal` quotient.
    Number(Number),
    /// `int / int`.
    Float(f64),
}

impl Number {
    fn to_decimal(&self) -> Decimal {
        match self {
            Self::Int(n) => Decimal::from_bigint(n),
            Self::Decimal(d) => d.clone(),
        }
    }

    fn decimal_op(
        &self,
        rhs: &Self,
        op: impl FnOnce(&Decimal, &Decimal) -> Result<Decimal>,
    ) -> Result<Self> {
        op(&self.to_decimal(), &rhs.to_decimal()).map(Self::Decimal)
    }

    /// `self + rhs`.
    ///
    /// # Errors
    /// The `Decimal` signals of the operation.
    pub fn add(&self, rhs: &Self) -> Result<Self> {
        match (self, rhs) {
            (Self::Int(a), Self::Int(b)) => Ok(Self::Int(a + b)),
            _ => self.decimal_op(rhs, Decimal::add),
        }
    }

    /// `self - rhs`.
    ///
    /// # Errors
    /// The `Decimal` signals of the operation.
    pub fn sub(&self, rhs: &Self) -> Result<Self> {
        match (self, rhs) {
            (Self::Int(a), Self::Int(b)) => Ok(Self::Int(a - b)),
            _ => self.decimal_op(rhs, Decimal::sub),
        }
    }

    /// `self * rhs`.
    ///
    /// # Errors
    /// The `Decimal` signals of the operation.
    pub fn mul(&self, rhs: &Self) -> Result<Self> {
        match (self, rhs) {
            (Self::Int(a), Self::Int(b)) => Ok(Self::Int(a * b)),
            _ => self.decimal_op(rhs, Decimal::mul),
        }
    }

    /// `self / rhs`.
    ///
    /// # Errors
    /// `ZeroDivisionError` / `OverflowError` for ints, the `Decimal` signals otherwise.
    pub fn true_div(&self, rhs: &Self) -> Result<Quotient> {
        match (self, rhs) {
            (Self::Int(a), Self::Int(b)) => {
                let f = true_divide(a, b)?;
                if f.is_infinite() {
                    return Err(Error::overflow(
                        "integer division result too large for a float",
                    ));
                }
                Ok(Quotient::Float(f))
            }
            _ => self.decimal_op(rhs, Decimal::div).map(Quotient::Number),
        }
    }

    /// `self % rhs`: ints floor, `Decimal`s truncate.
    ///
    /// # Errors
    /// `ZeroDivisionError` for ints, the `Decimal` signals otherwise.
    pub fn rem(&self, rhs: &Self) -> Result<Self> {
        match (self, rhs) {
            (Self::Int(_), Self::Int(b)) if b.is_zero() => Err(Error::Builtin(
                crate::PyError::zero_division_error("integer modulo by zero"),
            )),
            (Self::Int(a), Self::Int(b)) => Ok(Self::Int(a.mod_floor(b))),
            _ => self.decimal_op(rhs, Decimal::rem),
        }
    }

    /// `-self`.
    ///
    /// # Errors
    /// The `Decimal` signals of the operation.
    pub fn neg(&self) -> Result<Self> {
        match self {
            Self::Int(a) => Ok(Self::Int(-a)),
            Self::Decimal(d) => d.neg().map(Self::Decimal),
        }
    }

    /// `self == rhs`.
    ///
    /// # Errors
    /// `InvalidOperation` when a signaling NaN is compared.
    pub fn equals(&self, rhs: &Self) -> Result<bool> {
        match (self, rhs) {
            (Self::Int(a), Self::Int(b)) => Ok(a == b),
            _ => self.to_decimal().equals(&rhs.to_decimal()),
        }
    }

    /// The ordering of `<`, `>` and friends.
    ///
    /// # Errors
    /// `InvalidOperation` when a NaN is compared.
    pub fn compare(&self, rhs: &Self) -> Result<Ordering> {
        match (self, rhs) {
            (Self::Int(a), Self::Int(b)) => Ok(a.cmp(b)),
            _ => self.to_decimal().compare(&rhs.to_decimal()),
        }
    }

    /// `bool(self)`.
    pub fn is_true(&self) -> bool {
        match self {
            Self::Int(a) => !a.is_zero(),
            Self::Decimal(d) => d.is_true(),
        }
    }

    /// `int(self)`.
    ///
    /// # Errors
    /// `ValueError` / `OverflowError` for special `Decimal`s.
    pub fn to_int(&self) -> Result<BigInt> {
        match self {
            Self::Int(a) => Ok(a.clone()),
            Self::Decimal(d) => d.to_bigint(),
        }
    }

    /// `float(self)`.
    ///
    /// # Errors
    /// `OverflowError` for huge ints, `ValueError` for a signaling NaN.
    pub fn to_f64(&self) -> Result<f64> {
        match self {
            Self::Int(a) => a
                .to_f64()
                .filter(|f| f.is_finite())
                .ok_or_else(|| Error::overflow("int too large to convert to float")),
            Self::Decimal(d) => d.to_f64(),
        }
    }

    /// `self.to_integral(ROUND_FLOOR)`.
    ///
    /// # Errors
    /// `AttributeError` for an int (it has no such method), `InvalidOperation` for a sNaN.
    pub fn to_integral_floor(&self) -> Result<Self> {
        match self {
            Self::Int(_) => Err(Error::attribute(
                "'int' object has no attribute 'to_integral'",
            )),
            Self::Decimal(d) => d.to_integral(Rounding::Floor).map(Self::Decimal),
        }
    }
}

/// The operations `duration.py` applies to its operands, with Python's semantics.
///
/// Each method is one Python expression; implementations decide how values of other
/// types behave (the PyO3 layer falls back to Python's operators).
pub trait Operand: Sized {
    /// The error raised by an operation.
    type Error: From<Error>;

    /// The int literal `n`.
    fn int(n: i64) -> Self;
    /// `self + rhs`.
    fn add(&self, rhs: &Self) -> std::result::Result<Self, Self::Error>;
    /// `self - rhs`.
    fn sub(&self, rhs: &Self) -> std::result::Result<Self, Self::Error>;
    /// `self * rhs`.
    fn mul(&self, rhs: &Self) -> std::result::Result<Self, Self::Error>;
    /// `self / rhs`.
    fn true_div(&self, rhs: &Self) -> std::result::Result<Self, Self::Error>;
    /// `self % rhs`.
    fn rem(&self, rhs: &Self) -> std::result::Result<Self, Self::Error>;
    /// `self.to_integral(ROUND_FLOOR)`.
    fn to_integral_floor(&self) -> std::result::Result<Self, Self::Error>;
    /// `int(self)`.
    fn to_int(&self) -> std::result::Result<Self, Self::Error>;
    /// `float(self).is_integer()`.
    fn float_is_integer(&self) -> std::result::Result<bool, Self::Error>;
    /// `bool(self == rhs)`.
    fn eq(&self, rhs: &Self) -> std::result::Result<bool, Self::Error>;
    /// `bool(self != rhs)`.
    fn ne(&self, rhs: &Self) -> std::result::Result<bool, Self::Error>;
    /// `bool(self > rhs)`.
    fn gt(&self, rhs: &Self) -> std::result::Result<bool, Self::Error>;
    /// `self in items` for a tuple of int literals.
    fn is_in(&self, items: &[i64]) -> std::result::Result<bool, Self::Error> {
        for &item in items {
            if Self::int(item).eq(self)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

/// `fquotmod(val, low, high)`: a floor divmod with boundaries, returning `(int(div), mod)`.
///
/// # Errors
/// Whatever the operations raise (`AttributeError` when `/` gives a float).
pub fn fquotmod<T: Operand>(val: &T, low: &T, high: &T) -> std::result::Result<(T, T), T::Error> {
    let a = val.sub(low)?;
    let b = high.sub(low)?;
    let div = a.true_div(&b)?.to_integral_floor()?;
    let modulo = a.sub(&div.mul(&b)?)?;
    let modulo = modulo.add(low)?;
    Ok((div.to_int()?, modulo))
}

/// `max_days_in_month(year, month)`.
///
/// # Errors
/// Whatever the comparisons and `%` raise.
pub fn max_days_in_month<T: Operand>(year: &T, month: &T) -> std::result::Result<i64, T::Error> {
    if month.is_in(&[1, 3, 5, 7, 8, 10, 12])? {
        return Ok(31);
    }
    if month.is_in(&[4, 6, 9, 11])? {
        return Ok(30);
    }
    let zero = T::int(0);
    if year.rem(&T::int(400))?.eq(&zero)?
        || (year.rem(&T::int(100))?.ne(&zero)? && year.rem(&T::int(4))?.eq(&zero)?)
    {
        return Ok(29);
    }
    Ok(28)
}

/// [`max_days_in_month`] for plain `int`s, without the generic operations.
#[must_use]
pub const fn max_days_in_month_int(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        // `x % n == 0` is the same whether `%` floors or truncates.
        _ if year % 400 == 0 || (year % 100 != 0 && year % 4 == 0) => 29,
        _ => 28,
    }
}

/// Whether the duration is added to or subtracted from the date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// `Duration.__add__`: `date + duration`.
    Add,
    /// `Duration.__rsub__`: `date - duration`.
    Subtract,
}

/// The integer `year`, `month`, `day` for `other.replace(...)` when a duration with
/// `years` and `months` is added to (or subtracted from) a date-like `other`.
///
/// `field` reads `other.month`, `other.year` and `other.day`, in upstream's order.
///
/// # Errors
/// `ValueError` for fractional years or months, then whatever the operations raise.
pub fn shift_date<T: Operand>(
    years: &T,
    months: &T,
    direction: Direction,
    mut field: impl FnMut(&str) -> std::result::Result<T, T::Error>,
) -> std::result::Result<[T; 3], T::Error> {
    if !(years.float_is_integer()? && months.float_is_integer()?) {
        return Err(
            Error::value("fractional years or months not supported for date calculations").into(),
        );
    }
    let apply = |base: &T, delta: &T| match direction {
        Direction::Add => base.add(delta),
        Direction::Subtract => base.sub(delta),
    };
    let newmonth = apply(&field("month")?, months)?;
    let (carry, newmonth) = fquotmod(&newmonth, &T::int(1), &T::int(13))?;
    let newyear = apply(&field("year")?, years)?.add(&carry)?;
    let maxdays = T::int(max_days_in_month(&newyear, &newmonth)?);
    let newday = if field("day")?.gt(&maxdays)? {
        maxdays
    } else {
        field("day")?
    };
    Ok([newyear.to_int()?, newmonth.to_int()?, newday.to_int()?])
}

impl Operand for Number {
    type Error = Error;

    fn int(n: i64) -> Self {
        Self::Int(BigInt::from(n))
    }

    fn add(&self, rhs: &Self) -> Result<Self> {
        Number::add(self, rhs)
    }

    fn sub(&self, rhs: &Self) -> Result<Self> {
        Number::sub(self, rhs)
    }

    fn mul(&self, rhs: &Self) -> Result<Self> {
        Number::mul(self, rhs)
    }

    fn true_div(&self, rhs: &Self) -> Result<Self> {
        match Number::true_div(self, rhs)? {
            Quotient::Number(n) => Ok(n),
            Quotient::Float(_) => Err(Error::attribute(
                "'float' object has no attribute 'to_integral'",
            )),
        }
    }

    fn rem(&self, rhs: &Self) -> Result<Self> {
        Number::rem(self, rhs)
    }

    fn to_integral_floor(&self) -> Result<Self> {
        Number::to_integral_floor(self)
    }

    fn to_int(&self) -> Result<Self> {
        Number::to_int(self).map(Self::Int)
    }

    fn float_is_integer(&self) -> Result<bool> {
        let f = self.to_f64()?;
        Ok(f.is_finite() && f.fract() == 0.0)
    }

    fn eq(&self, rhs: &Self) -> Result<bool> {
        self.equals(rhs)
    }

    fn ne(&self, rhs: &Self) -> Result<bool> {
        self.equals(rhs).map(|e| !e)
    }

    fn gt(&self, rhs: &Self) -> Result<bool> {
        Ok(self.compare(rhs)? == Ordering::Greater)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_days_int_is_generic() -> Result<()> {
        for year in (-1000..=2500).chain([i64::MIN, i64::MAX, i64::MIN + 1]) {
            for month in -2..=14 {
                let generic = max_days_in_month(&int(year), &int(month))?;
                assert_eq!(
                    max_days_in_month_int(year, month),
                    generic,
                    "{year} {month}"
                );
            }
        }
        Ok(())
    }

    fn int(n: i64) -> Number {
        Number::Int(BigInt::from(n))
    }

    fn dec(n: i64) -> Number {
        Number::Decimal(Decimal::from_bigint(&BigInt::from(n)))
    }

    #[test]
    fn fquotmod_floors() -> Result<()> {
        let (div, modulo) = fquotmod(&dec(0), &dec(1), &dec(13))?;
        assert_eq!(div, int(-1));
        assert!(modulo.equals(&int(12))?);
        let (div, modulo) = fquotmod(&dec(25), &int(1), &int(13))?;
        assert_eq!(div, int(2));
        assert!(modulo.equals(&int(1))?);
        Ok(())
    }

    #[test]
    fn int_fquotmod_needs_decimal() {
        assert!(fquotmod(&int(5), &int(1), &int(13)).is_err());
    }

    #[test]
    fn leap_years() -> Result<()> {
        assert_eq!(max_days_in_month(&int(2000), &int(2))?, 29);
        assert_eq!(max_days_in_month(&int(1900), &int(2))?, 28);
        assert_eq!(max_days_in_month(&int(2004), &int(2))?, 29);
        assert_eq!(max_days_in_month(&dec(-100), &int(2))?, 28);
        assert_eq!(max_days_in_month(&int(-4), &dec(2))?, 29);
        assert_eq!(max_days_in_month(&int(1), &int(4))?, 30);
        Ok(())
    }

    #[test]
    fn shift_clamps_day() -> Result<()> {
        let fields = |name: &str| -> Result<Number> {
            Ok(match name {
                "year" => int(2000),
                "month" => int(1),
                _ => int(31),
            })
        };
        let [y, m, d] = shift_date(&dec(0), &dec(1), Direction::Add, fields)?;
        assert_eq!((y, m, d), (int(2000), int(2), int(29)));
        Ok(())
    }
}
