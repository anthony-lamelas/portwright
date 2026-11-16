//! Task `timedelta`: the `timedelta` and fixed-offset `timezone` types, and the shim's
//! timedelta functions.

mod bind;
mod delta;
pub mod num;
mod offset;

use num_bigint::BigInt;

use crate::args::{Default, Param};
use crate::error::{PyError, PyResult};
use crate::{Kwargs, Value};

pub(crate) use bind::bind_ref;
pub(crate) use delta::item;
pub use delta::{float_to_int, new_delta, td_in, TimeDelta, Unit, MAX_DELTA_DAYS};
pub use offset::{offset_in, FixedOffset};

const TD: [Param; 1] = [Param::required("td")];

/// The right operand of a timedelta operator, after the shim's conversion.
enum Operand {
    Int(BigInt),
    Float(f64),
    Delta(TimeDelta),
    /// A sequence: `*` tries sequence repetition, other operators give up.
    Sequence,
    /// Anything else: the operator isn't supported.
    Unsupported,
}

impl Operand {
    /// Classifies `value`; a tuple becomes a timedelta when `tuple_is_delta`.
    fn new(value: &Value, tuple_is_delta: TupleAs) -> PyResult<Self> {
        Ok(match value {
            Value::Int(n) => Self::Int(n.clone()),
            Value::Bool(b) => Self::Int(BigInt::from(u8::from(*b))),
            Value::Float(f) => Self::Float(*f),
            Value::Tuple(_) if tuple_is_delta == TupleAs::Delta => Self::Delta(td_in(value)?),
            Value::Tuple(_)
            | Value::List(_)
            | Value::Str(_)
            | Value::Bytes(_)
            | Value::ByteArray(_) => Self::Sequence,
            _ => Self::Unsupported,
        })
    }
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum TupleAs {
    Delta,
    Sequence,
}

fn unsupported(op: &str, right: &Value) -> PyError {
    PyError::type_error(format!(
        "unsupported operand type(s) for {op}: 'datetime.timedelta' and '{}'",
        right.type_name()
    ))
}

fn delta_from(us: &BigInt) -> PyResult<Value> {
    TimeDelta::from_microseconds(us).map(TimeDelta::to_value)
}

/// `pc_datetime.make_timedelta`.
///
/// # Errors
/// Binding errors, then TypeError/OverflowError/ValueError as `timedelta(**kw)` raises them.
pub fn make_timedelta(args: &[Value], kwargs: &Kwargs) -> PyResult<Value> {
    const UNITS: [Unit; 7] = [
        Unit::Days,
        Unit::Seconds,
        Unit::Microseconds,
        Unit::Milliseconds,
        Unit::Minutes,
        Unit::Hours,
        Unit::Weeks,
    ];
    const PARAMS: [Param; 7] = [
        Param::optional("days", Default::Int(0)),
        Param::optional("seconds", Default::Int(0)),
        Param::optional("microseconds", Default::Int(0)),
        Param::optional("milliseconds", Default::Int(0)),
        Param::optional("minutes", Default::Int(0)),
        Param::optional("hours", Default::Int(0)),
        Param::optional("weeks", Default::Int(0)),
    ];
    let v = crate::timedelta::bind_ref("make_timedelta", &PARAMS, args, kwargs)?;
    let components: Vec<(Unit, &Value)> =
        UNITS.iter().copied().zip(v.iter().map(|c| &**c)).collect();
    new_delta(&components).map(TimeDelta::to_value)
}

/// `pc_datetime.timedelta_str`.
///
/// # Errors
/// Binding errors and `_td_in`'s errors.
pub fn timedelta_str(args: &[Value], kwargs: &Kwargs) -> PyResult<Value> {
    let v = crate::timedelta::bind_ref("timedelta_str", &TD, args, kwargs)?;
    Ok(Value::str(td_in(&v[0])?.to_string()))
}

/// `pc_datetime.timedelta_repr`.
///
/// # Errors
/// Binding errors and `_td_in`'s errors.
pub fn timedelta_repr(args: &[Value], kwargs: &Kwargs) -> PyResult<Value> {
    let v = crate::timedelta::bind_ref("timedelta_repr", &TD, args, kwargs)?;
    Ok(Value::str(td_in(&v[0])?.repr()))
}

/// `pc_datetime.timedelta_total_seconds`.
///
/// # Errors
/// Binding errors and `_td_in`'s errors.
pub fn timedelta_total_seconds(args: &[Value], kwargs: &Kwargs) -> PyResult<Value> {
    let v = crate::timedelta::bind_ref("timedelta_total_seconds", &TD, args, kwargs)?;
    let us = td_in(&v[0])?.total_microseconds_big();
    num::true_divide(&us, &BigInt::from(1_000_000)).map(Value::Float)
}

/// `pc_datetime.timedelta_multiply`.
///
/// # Errors
/// Binding errors, `_td_in`'s errors, then the exception the original raises for unsupported operands or overflow.
pub fn timedelta_multiply(args: &[Value], kwargs: &Kwargs) -> PyResult<Value> {
    const PARAMS: [Param; 2] = [Param::required("td"), Param::required("factor")];
    let v = crate::timedelta::bind_ref("timedelta_multiply", &PARAMS, args, kwargs)?;
    let td = td_in(&v[0])?;
    let us = td.total_microseconds_big();
    match Operand::new(&v[1], TupleAs::Sequence)? {
        Operand::Int(n) => delta_from(&(us * n)),
        Operand::Float(f) => {
            let (num, den) = num::as_integer_ratio(f)?;
            delta_from(&num::divide_nearest(&(us * num), &den)?)
        }
        Operand::Sequence => Err(PyError::type_error(
            "can't multiply sequence by non-int of type 'datetime.timedelta'",
        )),
        Operand::Delta(_) | Operand::Unsupported => Err(unsupported("*", &v[1])),
    }
}

/// `pc_datetime.timedelta_divide`.
///
/// # Errors
/// Binding errors, `_td_in`'s errors, then the exception the original raises for unsupported operands, zero or overflow.
pub fn timedelta_divide(args: &[Value], kwargs: &Kwargs) -> PyResult<Value> {
    const PARAMS: [Param; 2] = [Param::required("td"), Param::required("divisor")];
    let v = crate::timedelta::bind_ref("timedelta_divide", &PARAMS, args, kwargs)?;
    let us = td_in(&v[0])?.total_microseconds_big();
    match Operand::new(&v[1], TupleAs::Delta)? {
        Operand::Int(n) => delta_from(&num::divide_nearest(&us, &n)?),
        Operand::Float(f) => {
            let (num, den) = num::as_integer_ratio(f)?;
            delta_from(&num::divide_nearest(&(us * den), &num)?)
        }
        Operand::Delta(d) => num::true_divide(&us, &d.total_microseconds_big()).map(Value::Float),
        Operand::Sequence | Operand::Unsupported => Err(unsupported("/", &v[1])),
    }
}

/// `pc_datetime.timedelta_floor_divide`.
///
/// # Errors
/// Binding errors, `_td_in`'s errors, then the exception the original raises for unsupported operands, zero or overflow.
pub fn timedelta_floor_divide(args: &[Value], kwargs: &Kwargs) -> PyResult<Value> {
    const PARAMS: [Param; 2] = [Param::required("td"), Param::required("divisor")];
    let v = crate::timedelta::bind_ref("timedelta_floor_divide", &PARAMS, args, kwargs)?;
    let us = td_in(&v[0])?.total_microseconds_big();
    match Operand::new(&v[1], TupleAs::Delta)? {
        Operand::Int(n) => delta_from(&num::floor_divide(&us, &n)?),
        Operand::Delta(d) => num::floor_divide(&us, &d.total_microseconds_big()).map(Value::Int),
        Operand::Float(_) | Operand::Sequence | Operand::Unsupported => {
            Err(unsupported("//", &v[1]))
        }
    }
}

/// `pc_datetime.timezone_name`.
///
/// # Errors
/// Binding errors, `_tz`'s errors, and AttributeError for `None`.
pub fn timezone_name(args: &[Value], kwargs: &Kwargs) -> PyResult<Value> {
    const PARAMS: [Param; 1] = [Param::required("utcoffset")];
    let v = crate::timedelta::bind_ref("timezone_name", &PARAMS, args, kwargs)?;
    match offset_in(&v[0])? {
        Some(tz) => Ok(Value::str(tz.tzname())),
        // The shim calls `None.tzname(None)`, which raises AttributeError.
        None => Err(PyError::attribute_error(
            "'NoneType' object has no attribute 'tzname'",
        )),
    }
}
