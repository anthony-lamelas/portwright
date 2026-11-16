//! Bindings for task `duration`: `isodate.duration.Duration`, `fquotmod` and
//! `max_days_in_month`.
//!
//! `Duration` keeps `years`, `months` and `tdelta` in its instance `__dict__`, as the
//! original does, so attribute assignment, `__getstate__` and `__setstate__` behave the
//! same. Arithmetic on ints and `Decimal`s runs in the core ([`Number`]); other objects
//! go through Python's own operators, which is how the original's duck typing behaves.

use std::cmp::Ordering;

use isodate_port_core::decimal::{Decimal, Exponent};
use isodate_port_core::duration::{self as core, Direction, Number, Operand, Quotient};
use isodate_port_core::pc_datetime::timedelta::{new_delta, Unit};
use isodate_port_core::pc_datetime::Value;
use isodate_port_core::DecimalSignal;
use num_bigint::BigInt;
use pyo3::exceptions::{
    PyAttributeError, PyMemoryError, PyRecursionError, PyTypeError, PyValueError,
};
use pyo3::intern;
use pyo3::prelude::*;
use pyo3::pyclass::CompareOp;
use pyo3::sync::PyOnceLock;
use pyo3::types::{PyBool, PyDelta, PyDict, PyFloat, PyInt, PyString, PyTuple, PyType};

use crate::errors::{builtin_error, to_py_err, IntoPyResult};
use crate::text::py_to_cps;
use crate::Modules;

static DECIMAL: PyOnceLock<Py<PyType>> = PyOnceLock::new();
static TIMEDELTA: PyOnceLock<Py<PyType>> = PyOnceLock::new();
fn decimal_type(py: Python<'_>) -> PyResult<&Bound<'_, PyType>> {
    DECIMAL.import(py, "decimal", "Decimal")
}

fn timedelta_type(py: Python<'_>) -> PyResult<&Bound<'_, PyType>> {
    TIMEDELTA.import(py, "datetime", "timedelta")
}

// ------------------------------------------------------------ Decimal conversion

/// Reads a Python `Decimal` (or subclass) through `as_tuple()`.
pub(crate) fn decimal_from_py(obj: &Bound<'_, PyAny>) -> PyResult<Decimal> {
    let py = obj.py();
    let parts = obj.call_method0("as_tuple")?;
    let negative: bool = parts.get_item(0)?.extract::<i64>()? != 0;
    let digits: Vec<u8> = parts.get_item(1)?.extract()?;
    let exp = parts.get_item(2)?;
    let exponent = if let Ok(marker) = exp.cast::<PyString>() {
        match marker.to_str()? {
            "F" => Exponent::Infinite,
            "n" => Exponent::Nan,
            _ => Exponent::SNan,
        }
    } else {
        Exponent::Finite(exp.extract()?)
    };
    Decimal::from_parts(negative, &digits, exponent).py_err(py)
}

/// Builds a Python `Decimal` from a core value, exactly (`Decimal((sign, digits, exp))`).
pub fn decimal_to_py<'py>(py: Python<'py>, value: &Decimal) -> PyResult<Bound<'py, PyAny>> {
    if let Some(n) = value.as_exact_int() {
        // `Decimal(int)` has the same sign, digits and exponent 0, and is faster to build.
        return decimal_type(py)?.call1((n,));
    }
    let (negative, digits, exponent) = value.to_parts();
    if let Some(text) = scientific_text(negative, &digits, exponent) {
        // `Decimal(str)` reads `[-]digitsEexp` exactly, as the tuple form does, but faster.
        return decimal_type(py)?.call1((text,));
    }
    let exp: Bound<'py, PyAny> = match exponent {
        Exponent::Finite(e) => e.into_pyobject(py)?.into_any(),
        Exponent::Infinite => PyString::new(py, "F").into_any(),
        Exponent::Nan => PyString::new(py, "n").into_any(),
        Exponent::SNan => PyString::new(py, "N").into_any(),
    };
    let digits = PyTuple::new(py, digits)?;
    let parts = (u8::from(negative), digits, exp);
    decimal_type(py)?.call1((parts,))
}

/// `[-]<digits>E<exp>` for a finite decimal of moderate size, which `Decimal(text)`
/// reads back to the same sign, coefficient and exponent.
fn scientific_text(negative: bool, digits: &[u8], exponent: Exponent) -> Option<String> {
    let Exponent::Finite(exp) = exponent else {
        return None;
    };
    if digits.is_empty() || digits.len() > 1000 || !(-1_000_000..=1_000_000).contains(&exp) {
        return None;
    }
    let mut text = String::with_capacity(digits.len() + 10);
    if negative {
        text.push('-');
    }
    for &d in digits {
        text.push(char::from_digit(u32::from(d), 10)?);
    }
    text.push('E');
    text.push_str(&exp.to_string());
    Some(text)
}

/// `Decimal(str(value))` unless `value` already is a `Decimal`, as `Duration.__init__` does.
fn as_decimal<'py>(value: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    let py = value.py();
    if value.is_instance(decimal_type(py)?)? {
        return Ok(value.clone());
    }
    let text = value.str()?;
    let parsed = Decimal::parse(&py_to_cps(&text)?).py_err(py)?;
    decimal_to_py(py, &parsed)
}

// ------------------------------------------------------------ operands

fn is_finite(n: &Number) -> bool {
    match n {
        Number::Int(_) => true,
        Number::Decimal(d) => !d.is_nan() && !d.is_infinite(),
    }
}

/// `decimal.InvalidOperation([<class 'decimal.DivisionImpossible'>])`, which the frozen
/// `DecimalSignal` can't name.
fn division_impossible(py: Python<'_>) -> PyErr {
    let build = || -> PyResult<PyErr> {
        let decimal = py.import("decimal")?;
        let condition = decimal.getattr("DivisionImpossible")?;
        let args = pyo3::types::PyList::new(py, [condition])?;
        Ok(PyErr::from_value(
            decimal.getattr("InvalidOperation")?.call1((args,))?,
        ))
    };
    build().unwrap_or_else(|e| e)
}

/// Ints below `2**2126` have at most 640 digits, which every
/// `sys.set_int_max_str_digits` setting allows (0 or at least 640).
const ALWAYS_ALLOWED_BITS: u64 = 2126;

/// Integer digits from which CPython's `int(Decimal)` can't allocate the `PyLong`
/// (about 0.44 bytes per decimal digit) and raises `MemoryError` before `"%d"` can
/// apply its digit limit. Above 2711437152599294 digits `mpd_sizeinbase` returns
/// `SIZE_MAX` and it fails on every machine; below that it depends on `malloc`. At
/// 10**10 digits (a 4.4 GB request) it fails under a few GB of memory, and smaller
/// sizes that do allocate take minutes to convert.
const MEMORY_ERROR_DIGITS: i64 = 10_000_000_000;

/// `str(n)`. Ints that may exceed the interpreter's current int→str digit limit
/// are converted by Python, so its `ValueError` follows `sys.set_int_max_str_digits`.
pub(crate) fn int_str(py: Python<'_>, n: &BigInt) -> PyResult<String> {
    if n.bits() <= ALWAYS_ALLOWED_BITS {
        return Ok(n.to_string());
    }
    n.into_pyobject(py)?.str()?.extract()
}

/// A Python value in `duration.py`'s arithmetic: a core number or any other object.
#[derive(Debug)]
enum PyOperand {
    Core(Number),
    Obj(Py<PyAny>),
}

/// A Python exception raised by an operand operation.
struct OpError(PyErr);

impl From<isodate_port_core::Error> for OpError {
    fn from(error: isodate_port_core::Error) -> Self {
        Self(Python::attach(|py| to_py_err(py, error)))
    }
}

impl From<PyErr> for OpError {
    fn from(error: PyErr) -> Self {
        Self(error)
    }
}

impl From<OpError> for PyErr {
    fn from(error: OpError) -> Self {
        error.0
    }
}

type OpResult<T> = Result<T, OpError>;

impl PyOperand {
    /// Exact ints, bools and `Decimal`s are computed in the core; everything else
    /// (subclasses included) keeps its Python behavior.
    fn new(obj: &Bound<'_, PyAny>) -> PyResult<Self> {
        let py = obj.py();
        if obj.is_exact_instance_of::<PyInt>() || obj.is_exact_instance_of::<PyBool>() {
            return Ok(Self::Core(Number::Int(obj.extract::<BigInt>()?)));
        }
        if obj.get_type().is(decimal_type(py)?) {
            return Ok(Self::Core(Number::Decimal(decimal_from_py(obj)?)));
        }
        Ok(Self::Obj(obj.clone().unbind()))
    }

    fn to_py<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        match self {
            Self::Core(Number::Int(n)) => Ok(n.into_pyobject(py)?.into_any()),
            Self::Core(Number::Decimal(d)) => decimal_to_py(py, d),
            Self::Obj(obj) => Ok(obj.bind(py).clone()),
        }
    }

    /// Applies `core_op` when both are core numbers, else the Python operator.
    fn binary(
        &self,
        rhs: &Self,
        core_op: impl FnOnce(&Number, &Number) -> isodate_port_core::Result<Number>,
        py_op: impl for<'py> FnOnce(
            &Bound<'py, PyAny>,
            &Bound<'py, PyAny>,
        ) -> PyResult<Bound<'py, PyAny>>,
    ) -> OpResult<Self> {
        if let (Self::Core(a), Self::Core(b)) = (self, rhs) {
            return Ok(Self::Core(core_op(a, b)?));
        }
        Python::attach(|py| {
            let result = py_op(&self.to_py(py)?, &rhs.to_py(py)?)?;
            Ok(Self::Obj(result.unbind()))
        })
    }

    fn compare(&self, rhs: &Self, op: CompareOp) -> OpResult<bool> {
        if let (Self::Core(a), Self::Core(b)) = (self, rhs) {
            return Ok(match op {
                CompareOp::Eq => a.equals(b)?,
                CompareOp::Ne => !a.equals(b)?,
                _ => {
                    let ord = a.compare(b)?;
                    match op {
                        CompareOp::Lt => ord == Ordering::Less,
                        CompareOp::Le => ord != Ordering::Greater,
                        CompareOp::Gt => ord == Ordering::Greater,
                        _ => ord != Ordering::Less,
                    }
                }
            });
        }
        Python::attach(|py| {
            let result = self.to_py(py)?.rich_compare(rhs.to_py(py)?, op)?;
            Ok(result.is_truthy()?)
        })
    }

    fn is_true(&self, py: Python<'_>) -> PyResult<bool> {
        match self {
            Self::Core(n) => Ok(n.is_true()),
            Self::Obj(obj) => obj.bind(py).is_truthy(),
        }
    }

    fn neg(&self, py: Python<'_>) -> OpResult<Self> {
        match self {
            Self::Core(n) => Ok(Self::Core(n.neg()?)),
            Self::Obj(obj) => Ok(Self::Obj(obj.bind(py).neg()?.unbind())),
        }
    }

    /// `"%d" % value`.
    fn format_d(&self, py: Python<'_>) -> OpResult<String> {
        match self {
            Self::Core(Number::Decimal(d))
                if d.is_true() && d.adjusted() >= MEMORY_ERROR_DIGITS - 1 =>
            {
                Err(OpError(PyMemoryError::new_err(())))
            }
            Self::Core(n) => Ok(int_str(py, &n.to_int()?)?),
            Self::Obj(obj) => {
                let fmt = PyString::new(py, "%d");
                let tuple = PyTuple::new(py, [obj.bind(py)])?;
                Ok(fmt.rem(tuple)?.extract()?)
            }
        }
    }
}

impl Operand for PyOperand {
    type Error = OpError;

    fn int(n: i64) -> Self {
        Self::Core(Number::Int(BigInt::from(n)))
    }

    fn add(&self, rhs: &Self) -> OpResult<Self> {
        self.binary(rhs, Number::add, |a, b| a.add(b))
    }

    fn sub(&self, rhs: &Self) -> OpResult<Self> {
        self.binary(rhs, Number::sub, |a, b| a.sub(b))
    }

    fn mul(&self, rhs: &Self) -> OpResult<Self> {
        self.binary(rhs, Number::mul, |a, b| a.mul(b))
    }

    fn true_div(&self, rhs: &Self) -> OpResult<Self> {
        if let (Self::Core(a), Self::Core(b)) = (self, rhs) {
            return match a.true_div(b)? {
                Quotient::Number(n) => Ok(Self::Core(n)),
                Quotient::Float(f) => {
                    Python::attach(|py| Ok(Self::Obj(PyFloat::new(py, f).into_any().unbind())))
                }
            };
        }
        Python::attach(|py| {
            let result = self.to_py(py)?.div(rhs.to_py(py)?)?;
            Ok(Self::Obj(result.unbind()))
        })
    }

    fn rem(&self, rhs: &Self) -> OpResult<Self> {
        if let (Self::Core(a), Self::Core(b)) = (self, rhs) {
            return match a.rem(b) {
                Ok(n) => Ok(Self::Core(n)),
                // The core reports DivisionImpossible as plain InvalidOperation.
                Err(isodate_port_core::Error::Decimal(DecimalSignal::InvalidOperation))
                    if is_finite(a) && is_finite(b) && b.is_true() =>
                {
                    Err(OpError(Python::attach(division_impossible)))
                }
                Err(e) => Err(e.into()),
            };
        }
        self.binary(rhs, Number::rem, |a, b| a.rem(b))
    }

    fn to_integral_floor(&self) -> OpResult<Self> {
        match self {
            Self::Core(n) => Ok(Self::Core(n.to_integral_floor()?)),
            Self::Obj(obj) => Python::attach(|py| {
                let method = obj.bind(py).getattr("to_integral")?;
                Ok(Self::Obj(method.call1(("ROUND_FLOOR",))?.unbind()))
            }),
        }
    }

    fn to_int(&self) -> OpResult<Self> {
        match self {
            Self::Core(n) => Ok(Self::Core(Number::Int(n.to_int()?))),
            Self::Obj(obj) => Python::attach(|py| {
                let int = py.get_type::<PyInt>().call1((obj.bind(py),))?;
                Ok(Self::new(&int)?)
            }),
        }
    }

    fn float_is_integer(&self) -> OpResult<bool> {
        let f = match self {
            Self::Core(n) => n.to_f64()?,
            Self::Obj(obj) => Python::attach(|py| {
                py.get_type::<PyFloat>()
                    .call1((obj.bind(py),))?
                    .extract::<f64>()
            })?,
        };
        Ok(f.is_finite() && f.fract() == 0.0)
    }

    fn eq(&self, rhs: &Self) -> OpResult<bool> {
        self.compare(rhs, CompareOp::Eq)
    }

    fn ne(&self, rhs: &Self) -> OpResult<bool> {
        self.compare(rhs, CompareOp::Ne)
    }

    fn gt(&self, rhs: &Self) -> OpResult<bool> {
        self.compare(rhs, CompareOp::Gt)
    }

    fn is_in(&self, items: &[i64]) -> OpResult<bool> {
        match self {
            Self::Core(_) => {
                for &item in items {
                    if Self::int(item).eq(self)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            // `tuple.__contains__`: identity, then `item == value` with reflection.
            Self::Obj(obj) => {
                Python::attach(|py| Ok(PyTuple::new(py, items)?.contains(obj.bind(py))?))
            }
        }
    }
}

// ------------------------------------------------------------ timedelta construction

/// A `timedelta` constructor argument as `pc_datetime` sees it.
fn delta_component(obj: &Bound<'_, PyAny>) -> PyResult<Value> {
    if let Ok(b) = obj.cast::<PyBool>() {
        return Ok(Value::Bool(b.is_true()));
    }
    if obj.is_instance_of::<PyInt>() {
        return Ok(Value::Int(obj.extract::<BigInt>()?));
    }
    if obj.is_instance_of::<PyFloat>() {
        return Ok(Value::Float(obj.extract::<f64>()?));
    }
    Ok(Value::Other(crate::tz::tp_name(obj)?))
}

/// `timedelta(days, seconds, microseconds, milliseconds, minutes, hours, weeks)`.
fn make_timedelta<'py>(
    py: Python<'py>,
    values: &[Bound<'py, PyAny>; 7],
) -> PyResult<Bound<'py, PyAny>> {
    const UNITS: [Unit; 7] = [
        Unit::Days,
        Unit::Seconds,
        Unit::Microseconds,
        Unit::Milliseconds,
        Unit::Minutes,
        Unit::Hours,
        Unit::Weeks,
    ];
    let converted = values
        .iter()
        .map(delta_component)
        .collect::<PyResult<Vec<Value>>>()?;
    let components: Vec<(Unit, &Value)> = UNITS.iter().copied().zip(converted.iter()).collect();
    let delta = new_delta(&components).map_err(|e| builtin_error(py, e))?;
    Ok(PyDelta::new(
        py,
        delta.days(),
        delta.seconds(),
        delta.microseconds(),
        false,
    )?
    .into_any())
}

// ------------------------------------------------------------ Duration

/// `isodate.duration.Duration`: a timedelta plus separate years and months.
#[pyclass(name = "Duration", module = "isodate.duration", dict, subclass)]
pub struct Duration {}

const PARAMS: [&str; 9] = [
    "days",
    "seconds",
    "microseconds",
    "milliseconds",
    "minutes",
    "hours",
    "weeks",
    "months",
    "years",
];

/// Python's binding of `Duration.__init__(self, days=0, ..., years=0)`.
fn bind_init<'py>(
    py: Python<'py>,
    args: &Bound<'py, PyTuple>,
    kwargs: Option<&Bound<'py, PyDict>>,
) -> PyResult<Vec<Bound<'py, PyAny>>> {
    let zero = 0i64.into_pyobject(py)?.into_any();
    let mut values: Vec<Option<Bound<'py, PyAny>>> = vec![None; PARAMS.len()];
    let given = args.len();
    for (slot, arg) in values.iter_mut().zip(args.iter()) {
        *slot = Some(arg);
    }
    if let Some(kwargs) = kwargs {
        for (key, value) in kwargs.iter() {
            let name: String = key.extract()?;
            let Some(index) = PARAMS.iter().position(|p| *p == name) else {
                return Err(PyTypeError::new_err(format!(
                    "Duration.__init__() got an unexpected keyword argument '{name}'"
                )));
            };
            if index < given.min(PARAMS.len()) {
                return Err(PyTypeError::new_err(format!(
                    "Duration.__init__() got multiple values for argument '{name}'"
                )));
            }
            if let Some(slot) = values.get_mut(index) {
                *slot = Some(value);
            }
        }
    }
    if given > PARAMS.len() {
        return Err(PyTypeError::new_err(format!(
            "Duration.__init__() takes from 1 to {} positional arguments but {} were given",
            PARAMS.len() + 1,
            given + 1
        )));
    }
    Ok(values
        .into_iter()
        .map(|v| v.unwrap_or_else(|| zero.clone()))
        .collect())
}

/// Sets the instance attributes the way `Duration.__init__` does.
fn init_fields<'py>(
    slf: &Bound<'py, PyAny>,
    months: &Bound<'py, PyAny>,
    years: &Bound<'py, PyAny>,
    delta: [Bound<'py, PyAny>; 7],
) -> PyResult<()> {
    let months = as_decimal(months)?;
    let years = as_decimal(years)?;
    slf.setattr("months", months)?;
    slf.setattr("years", years)?;
    let tdelta = make_timedelta(slf.py(), &delta)?;
    slf.setattr("tdelta", tdelta)
}

/// `Duration(years=years, months=months)`: always the base class.
fn new_duration<'py>(
    py: Python<'py>,
    years: &Bound<'py, PyAny>,
    months: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyAny>> {
    let obj = Bound::new(py, Duration {})?.into_any();
    let zero = 0i64.into_pyobject(py)?.into_any();
    let delta = [(); 7].map(|()| zero.clone());
    init_fields(&obj, months, years, delta)?;
    Ok(obj)
}

fn attr<'py>(obj: &Bound<'py, PyAny>, name: &str) -> PyResult<Bound<'py, PyAny>> {
    obj.getattr(name)
}

fn operand(obj: &Bound<'_, PyAny>, name: &str) -> PyResult<PyOperand> {
    PyOperand::new(&attr(obj, name)?)
}

fn is_duration(other: &Bound<'_, PyAny>) -> bool {
    other.is_instance_of::<Duration>()
}

/// `Duration(years=f(self.years, other.years), months=f(self.months, other.months))`
/// with `tdelta = g(self.tdelta, other.tdelta)`.
fn combine<'py>(
    slf: &Bound<'py, PyAny>,
    other: &Bound<'py, PyAny>,
    op: fn(&PyOperand, &PyOperand) -> OpResult<PyOperand>,
    delta_op: fn(&Bound<'py, PyAny>, &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    let py = slf.py();
    let years = op(&operand(slf, "years")?, &operand(other, "years")?)?.to_py(py)?;
    let months = op(&operand(slf, "months")?, &operand(other, "months")?)?.to_py(py)?;
    let result = new_duration(py, &years, &months)?;
    let tdelta = delta_op(&attr(slf, "tdelta")?, &attr(other, "tdelta")?)?;
    result.setattr("tdelta", tdelta)?;
    Ok(result)
}

/// `Duration(years=self.years, months=self.months)` with `tdelta = op(self.tdelta, other)`.
fn with_delta<'py>(
    slf: &Bound<'py, PyAny>,
    other: &Bound<'py, PyAny>,
    delta_op: fn(&Bound<'py, PyAny>, &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    let py = slf.py();
    let result = new_duration(py, &attr(slf, "years")?, &attr(slf, "months")?)?;
    let tdelta = delta_op(&attr(slf, "tdelta")?, other)?;
    result.setattr("tdelta", tdelta)?;
    Ok(result)
}

/// `other.replace(...)` for `other +/- duration`, where `other` looks like a date.
fn shifted_date<'py>(
    slf: &Bound<'py, PyAny>,
    other: &Bound<'py, PyAny>,
    direction: Direction,
) -> PyResult<Bound<'py, PyAny>> {
    let py = slf.py();
    let years = operand(slf, "years")?;
    let months = operand(slf, "months")?;
    let [year, month, day] =
        core::shift_date(&years, &months, direction, |name| Ok(operand(other, name)?))?;
    let kwargs = PyDict::new(py);
    kwargs.set_item("year", year.to_py(py)?)?;
    kwargs.set_item("month", month.to_py(py)?)?;
    kwargs.set_item("day", day.to_py(py)?)?;
    other.getattr("replace")?.call((), Some(&kwargs))
}

/// Runs `f`, turning an `AttributeError` into `None` (upstream's `except AttributeError`).
fn unless_attribute_error<'py>(
    py: Python<'py>,
    f: impl FnOnce() -> PyResult<Bound<'py, PyAny>>,
) -> PyResult<Option<Bound<'py, PyAny>>> {
    match f() {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.is_instance_of::<PyAttributeError>(py) => Ok(None),
        Err(e) => Err(e),
    }
}

fn add_impl<'py>(slf: &Bound<'py, PyAny>, other: &Bound<'py, PyAny>) -> PyResult<Py<PyAny>> {
    let py = slf.py();
    if is_duration(other) {
        return Ok(combine(slf, other, Operand::add, |a, b| a.add(b))?.unbind());
    }
    let dated = unless_attribute_error(py, || {
        let newdt = shifted_date(slf, other, Direction::Add)?;
        attr(slf, "tdelta")?.add(newdt)
    })?;
    if let Some(result) = dated {
        return Ok(result.unbind());
    }
    let delta = unless_attribute_error(py, || with_delta(slf, other, |a, b| a.add(b)))?;
    Ok(match delta {
        Some(result) => result.unbind(),
        None => py.NotImplemented(),
    })
}

/// Arguments of `__eq__` / `__ne__` that differ.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Equality {
    Eq,
    Ne,
}

fn eq_impl<'py>(
    slf: &Bound<'py, PyAny>,
    other: &Bound<'py, PyAny>,
    kind: Equality,
) -> PyResult<Py<PyAny>> {
    let py = slf.py();
    let want_eq = kind == Equality::Eq;
    if is_duration(other) {
        let twelve = PyOperand::int(12);
        let total = |obj: &Bound<'py, PyAny>| -> PyResult<PyOperand> {
            let years = operand(obj, "years")?;
            let months = operand(obj, "months")?;
            Ok(years.mul(&twelve)?.add(&months)?)
        };
        let (a, b) = (total(slf)?, total(other)?);
        let tdelta = attr(slf, "tdelta")?;
        let other_delta = attr(other, "tdelta")?;
        let result = if want_eq {
            a.eq(&b)?
                && tdelta
                    .rich_compare(other_delta, CompareOp::Eq)?
                    .is_truthy()?
        } else {
            a.ne(&b)?
                || tdelta
                    .rich_compare(other_delta, CompareOp::Ne)?
                    .is_truthy()?
        };
        return Ok(PyBool::new(py, result).to_owned().into_any().unbind());
    }
    let zero = PyOperand::int(0);
    if operand(slf, "years")?.eq(&zero)? && operand(slf, "months")?.eq(&zero)? {
        let op = if want_eq {
            CompareOp::Eq
        } else {
            CompareOp::Ne
        };
        return Ok(attr(slf, "tdelta")?.rich_compare(other, op)?.unbind());
    }
    Ok(PyBool::new(py, !want_eq).to_owned().into_any().unbind())
}

#[pymethods]
impl Duration {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn py_new(_args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>) -> Self {
        Self {}
    }

    #[pyo3(signature = (*args, **kwargs))]
    fn __init__(
        slf: &Bound<'_, Self>,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<()> {
        let py = slf.py();
        let mut values = bind_init(py, args, kwargs)?.into_iter();
        let mut next = || {
            values
                .next()
                .ok_or_else(|| PyTypeError::new_err("Duration"))
        };
        let delta = [
            next()?,
            next()?,
            next()?,
            next()?,
            next()?,
            next()?,
            next()?,
        ];
        let months = next()?;
        let years = next()?;
        init_fields(slf.as_any(), &months, &years, delta)
    }

    fn __getstate__<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        slf.getattr("__dict__")
    }

    fn __setstate__(slf: &Bound<'_, Self>, state: &Bound<'_, PyAny>) -> PyResult<()> {
        slf.getattr("__dict__")?.call_method1("update", (state,))?;
        Ok(())
    }

    fn _op_getattr<'py>(
        slf: &Bound<'py, Self>,
        name: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let py = slf.py();
        // `__getattr__('tdelta')` runs only when `tdelta` is missing, and upstream then
        // recurses through `self.tdelta` until the recursion limit.
        if name
            .cast::<PyString>()
            .is_ok_and(|n| n.to_str().is_ok_and(|n| n == "tdelta"))
        {
            return Err(PyRecursionError::new_err(
                "maximum recursion depth exceeded",
            ));
        }
        let tdelta = slf.as_any().getattr("tdelta")?;
        py.import("builtins")?
            .getattr("getattr")?
            .call1((tdelta, name))
    }

    fn __str__(slf: &Bound<'_, Self>) -> PyResult<String> {
        let py = slf.py();
        let mut params: Vec<String> = Vec::new();
        let years = operand(slf.as_any(), "years")?;
        if years.is_true(py)? {
            params.push(format!("{} years", years.format_d(py)?));
        }
        let months = operand(slf.as_any(), "months")?;
        if months.is_true(py)? {
            let one = PyOperand::int(1);
            let singular = months.compare(&one, CompareOp::Le)?;
            let unit = if singular { "month" } else { "months" };
            params.push(format!("{} {unit}", months.format_d(py)?));
        }
        params.push(slf.getattr("tdelta")?.str()?.extract()?);
        Ok(params.join(", "))
    }

    fn __repr__(slf: &Bound<'_, Self>) -> PyResult<String> {
        let py = slf.py();
        let cls = slf.getattr("__class__")?;
        let module = cls.getattr("__module__")?;
        let name = cls.getattr("__name__")?;
        let tdelta = slf.getattr("tdelta")?;
        let fields = [
            PyOperand::new(&tdelta.getattr("days")?)?,
            PyOperand::new(&slf.getattr("tdelta")?.getattr("seconds")?)?,
            PyOperand::new(&slf.getattr("tdelta")?.getattr("microseconds")?)?,
            operand(slf.as_any(), "years")?,
            operand(slf.as_any(), "months")?,
        ];
        let module = module.str()?.to_string();
        let name = name.str()?.to_string();
        let [days, seconds, microseconds, years, months] = &fields;
        Ok(format!(
            "{module}.{name}({}, {}, {}, years={}, months={})",
            days.format_d(py)?,
            seconds.format_d(py)?,
            microseconds.format_d(py)?,
            years.format_d(py)?,
            months.format_d(py)?,
        ))
    }

    fn __hash__(slf: &Bound<'_, Self>) -> PyResult<isize> {
        let py = slf.py();
        let key = PyTuple::new(
            py,
            [
                slf.getattr("tdelta")?,
                slf.getattr("months")?,
                slf.getattr("years")?,
            ],
        )?;
        key.hash()
    }

    fn __neg__<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        let py = slf.py();
        let years = operand(slf.as_any(), "years")?.neg(py)?.to_py(py)?;
        let months = operand(slf.as_any(), "months")?.neg(py)?.to_py(py)?;
        let result = new_duration(py, &years, &months)?;
        result.setattr("tdelta", slf.getattr("tdelta")?.neg()?)?;
        Ok(result)
    }

    fn __eq__(slf: &Bound<'_, Self>, other: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        eq_impl(slf.as_any(), other, Equality::Eq)
    }

    fn __ne__(slf: &Bound<'_, Self>, other: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        eq_impl(slf.as_any(), other, Equality::Ne)
    }

    #[pyo3(signature = (start=None, end=None))]
    fn totimedelta<'py>(
        slf: &Bound<'py, Self>,
        start: Option<Bound<'py, PyAny>>,
        end: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        match (start, end) {
            (None, None) => Err(PyValueError::new_err("start or end required")),
            (Some(_), Some(_)) => Err(PyValueError::new_err("only start or end allowed")),
            (Some(start), None) => start.add(slf)?.sub(&start),
            (None, Some(end)) => end.sub(end.sub(slf)?),
        }
    }
}

/// `Duration.__sub__(self, other)`.
fn sub_impl<'py>(slf: &Bound<'py, PyAny>, other: &Bound<'py, PyAny>) -> PyResult<Py<PyAny>> {
    let py = slf.py();
    if is_duration(other) {
        return Ok(combine(slf, other, Operand::sub, |a, b| a.sub(b))?.unbind());
    }
    match with_delta(slf, other, |a, b| a.sub(b)) {
        Ok(result) => Ok(result.unbind()),
        Err(e) if e.is_instance_of::<PyTypeError>(py) => Ok(py.NotImplemented()),
        Err(e) => Err(e),
    }
}

/// `Duration.__rsub__(self, other)`.
fn rsub_impl<'py>(slf: &Bound<'py, PyAny>, other: &Bound<'py, PyAny>) -> PyResult<Py<PyAny>> {
    let py = slf.py();
    if other.is_instance(timedelta_type(py)?)? {
        let zero = 0i64.into_pyobject(py)?.into_any();
        let tmpdur = new_duration(py, &zero, &zero)?;
        tmpdur.setattr("tdelta", other)?;
        return Ok(tmpdur.sub(slf)?.unbind());
    }
    let dated = unless_attribute_error(py, || {
        let newdt = shifted_date(slf, other, Direction::Subtract)?;
        newdt.sub(attr(slf, "tdelta")?)
    })?;
    Ok(match dated {
        Some(result) => result.unbind(),
        None => py.NotImplemented(),
    })
}

fn mul_impl<'py>(slf: &Bound<'py, PyAny>, other: &Bound<'py, PyAny>) -> PyResult<Py<PyAny>> {
    let py = slf.py();
    if !other.is_instance_of::<PyInt>() {
        return Ok(py.NotImplemented());
    }
    let factor = PyOperand::new(other)?;
    let years = operand(slf, "years")?.mul(&factor)?.to_py(py)?;
    let months = operand(slf, "months")?.mul(&factor)?.to_py(py)?;
    let result = new_duration(py, &years, &months)?;
    result.setattr("tdelta", attr(slf, "tdelta")?.mul(other)?)?;
    Ok(result.unbind())
}

/// `fquotmod(val, low, high)`.
#[pyfunction]
fn fquotmod<'py>(
    py: Python<'py>,
    val: &Bound<'py, PyAny>,
    low: &Bound<'py, PyAny>,
    high: &Bound<'py, PyAny>,
) -> PyResult<(Bound<'py, PyAny>, Bound<'py, PyAny>)> {
    let (div, modulo) = core::fquotmod(
        &PyOperand::new(val)?,
        &PyOperand::new(low)?,
        &PyOperand::new(high)?,
    )?;
    Ok((div.to_py(py)?, modulo.to_py(py)?))
}

/// `max_days_in_month(year, month)`.
#[pyfunction]
fn max_days_in_month(year: &Bound<'_, PyAny>, month: &Bound<'_, PyAny>) -> PyResult<i64> {
    let small = |obj: &Bound<'_, PyAny>| -> Option<i64> {
        let exact = obj.is_exact_instance_of::<PyInt>() || obj.is_exact_instance_of::<PyBool>();
        exact.then(|| obj.extract::<i64>().ok()).flatten()
    };
    if let (Some(year), Some(month)) = (small(year), small(month)) {
        // Plain ints have no side effects, and `x % n == 0` doesn't depend on rounding.
        return Ok(core::max_days_in_month_int(year, month));
    }
    Ok(core::max_days_in_month(
        &PyOperand::new(year)?,
        &PyOperand::new(month)?,
    )?)
}

/// A Python `Duration` with the given parts (for the parse task).
///
/// # Errors
/// As `Duration(...)` with `Decimal` years and months.
pub fn duration_to_py<'py>(
    py: Python<'py>,
    years: &Decimal,
    months: &Decimal,
    tdelta: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyAny>> {
    // `Duration(years=years, months=months)` then `tdelta`: the attributes `__init__`
    // sets, in its order. Its own zero timedelta would be replaced at once, so it is skipped.
    let years = decimal_to_py(py, years)?;
    let months = decimal_to_py(py, months)?;
    let result = Bound::new(py, Duration {})?.into_any();
    result.setattr(intern!(py, "months"), months)?;
    result.setattr(intern!(py, "years"), years)?;
    result.setattr(intern!(py, "tdelta"), tdelta)?;
    Ok(result)
}

/// The `(years, months, tdelta)` attributes of a `Duration` (for the strf task); `years`
/// and `months` are `None` when they aren't `Decimal`s.
///
/// # Errors
/// The attribute lookups' errors.
pub fn duration_from_py<'py>(
    obj: &Bound<'py, PyAny>,
) -> PyResult<(Option<Decimal>, Option<Decimal>, Bound<'py, PyAny>)> {
    let py = obj.py();
    let read = |name: &str| -> PyResult<Option<Decimal>> {
        let value = obj.getattr(name)?;
        if value.is_instance(decimal_type(py)?)? {
            Ok(Some(decimal_from_py(&value)?))
        } else {
            Ok(None)
        }
    };
    let years = read("years")?;
    let months = read("months")?;
    Ok((years, months, obj.getattr("tdelta")?))
}

/// Whether `obj` is a `Duration` (or subclass) instance.
pub fn is_duration_instance(obj: &Bound<'_, PyAny>) -> bool {
    is_duration(obj)
}

/// Installs the binary operators as ordinary methods (`__radd__ is __add__`, as upstream
/// assigns them), so CPython builds Python-class slots: `d.__rsub__(x)` then runs
/// `__rsub__` itself rather than PyO3's combined `__sub__`/`__rsub__` slot.
/// `Duration.__new__`: `object.__new__(cls)`, the `__new__` upstream inherits.
///
/// Setting a Python-level `__new__` makes the type's `tp_new` CPython's `slot_tp_new`,
/// so, as for upstream's pure-Python class, `copyreg` pickles protocols 0 and 1 through
/// `_reconstructor` and `object.__new__(Duration)` is allowed. `Duration` holds no Rust
/// state (its fields live in `__dict__`), so `object.__new__`'s zeroed object is valid.
#[pyfunction(name = "__new__", signature = (cls, *_args, **_kwargs))]
fn duration_new<'py>(
    cls: &Bound<'py, PyAny>,
    _args: &Bound<'py, PyTuple>,
    _kwargs: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyAny>> {
    let object = cls.py().import("builtins")?.getattr("object")?;
    object.call_method1("__new__", (cls,))
}

/// The Rust body of a `def name(self, other)` method.
type BinaryBody = for<'py> fn(&Bound<'py, PyAny>, &Bound<'py, PyAny>) -> PyResult<Py<PyAny>>;

/// Upstream's docstring of `Duration.__add__` (and `__radd__`).
const ADD_DOC: &str = "
        Durations can be added with Duration, timedelta, date and datetime
        objects.
        ";

/// Upstream's docstring of `Duration.__sub__`.
const SUB_DOC: &str = "
        It is possible to subtract Duration and timedelta objects from Duration
        objects.
        ";

/// Upstream's docstring of `Duration.__rsub__`.
const RSUB_DOC: &str = "
        It is possible to subtract Duration objects from date, datetime and
        timedelta objects.

        TODO: there is some weird behaviour in date - timedelta ...
              if timedelta has seconds or microseconds set, then
              date - timedelta != date + (-timedelta)
              for now we follow this behaviour to avoid surprises when mixing
              timedeltas with Durations, but in case this ever changes in
              the stdlib we can just do:
                return -self + other
              instead of all the current code
        ";

// A `def name(self, other)` method of upstream's pure-Python `Duration`.
//
// Unlike a pyo3 method it behaves as a Python function: called through the class it
// takes any `self`, and a wrong call raises CPython's `TypeError` naming
// `Duration.<name>()`. Accessed on an instance it binds like a function. Its type is
// the port's own (an accepted difference); name, qualname, module, doc and signature
// follow upstream. (Plain comments: a doc comment would become the type's `__doc__`.)
#[pyclass(name = "DurationMethod", module = "isodate.duration", frozen)]
struct BinaryMethod {
    name: &'static str,
    qualname: &'static str,
    doc: Option<&'static str>,
    body: BinaryBody,
}

#[pymethods]
impl BinaryMethod {
    #[pyo3(signature = (*args, **kwargs))]
    fn __call__(
        &self,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Py<PyAny>> {
        let slots = crate::tz::bind_args(self.qualname, &["self", "other"], 2, args, kwargs)?;
        match slots.as_slice() {
            [Some(slf), Some(other)] => (self.body)(slf, other),
            _ => Err(PyTypeError::new_err(format!(
                "{}() missing required positional arguments",
                self.qualname
            ))),
        }
    }

    fn __get__(
        slf: &Bound<'_, Self>,
        obj: Option<&Bound<'_, PyAny>>,
        owner: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        let _ = owner;
        let Some(obj) = obj.filter(|o| !o.is_none()) else {
            return Ok(slf.clone().into_any().unbind());
        };
        let bound = BoundBinaryMethod {
            func: slf.clone().unbind(),
            obj: obj.clone().unbind(),
        };
        Ok(Bound::new(slf.py(), bound)?.into_any().unbind())
    }

    #[getter]
    fn __name__(&self) -> &'static str {
        self.name
    }

    #[getter]
    fn __qualname__(&self) -> &'static str {
        self.qualname
    }

    #[getter]
    fn __module__(&self) -> &'static str {
        "isodate.duration"
    }

    #[getter]
    fn __doc__(&self) -> Option<&'static str> {
        self.doc
    }

    /// `inspect.signature` reads this text, as for upstream's `def name(self, other)`.
    #[getter]
    fn __signature__(&self) -> &'static str {
        "(self, other)"
    }

    fn __repr__(slf: &Bound<'_, Self>) -> String {
        format!(
            "<function {} at {:#x}>",
            slf.get().qualname,
            slf.as_ptr() as usize
        )
    }
}

// A `BinaryMethod` bound to an instance (`duration.__mul__`). Plain comment so it
// isn't published as `__doc__`; `__doc__` and `__module__` forward to the function.
#[pyclass(name = "BoundDurationMethod", module = "isodate.duration", frozen)]
struct BoundBinaryMethod {
    func: Py<BinaryMethod>,
    obj: Py<PyAny>,
}

#[pymethods]
impl BoundBinaryMethod {
    #[pyo3(signature = (*args, **kwargs))]
    fn __call__(
        &self,
        py: Python<'_>,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Py<PyAny>> {
        let all: Vec<Bound<'_, PyAny>> = std::iter::once(self.obj.bind(py).clone())
            .chain(args.iter())
            .collect();
        self.func.get().__call__(&PyTuple::new(py, all)?, kwargs)
    }

    #[getter]
    fn __func__(&self, py: Python<'_>) -> Py<BinaryMethod> {
        self.func.clone_ref(py)
    }

    #[getter]
    fn __self__(&self, py: Python<'_>) -> Py<PyAny> {
        self.obj.clone_ref(py)
    }

    #[getter]
    fn __name__(&self) -> &'static str {
        self.func.get().name
    }

    #[getter]
    fn __qualname__(&self) -> &'static str {
        self.func.get().qualname
    }

    #[getter]
    fn __module__(&self) -> &'static str {
        self.func.get().__module__()
    }

    #[getter]
    fn __doc__(&self) -> Option<&'static str> {
        self.func.get().doc
    }

    /// `inspect.signature` of a bound method drops `self`.
    #[getter]
    fn __signature__(&self) -> &'static str {
        "(other)"
    }

    /// Binding an already bound method gives it back, so `inspect` treats it as a
    /// routine, as it does upstream's bound method.
    fn __get__(
        slf: &Bound<'_, Self>,
        obj: Option<&Bound<'_, PyAny>>,
        owner: Option<&Bound<'_, PyAny>>,
    ) -> Py<Self> {
        let _ = (obj, owner);
        slf.clone().unbind()
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let shown = self.obj.bind(py).repr()?;
        Ok(format!(
            "<bound method {} of {}>",
            self.func.get().qualname,
            shown.to_str()?
        ))
    }
}

/// Installs upstream's Python-level class attributes: `__new__`, the arithmetic
/// operators (with `__radd__ = __add__` and `__rmul__ = __mul__`) and `__getattr__`.
fn install_operators(cls: &Bound<'_, PyType>) -> PyResult<()> {
    let py = cls.py();
    let new = wrap_pyfunction!(duration_new, py)?;
    let staticmethod = py.import("builtins")?.getattr("staticmethod")?;
    cls.setattr("__new__", staticmethod.call1((new,))?)?;
    type Spec = (
        &'static str,
        &'static str,
        &'static [&'static str],
        Option<&'static str>,
        BinaryBody,
    );
    let methods: [Spec; 4] = [
        (
            "__add__",
            "Duration.__add__",
            &["__add__", "__radd__"],
            Some(ADD_DOC),
            add_impl,
        ),
        (
            "__mul__",
            "Duration.__mul__",
            &["__mul__", "__rmul__"],
            None,
            mul_impl,
        ),
        (
            "__sub__",
            "Duration.__sub__",
            &["__sub__"],
            Some(SUB_DOC),
            sub_impl,
        ),
        (
            "__rsub__",
            "Duration.__rsub__",
            &["__rsub__"],
            Some(RSUB_DOC),
            rsub_impl,
        ),
    ];
    for (name, qualname, attrs, doc, body) in methods {
        let method = Bound::new(
            py,
            BinaryMethod {
                name,
                qualname,
                doc,
                body,
            },
        )?;
        for attr in attrs {
            cls.setattr(*attr, &method)?;
        }
    }
    let getattr = cls.getattr("__dict__")?.get_item("_op_getattr")?;
    cls.setattr("__getattr__", getattr)?;
    cls.delattr("_op_getattr")?;
    Ok(())
}

/// Adds this task's names to their submodules.
pub(crate) fn register(modules: &Modules<'_>) -> PyResult<()> {
    let py = modules.root.py();
    let m = &modules.duration;
    m.add_class::<Duration>()?;
    install_operators(&py.get_type::<Duration>())?;
    for cls in [
        py.get_type::<Duration>(),
        py.get_type::<BinaryMethod>(),
        py.get_type::<BoundBinaryMethod>(),
    ] {
        crate::tz::use_bare_type_name(&cls)?;
    }
    m.add_function(wrap_pyfunction!(fquotmod, m)?)?;
    m.add_function(wrap_pyfunction!(max_days_in_month, m)?)?;
    m.add("timedelta", timedelta_type(py)?)?;
    m.add("Decimal", decimal_type(py)?)?;
    m.add("ROUND_FLOOR", "ROUND_FLOOR")?;
    Ok(())
}
