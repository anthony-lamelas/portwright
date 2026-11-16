//! Bindings for task `tz`: `isodate.tzinfo` and `isodate.isotzinfo`.
//!
//! The tzinfo classes subclass `datetime.tzinfo`, so they live here; the parsing and
//! offset formatting they rely on is in `isodate_port_core::tz`.

use std::sync::Arc;

use isodate_port_core::pc_datetime::timedelta::TimeDelta;
use isodate_port_core::pc_datetime::Value;
use isodate_port_core::tz::{self as core_tz, SplitOffset, TzSpec};
use num_bigint::BigInt;
use pyo3::exceptions::{PyAttributeError, PyOverflowError, PyTypeError};
use pyo3::intern;
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{
    PyBool, PyByteArray, PyBytes, PyDelta, PyDict, PyFloat, PyInt, PyMemoryView, PyString, PyTuple,
    PyType, PyTzInfo,
};

use crate::errors::IntoPyResult;
use crate::text::{cps_to_py, py_to_cps};
use crate::Modules;

mod pattern;

pub use pattern::RePattern;

/// `tzinfo.ZERO`.
static ZERO: PyOnceLock<Py<PyDelta>> = PyOnceLock::new();
/// `tzinfo.UTC`.
static UTC: PyOnceLock<Py<Utc>> = PyOnceLock::new();
/// `tzinfo.LOCAL`.
static LOCAL: PyOnceLock<Py<LocalTimezone>> = PyOnceLock::new();
/// The local zone as the `time` module would report it.
static LOCAL_ZONE: PyOnceLock<LocalZone> = PyOnceLock::new();
/// `isodate.tzinfo._Utc`, the pickle helper `Utc.__reduce__` returns.
static UTC_HELPER: PyOnceLock<Py<PyAny>> = PyOnceLock::new();

fn initialized<'py, T>(py: Python<'py>, cell: &PyOnceLock<Py<T>>) -> PyResult<Bound<'py, T>> {
    cell.get(py)
        .map(|v| v.bind(py).clone())
        .ok_or_else(|| PyAttributeError::new_err("isodate.tzinfo is not initialized"))
}

fn zero(py: Python<'_>) -> PyResult<Bound<'_, PyDelta>> {
    initialized(py, &ZERO)
}

/// Gives a pyclass the bare `tp_name` of a Python class (`'Duration'`, not pyo3's
/// `'isodate.duration.Duration'`), as CPython's own messages show it (`'<' not
/// supported between instances of 'Duration' and 'int'`). Assigning `__name__` to a
/// heap type resets `tp_name`; `__module__` and `__qualname__` are unchanged.
pub(crate) fn use_bare_type_name(cls: &Bound<'_, PyType>) -> PyResult<()> {
    cls.setattr("__name__", cls.getattr("__name__")?)
}

/// `type.__dict__[attr].__get__(ty)`: the attribute as `type` itself computes it
/// (`type_name`, `type_module`, ...), never a property a metaclass defines.
fn type_slot<'py>(ty: &Bound<'py, PyType>, attr: &str) -> PyResult<Bound<'py, PyAny>> {
    let py = ty.py();
    py.get_type::<PyType>()
        .getattr(intern!(py, "__dict__"))?
        .get_item(attr)?
        .call_method1(intern!(py, "__get__"), (ty,))
}

/// `Py_TYPE(obj)->tp_name`: the bare name for builtins and heap types,
/// `module.name` for other static types (`decimal.Decimal`, `datetime.date`).
/// Read through `type`'s own descriptors, so a metaclass that reports another
/// `__name__`, `__module__` or `__flags__` doesn't change it, as in CPython.
pub(crate) fn tp_name(obj: &Bound<'_, PyAny>) -> PyResult<String> {
    /// `Py_TPFLAGS_HEAPTYPE`: classes defined in Python have a bare `tp_name`.
    const HEAPTYPE: u64 = 1 << 9;
    let ty = obj.get_type();
    let name: String = type_slot(&ty, "__name__")?.extract()?;
    let flags: u64 = type_slot(&ty, "__flags__")?.extract()?;
    if flags & HEAPTYPE != 0 {
        return Ok(name);
    }
    // A static type's `__module__` is the part of `tp_name` before the last dot.
    let module: String = type_slot(&ty, "__module__")?.extract()?;
    if module == "builtins" {
        Ok(name)
    } else {
        Ok(format!("{module}.{name}"))
    }
}

/// A Python number as `timedelta`'s constructor sees it.
fn to_value(obj: &Bound<'_, PyAny>) -> PyResult<Value> {
    if let Ok(b) = obj.cast::<PyBool>() {
        return Ok(Value::Bool(b.is_true()));
    }
    if obj.is_instance_of::<PyInt>() {
        return Ok(Value::Int(obj.extract::<BigInt>()?));
    }
    if obj.is_instance_of::<PyFloat>() {
        return Ok(Value::Float(obj.extract::<f64>()?));
    }
    Ok(Value::Other(tp_name(obj)?))
}

pub(crate) fn delta_to_py(py: Python<'_>, delta: TimeDelta) -> PyResult<Bound<'_, PyDelta>> {
    PyDelta::new(
        py,
        delta.days(),
        delta.seconds(),
        delta.microseconds(),
        false,
    )
}

// ------------------------------------------------------------------------ Utc

/// `isodate.tzinfo.Utc`: the UTC time zone.
#[pyclass(extends = PyTzInfo, subclass, module = "isodate.tzinfo", name = "Utc")]
pub struct Utc;

#[pymethods]
impl Utc {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn py_new(_args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>) -> Self {
        Self
    }

    /// Offset from UTC: always `ZERO`.
    fn utcoffset<'py>(
        &self,
        py: Python<'py>,
        dt: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyDelta>> {
        let _ = dt;
        zero(py)
    }

    /// Always `'UTC'`.
    fn tzname(&self, dt: &Bound<'_, PyAny>) -> &'static str {
        let _ = dt;
        "UTC"
    }

    /// No DST: always `ZERO`.
    fn dst<'py>(&self, py: Python<'py>, dt: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyDelta>> {
        let _ = dt;
        zero(py)
    }

    /// `(_Utc, ())`: unpickles to the `UTC` singleton.
    fn __reduce__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        let helper = initialized(py, &UTC_HELPER)?;
        PyTuple::new(py, [helper.into_any(), PyTuple::empty(py).into_any()])
    }
}

/// `_Utc()`: the `UTC` singleton, for unpickling.
#[pyfunction(name = "_Utc")]
fn utc_helper(py: Python<'_>) -> PyResult<Bound<'_, Utc>> {
    initialized(py, &UTC)
}

// ---------------------------------------------------------------- FixedOffset

/// `isodate.tzinfo.FixedOffset(offset_hours=0, offset_minutes=0, name='UTC')`.
///
/// As upstream, the state lives in the instance `__dict__` (`_FixedOffset__offset`,
/// `_FixedOffset__name`), so the inherited `tzinfo.__reduce__` pickles it.
#[pyclass(extends = PyTzInfo, subclass, dict, module = "isodate.tzinfo", name = "FixedOffset")]
pub struct FixedOffset;

impl FixedOffset {
    /// `self.__offset`.
    fn offset<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        slf.getattr(intern!(slf.py(), "_FixedOffset__offset"))
    }

    /// `self.__name`.
    fn name<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        slf.getattr(intern!(slf.py(), "_FixedOffset__name"))
    }

    /// `__init__`'s body: `self.__offset = timedelta(hours=h, minutes=m)`, `self.__name = name`.
    fn init_state(
        slf: &Bound<'_, Self>,
        hours: &Bound<'_, PyAny>,
        minutes: &Bound<'_, PyAny>,
        name: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let py = slf.py();
        let delta =
            core_tz::fixed_offset_delta(&to_value(hours)?, &to_value(minutes)?).py_err(py)?;
        Self::set_state(slf, delta, name)
    }

    /// Stores `__offset` and `__name`.
    fn set_state(
        slf: &Bound<'_, Self>,
        offset: TimeDelta,
        name: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let py = slf.py();
        slf.setattr(
            intern!(py, "_FixedOffset__offset"),
            delta_to_py(py, offset)?,
        )?;
        slf.setattr(intern!(py, "_FixedOffset__name"), name)
    }

    /// `FixedOffset(hours, minutes, name)` for `int` hours and minutes.
    fn from_ints<'py>(
        py: Python<'py>,
        hours: i64,
        minutes: i64,
        name: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, Self>> {
        let delta = core_tz::fixed_offset_delta_ints(hours, minutes).py_err(py)?;
        let obj = Bound::new(py, Self)?;
        Self::set_state(&obj, delta, name)?;
        Ok(obj)
    }

    /// `FixedOffset(hours, minutes, name)` from Rust.
    pub(crate) fn create<'py>(
        hours: &Bound<'py, PyAny>,
        minutes: &Bound<'py, PyAny>,
        name: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, Self>> {
        let obj = Bound::new(hours.py(), Self)?;
        Self::init_state(&obj, hours, minutes, name)?;
        Ok(obj)
    }
}

/// Binds `*args, **kwargs` to positional-or-keyword parameters, raising CPython's
/// `TypeError`s in CPython's order.
pub(crate) fn bind_args<'py>(
    func: &str,
    params: &[&str],
    required: usize,
    args: &Bound<'py, PyTuple>,
    kwargs: Option<&Bound<'py, PyDict>>,
) -> PyResult<Vec<Option<Bound<'py, PyAny>>>> {
    let mut slots: Vec<Option<Bound<'py, PyAny>>> = vec![None; params.len()];
    for (slot, value) in slots.iter_mut().zip(args.iter()) {
        *slot = Some(value);
    }
    if let Some(kwargs) = kwargs {
        for (key, value) in kwargs.iter() {
            let key: String = key.extract()?;
            let Some(slot) = params
                .iter()
                .position(|p| *p == key)
                .and_then(|index| slots.get_mut(index))
            else {
                return Err(PyTypeError::new_err(format!(
                    "{func}() got an unexpected keyword argument '{key}'"
                )));
            };
            if slot.is_some() {
                return Err(PyTypeError::new_err(format!(
                    "{func}() got multiple values for argument '{key}'"
                )));
            }
            *slot = Some(value);
        }
    }
    if args.len() > params.len() {
        let takes = if required == params.len() {
            format!("{required}")
        } else {
            format!("from {required} to {}", params.len())
        };
        let plural = if params.len() == 1 && required == 1 {
            "argument"
        } else {
            "arguments"
        };
        return Err(PyTypeError::new_err(format!(
            "{func}() takes {takes} positional {plural} but {} were given",
            args.len()
        )));
    }
    let missing: Vec<String> = params
        .iter()
        .zip(&slots)
        .take(required)
        .filter(|(_, slot)| slot.is_none())
        .map(|(name, _)| format!("'{name}'"))
        .collect();
    if !missing.is_empty() {
        let names = match missing.as_slice() {
            [one] => one.clone(),
            [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
            [] => String::new(),
        };
        let plural = if missing.len() == 1 { "" } else { "s" };
        return Err(PyTypeError::new_err(format!(
            "{func}() missing {} required positional argument{plural}: {names}",
            missing.len()
        )));
    }
    Ok(slots)
}

/// [`py_to_cps`] with a fast path for ASCII text (each byte is its code point), which
/// allocates once instead of growing the vector char by char.
pub(crate) fn str_cps(s: &Bound<'_, PyString>) -> PyResult<Vec<u32>> {
    if let Ok(text) = s.to_str() {
        if text.is_ascii() {
            return Ok(text.bytes().map(u32::from).collect());
        }
    }
    py_to_cps(s)
}

/// [`cps_to_py`] with a fast path for ASCII code points.
pub(crate) fn cps_str<'py>(py: Python<'py>, cps: &[u32]) -> PyResult<Bound<'py, PyString>> {
    let mut buffer = [0_u8; 32];
    if cps.len() <= buffer.len() {
        let mut ascii = true;
        for (slot, &c) in buffer.iter_mut().zip(cps) {
            match u8::try_from(c) {
                Ok(b) if b.is_ascii() => *slot = b,
                _ => {
                    ascii = false;
                    break;
                }
            }
        }
        if ascii {
            if let Some(text) = buffer
                .get(..cps.len())
                .and_then(|b| std::str::from_utf8(b).ok())
            {
                return Ok(PyString::new(py, text));
            }
        }
    }
    cps_to_py(py, cps)
}

/// `pattern.match(arg)` / `TZ_RE.match(arg)`'s type check, with `re`'s messages.
pub(crate) fn match_text(text: &Bound<'_, PyAny>) -> PyResult<Vec<u32>> {
    if let Ok(s) = text.cast::<PyString>() {
        return str_cps(s);
    }
    let bytes_like = text.is_instance_of::<PyBytes>()
        || text.is_instance_of::<PyByteArray>()
        || text.is_instance_of::<PyMemoryView>()
        || PyMemoryView::from(text).is_ok();
    if bytes_like {
        return Err(PyTypeError::new_err(
            "cannot use a string pattern on a bytes-like object",
        ));
    }
    Err(PyTypeError::new_err(format!(
        "expected string or bytes-like object, got '{}'",
        tp_name(text)?
    )))
}

fn zero_int(py: Python<'_>) -> PyResult<Bound<'_, PyAny>> {
    Ok(0_i32.into_pyobject(py)?.into_any())
}

#[pymethods]
impl FixedOffset {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn py_new(_args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>) -> Self {
        Self
    }

    #[pyo3(signature = (*args, **kwargs))]
    fn __init__(
        slf: &Bound<'_, Self>,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<()> {
        let py = slf.py();
        // `self` counts as a parameter in CPython's messages.
        let mut slots = bind_args(
            "FixedOffset.__init__",
            &["self", "offset_hours", "offset_minutes", "name"],
            1,
            &PyTuple::new(
                py,
                std::iter::once(slf.as_any().clone())
                    .chain(args.iter())
                    .collect::<Vec<_>>(),
            )?,
            kwargs,
        )?
        .into_iter()
        .skip(1);
        let h = slots.next().flatten().map_or_else(|| zero_int(py), Ok)?;
        let m = slots.next().flatten().map_or_else(|| zero_int(py), Ok)?;
        let name = slots
            .next()
            .flatten()
            .unwrap_or_else(|| PyString::new(py, "UTC").into_any());
        Self::init_state(slf, &h, &m, &name)
    }

    /// The fixed offset, whatever `dt` is.
    fn utcoffset<'py>(
        slf: &Bound<'py, Self>,
        dt: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let _ = dt;
        Self::offset(slf)
    }

    /// The name given to the constructor, as given.
    fn tzname<'py>(slf: &Bound<'py, Self>, dt: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
        let _ = dt;
        Self::name(slf)
    }

    /// No DST: always `ZERO`.
    fn dst<'py>(slf: &Bound<'py, Self>, dt: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyDelta>> {
        let _ = dt;
        zero(slf.py())
    }

    /// `'<FixedOffset %r>' % name` (a tuple name is the format's argument tuple).
    fn __repr__<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyString>> {
        let name = Self::name(slf)?;
        let shown = match name.cast::<PyTuple>() {
            Ok(items) => match items.len() {
                0 => {
                    return Err(PyTypeError::new_err(
                        "not enough arguments for format string",
                    ))
                }
                1 => items.get_item(0)?,
                _ => {
                    return Err(PyTypeError::new_err(
                        "not all arguments converted during string formatting",
                    ))
                }
            },
            Err(_) => name,
        };
        let mut text: Vec<u32> = "<FixedOffset ".chars().map(u32::from).collect();
        text.extend(py_to_cps(&shown.repr()?)?);
        text.push(u32::from('>'));
        cps_to_py(slf.py(), &text)
    }
}

// -------------------------------------------------------------- LocalTimezone

/// The local zone, computed once through `datetime` (the port can't import `time`).
struct LocalZone {
    std_offset: Py<PyDelta>,
    dst_offset: Py<PyDelta>,
    dst_diff: Py<PyDelta>,
    names: [Py<PyAny>; 2],
    daylight: bool,
}

impl LocalZone {
    /// `time.timezone`, `time.altzone`, `time.daylight` and `time.tzname`, from the
    /// local offsets on January 1 and July 1 of the current year.
    fn compute(py: Python<'_>) -> PyResult<Self> {
        let datetime = py.import("datetime")?.getattr("datetime")?;
        let year = datetime.call_method0("now")?.getattr("year")?;
        let at = |month: u8| -> PyResult<(Bound<'_, PyAny>, Bound<'_, PyAny>)> {
            let local = datetime
                .call1((year.clone(), month, 1))?
                .call_method0("astimezone")?;
            Ok((
                local.call_method0("utcoffset")?,
                local.call_method0("tzname")?,
            ))
        };
        let (jan_off, jan_name) = at(1)?;
        let (jul_off, jul_name) = at(7)?;
        // CPython's init_timezone: the larger offset is DST (southern zones swap).
        let ((std_off, std_name), (dst_off, dst_name)) = if jan_off.gt(&jul_off)? {
            ((jul_off, jul_name), (jan_off, jan_name))
        } else {
            ((jan_off, jan_name), (jul_off, jul_name))
        };
        let daylight = !std_off.eq(&dst_off)?;
        let dst_offset = if daylight {
            dst_off.clone()
        } else {
            std_off.clone()
        };
        let dst_diff = dst_offset.sub(&std_off)?;
        Ok(Self {
            std_offset: std_off.cast_into::<PyDelta>()?.unbind(),
            dst_offset: dst_offset.cast_into::<PyDelta>()?.unbind(),
            dst_diff: dst_diff.cast_into::<PyDelta>()?.unbind(),
            names: [std_name.unbind(), dst_name.unbind()],
            daylight,
        })
    }
}

fn local_zone(py: Python<'_>) -> PyResult<&LocalZone> {
    LOCAL_ZONE
        .get(py)
        .ok_or_else(|| PyAttributeError::new_err("isodate.tzinfo is not initialized"))
}

/// `PyArg_Parse` format `i`: a C int through `__index__`, with CPython's messages.
fn c_int(value: &Bound<'_, PyAny>) -> PyResult<i32> {
    let index = if value.is_instance_of::<PyInt>() {
        value.clone()
    } else if value.get_type().hasattr("__index__")? {
        value.call_method0("__index__")?
    } else {
        return Err(PyTypeError::new_err(format!(
            "'{}' object cannot be interpreted as an integer",
            tp_name(value)?
        )));
    };
    let big: BigInt = index.extract()?;
    let Ok(long) = i64::try_from(&big) else {
        return Err(PyOverflowError::new_err(
            "Python int too large to convert to C long",
        ));
    };
    i32::try_from(long).map_err(|_| {
        PyOverflowError::new_err(if long > 0 {
            "signed integer is greater than maximum"
        } else {
            "signed integer is less than minimum"
        })
    })
}

/// `LocalTimezone._isdst(dt)`: `time.localtime(time.mktime(tt)).tm_isdst > 0`.
fn is_dst(dt: &Bound<'_, PyAny>) -> PyResult<bool> {
    let py = dt.py();
    let fields = [
        dt.getattr("year")?,
        dt.getattr("month")?,
        dt.getattr("day")?,
        dt.getattr("hour")?,
        dt.getattr("minute")?,
        dt.getattr("second")?,
    ];
    let weekday = dt.call_method0("weekday")?;
    // time.mktime parses the 9-tuple (the last two are 0 and -1) as C ints.
    for field in fields.iter().chain([&weekday]) {
        c_int(field)?;
    }
    let zone = local_zone(py)?;
    if !zone.daylight {
        return Ok(false);
    }
    let datetime = py.import("datetime")?.getattr("datetime")?;
    let naive = datetime.call1(PyTuple::new(py, fields)?)?;
    let offset = naive
        .call_method0("astimezone")?
        .call_method0("utcoffset")?;
    offset.eq(zone.dst_offset.bind(py))
}

/// `isodate.tzinfo.LocalTimezone`: the platform's idea of local time.
#[pyclass(extends = PyTzInfo, subclass, module = "isodate.tzinfo", name = "LocalTimezone")]
pub struct LocalTimezone;

#[pymethods]
impl LocalTimezone {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn py_new(_args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>) -> Self {
        Self
    }

    /// `DSTOFFSET` during DST, else `STDOFFSET`.
    fn utcoffset<'py>(&self, dt: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyDelta>> {
        let py = dt.py();
        let dst = is_dst(dt)?;
        let zone = local_zone(py)?;
        Ok(if dst {
            zone.dst_offset.bind(py).clone()
        } else {
            zone.std_offset.bind(py).clone()
        })
    }

    /// `DSTDIFF` during DST, else `ZERO`.
    fn dst<'py>(&self, dt: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyDelta>> {
        let py = dt.py();
        if is_dst(dt)? {
            Ok(local_zone(py)?.dst_diff.bind(py).clone())
        } else {
            zero(py)
        }
    }

    /// `time.tzname[self._isdst(dt)]`.
    fn tzname<'py>(&self, dt: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
        let py = dt.py();
        let dst = is_dst(dt)?;
        let [std, dst_name] = &local_zone(py)?.names;
        Ok(if dst { dst_name } else { std }.bind(py).clone())
    }

    /// Whether DST is in effect at `dt` (local time).
    fn _isdst(&self, dt: &Bound<'_, PyAny>) -> PyResult<bool> {
        is_dst(dt)
    }
}

// ------------------------------------------------------------------ isotzinfo

/// `build_tzinfo(tzname, tzsign='+', tzhour=0, tzmin=0)`.
#[pyfunction]
#[pyo3(signature = (*args, **kwargs))]
fn build_tzinfo<'py>(
    args: &Bound<'py, PyTuple>,
    kwargs: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyAny>> {
    let py = args.py();
    let mut slots = bind_args(
        "build_tzinfo",
        &["tzname", "tzsign", "tzhour", "tzmin"],
        1,
        args,
        kwargs,
    )?
    .into_iter();
    let tzname = slots
        .next()
        .flatten()
        .unwrap_or_else(|| py.None().into_bound(py));
    let tzsign = slots.next().flatten();
    let tzhour = slots.next().flatten().map_or_else(|| zero_int(py), Ok)?;
    let tzmin = slots.next().flatten().map_or_else(|| zero_int(py), Ok)?;
    if tzname.is_none() || tzname.eq("")? {
        return Ok(py.None().into_bound(py));
    }
    if tzname.eq("Z")? {
        return Ok(initialized(py, &UTC)?.into_any());
    }
    let negative = match tzsign {
        Some(sign) => sign.eq("-")?,
        None => false,
    };
    let sign = (if negative { -1_i32 } else { 1 }).into_pyobject(py)?;
    let hours = sign.mul(tzhour)?;
    let minutes = sign.mul(tzmin)?;
    Ok(FixedOffset::create(&hours, &minutes, &tzname)?.into_any())
}

/// `parse_tzinfo(tzstring)`.
#[pyfunction]
fn parse_tzinfo<'py>(tzstring: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    let py = tzstring.py();
    let text = match_text(tzstring)?;
    tzspec_to_py(py, core_tz::parse_tzinfo(&text).py_err(py)?)
}

/// The tzinfo object a parsed designator describes.
pub(crate) fn tzspec_to_py(py: Python<'_>, spec: TzSpec) -> PyResult<Bound<'_, PyAny>> {
    Ok(match spec {
        TzSpec::Utc => initialized(py, &UTC)?.into_any(),
        TzSpec::Fixed {
            hours,
            minutes,
            name,
        } => {
            FixedOffset::from_ints(py, hours, minutes, &cps_str(py, &name)?.into_any())?.into_any()
        }
        _ => py.None().into_bound(py),
    })
}

/// `tz_isoformat(dt, format='%Z')`.
#[pyfunction]
#[pyo3(signature = (*args, **kwargs))]
fn tz_isoformat<'py>(
    args: &Bound<'py, PyTuple>,
    kwargs: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyString>> {
    let py = args.py();
    let mut slots = bind_args("tz_isoformat", &["dt", "format"], 1, args, kwargs)?.into_iter();
    let dt = slots
        .next()
        .flatten()
        .unwrap_or_else(|| py.None().into_bound(py));
    let format = slots.next().flatten();
    let dt = &dt;
    let tzinfo = dt.getattr("tzinfo")?;
    if tzinfo.is_none() || tzinfo.call_method1("utcoffset", (dt,))?.is_none() {
        return Ok(PyString::new(py, ""));
    }
    let zero = zero(py)?;
    if tzinfo.call_method1("utcoffset", (dt,))?.eq(&zero)?
        && tzinfo.call_method1("dst", (dt,))?.eq(&zero)?
    {
        return Ok(PyString::new(py, "Z"));
    }
    let tdelta = tzinfo.call_method1("utcoffset", (dt,))?;
    let days = tdelta.getattr("days")?;
    let seconds = tdelta.getattr("seconds")?;
    let total = days.mul(24)?.mul(60)?.mul(60)?.add(seconds)?;
    let total: BigInt = total.extract()?;
    let total = i64::try_from(&total).unwrap_or(i64::MAX);
    let format_text = match format {
        None => "%Z".chars().map(u32::from).collect(),
        Some(f) => match f.cast::<PyString>() {
            Ok(s) => py_to_cps(s)?,
            Err(_) => {
                SplitOffset::from_seconds(total).py_err(py)?;
                let shown = py_to_cps(&f.str()?)?;
                return Err(crate::errors::to_py_err(
                    py,
                    core_tz::unknown_format(&shown),
                ));
            }
        },
    };
    let text = core_tz::format_offset(total, &format_text).py_err(py)?;
    cps_to_py(py, text.code_points())
}

// ------------------------------------------------------------------- register

/// Adds this task's names to their submodules.
pub(crate) fn register(modules: &Modules<'_>) -> PyResult<()> {
    let py = modules.root.py();
    let tz = &modules.tzinfo;
    let iso = &modules.isotzinfo;

    let zero = ZERO.get_or_try_init(py, || PyDelta::new(py, 0, 0, 0, false).map(Bound::unbind))?;
    let utc = UTC.get_or_try_init(py, || Py::new(py, Utc))?;
    let local = LOCAL.get_or_try_init(py, || Py::new(py, LocalTimezone))?;
    let zone = LOCAL_ZONE.get_or_try_init(py, || LocalZone::compute(py))?;

    // `_Utc.__module__` is `isodate.tzinfo`, which is in `sys.modules`.
    let utc_func = wrap_pyfunction!(utc_helper, tz)?;
    tz.add_function(utc_func.clone())?;
    let _ = UTC_HELPER.set(py, utc_func.into_any().unbind());

    tz.add("ZERO", zero.bind(py))?;
    tz.add_class::<Utc>()?;
    tz.add("UTC", utc.bind(py))?;
    tz.add_class::<FixedOffset>()?;
    tz.add("STDOFFSET", zone.std_offset.bind(py))?;
    tz.add("DSTOFFSET", zone.dst_offset.bind(py))?;
    tz.add("DSTDIFF", zone.dst_diff.bind(py))?;
    tz.add_class::<LocalTimezone>()?;
    use_bare_type_name(&py.get_type::<Utc>())?;
    use_bare_type_name(&py.get_type::<FixedOffset>())?;
    use_bare_type_name(&py.get_type::<LocalTimezone>())?;
    tz.add("LOCAL", local.bind(py))?;

    iso.add("TZ_REGEX", core_tz::TZ_REGEX)?;
    let pattern = core_tz::TZ_RE.pattern().py_err(py)?;
    iso.add("TZ_RE", Py::new(py, RePattern::new(Arc::clone(pattern)))?)?;
    iso.add("ISO8601Error", py.get_type::<crate::errors::ISO8601Error>())?;
    iso.add("UTC", utc.bind(py))?;
    iso.add("ZERO", zero.bind(py))?;
    iso.add_class::<FixedOffset>()?;
    iso.add_function(wrap_pyfunction!(build_tzinfo, iso)?)?;
    iso.add_function(wrap_pyfunction!(parse_tzinfo, iso)?)?;
    iso.add_function(wrap_pyfunction!(tz_isoformat, iso)?)?;
    Ok(())
}
