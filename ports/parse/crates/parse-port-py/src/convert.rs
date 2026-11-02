//! Python objects for the converters task: `int_convert`, `convert_first`,
//! `percentage`, `FixedTzOffset`, `date_convert`, `with_pattern`, plus
//! [`temporal_to_py`] shared with the other tasks.

use num_bigint::{BigInt, Sign};
use parse_port_core::convert::{self as core_convert, DateArgs, Index};
use parse_port_core::types::{FixedTzOffset as CoreTz, Temporal, TimeDelta, TzInfo};
use parse_port_core::Error;
use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::{PyDate, PyDateTime, PyDelta, PyDict, PyInt, PyString, PyTime, PyTzInfo};

use crate::errors::IntoPyResult;

fn type_name(obj: &Bound<'_, PyAny>) -> PyResult<String> {
    Ok(obj.get_type().name()?.to_string())
}

/// `string[0]`-style access on a non-`str` raises upstream's TypeError.
fn as_str(obj: &Bound<'_, PyAny>) -> PyResult<String> {
    match obj.cast::<PyString>() {
        Ok(s) => Ok(s.to_str()?.to_owned()),
        Err(_) => Err(PyTypeError::new_err(format!(
            "'{}' object is not subscriptable",
            type_name(obj)?
        ))),
    }
}

// ---------------------------------------------------------------- int_convert

/// `parse.int_convert`.
#[pyclass(module = "parse", name = "int_convert", subclass)]
pub struct IntConvert {
    #[pyo3(get, set)]
    base: Py<PyAny>,
}

#[pymethods]
impl IntConvert {
    #[new]
    #[pyo3(signature = (base = None))]
    fn new(py: Python<'_>, base: Option<Py<PyAny>>) -> Self {
        Self {
            base: base.unwrap_or_else(|| py.None()),
        }
    }

    #[classattr]
    #[pyo3(name = "CHARS")]
    fn chars() -> &'static str {
        core_convert::CHARS
    }

    #[pyo3(signature = (string, r#match))]
    fn __call__(
        &self,
        py: Python<'_>,
        string: &Bound<'_, PyAny>,
        r#match: &Bound<'_, PyAny>,
    ) -> PyResult<BigInt> {
        let _ = r#match;
        let s = as_str(string)?;
        let base = self.base.bind(py);
        let base = if base.is_none() {
            None
        } else {
            match base.extract::<BigInt>() {
                Ok(b) => Some(
                    i64::try_from(&b)
                        .ok()
                        .unwrap_or(if b.sign() == Sign::Minus {
                            i64::MIN
                        } else {
                            i64::MAX
                        }),
                ),
                Err(_) => {
                    if s.is_empty() {
                        return core_convert::int_convert(&s, Some(10)).py(py);
                    }
                    return Err(PyTypeError::new_err(
                        "slice indices must be integers or None or have an __index__ method",
                    ));
                }
            }
        };
        core_convert::int_convert(&s, base).py(py)
    }

    fn __reduce__<'py>(slf: &Bound<'py, Self>) -> PyResult<(Bound<'py, PyAny>, (Py<PyAny>,))> {
        let base = slf.borrow().base.clone_ref(slf.py());
        Ok((slf.get_type().into_any(), (base,)))
    }
}

// ---------------------------------------------------------------- convert_first

/// `parse.convert_first`.
#[pyclass(module = "parse", name = "convert_first", subclass)]
pub struct ConvertFirst {
    #[pyo3(get, set)]
    converter: Py<PyAny>,
}

#[pymethods]
impl ConvertFirst {
    #[new]
    fn new(converter: Py<PyAny>) -> Self {
        Self { converter }
    }

    #[pyo3(signature = (string, r#match))]
    fn __call__(
        &self,
        py: Python<'_>,
        string: &Bound<'_, PyAny>,
        r#match: &Bound<'_, PyAny>,
    ) -> PyResult<Py<PyAny>> {
        let _ = r#match;
        let conv = self.converter.clone_ref(py);
        Ok(conv.bind(py).call1((string,))?.unbind())
    }

    fn __reduce__<'py>(slf: &Bound<'py, Self>) -> PyResult<(Bound<'py, PyAny>, (Py<PyAny>,))> {
        let conv = slf.borrow().converter.clone_ref(slf.py());
        Ok((slf.get_type().into_any(), (conv,)))
    }
}

// ---------------------------------------------------------------- percentage

/// `parse.percentage(string, match)`.
#[pyfunction]
#[pyo3(signature = (string, r#match))]
fn percentage(
    py: Python<'_>,
    string: &Bound<'_, PyAny>,
    r#match: &Bound<'_, PyAny>,
) -> PyResult<f64> {
    let _ = r#match;
    let s = as_str(string)?;
    core_convert::percentage(&s).py(py)
}

// ---------------------------------------------------------------- FixedTzOffset

/// `parse.FixedTzOffset`: fixed offset in minutes east from UTC.
#[pyclass(module = "parse", name = "FixedTzOffset", extends = PyTzInfo, subclass)]
pub struct FixedTzOffset {
    offset: Py<PyAny>,
    name: Py<PyAny>,
}

#[pymethods]
impl FixedTzOffset {
    #[new]
    fn new(py: Python<'_>, offset: &Bound<'_, PyAny>, name: Py<PyAny>) -> PyResult<Self> {
        let kwargs = PyDict::new(py);
        kwargs.set_item("minutes", offset)?;
        let delta = py.get_type::<PyDelta>().call((), Some(&kwargs))?;
        Ok(Self {
            offset: delta.unbind(),
            name,
        })
    }

    #[classattr]
    #[pyo3(name = "ZERO")]
    fn zero(py: Python<'_>) -> PyResult<Py<PyDelta>> {
        Ok(PyDelta::new(py, 0, 0, 0, false)?.unbind())
    }

    #[classattr]
    const __hash__: Option<Py<PyAny>> = None;

    #[getter(_offset)]
    fn get_offset(&self, py: Python<'_>) -> Py<PyAny> {
        self.offset.clone_ref(py)
    }

    #[setter(_offset)]
    fn set_offset(&mut self, value: Py<PyAny>) {
        self.offset = value;
    }

    #[getter(_name)]
    fn get_name(&self, py: Python<'_>) -> Py<PyAny> {
        self.name.clone_ref(py)
    }

    #[setter(_name)]
    fn set_name(&mut self, value: Py<PyAny>) {
        self.name = value;
    }

    fn __repr__(slf: &Bound<'_, Self>) -> PyResult<String> {
        let py = slf.py();
        let this = slf.borrow();
        Ok(format!(
            "<{} {} {}>",
            slf.get_type().name()?,
            this.name.bind(py).str()?,
            this.offset.bind(py).str()?
        ))
    }

    fn utcoffset(&self, py: Python<'_>, dt: &Bound<'_, PyAny>) -> Py<PyAny> {
        let _ = dt;
        self.offset.clone_ref(py)
    }

    fn tzname(&self, py: Python<'_>, dt: &Bound<'_, PyAny>) -> Py<PyAny> {
        let _ = dt;
        self.name.clone_ref(py)
    }

    fn dst<'py>(slf: &Bound<'py, Self>, dt: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
        let _ = dt;
        slf.getattr("ZERO")
    }

    fn __eq__<'py>(slf: &Bound<'py, Self>, other: &Bound<'py, PyAny>) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        let Ok(other) = other.cast::<FixedTzOffset>() else {
            return Ok(py.NotImplemented());
        };
        let (a_name, a_off) = {
            let a = slf.borrow();
            (a.name.clone_ref(py), a.offset.clone_ref(py))
        };
        let (b_name, b_off) = {
            let b = other.borrow();
            (b.name.clone_ref(py), b.offset.clone_ref(py))
        };
        let names = a_name.bind(py).eq_obj(b_name.bind(py))?;
        if !names.is_truthy()? {
            return Ok(names.unbind());
        }
        Ok(a_off.bind(py).eq_obj(b_off.bind(py))?.unbind())
    }
}

trait EqObj<'py> {
    fn eq_obj(&self, other: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>>;
}

impl<'py> EqObj<'py> for Bound<'py, PyAny> {
    /// `self == other`, returning the raw result object.
    fn eq_obj(&self, other: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
        self.rich_compare(other, pyo3::basic::CompareOp::Eq)
    }
}

fn delta_to_py<'py>(py: Python<'py>, td: &TimeDelta) -> PyResult<Bound<'py, PyDelta>> {
    let days = i32::try_from(td.days()).unwrap_or(0);
    let seconds = i32::try_from(td.seconds()).unwrap_or(0);
    let micros = i32::try_from(td.microseconds()).unwrap_or(0);
    PyDelta::new(py, days, seconds, micros, false)
}

/// Build a `parse.FixedTzOffset` instance from its core value.
pub fn fixed_tz_to_py<'py>(py: Python<'py>, tz: &CoreTz) -> PyResult<Bound<'py, PyTzInfo>> {
    let obj = Bound::new(
        py,
        FixedTzOffset {
            offset: delta_to_py(py, &tz.offset)?.into_any().unbind(),
            name: PyString::new(py, &tz.name).into_any().unbind(),
        },
    )?;
    Ok(obj.into_super())
}

fn tz_to_py<'py>(py: Python<'py>, tz: &Option<TzInfo>) -> PyResult<Option<Bound<'py, PyTzInfo>>> {
    match tz {
        None => Ok(None),
        Some(TzInfo::Fixed(f)) => Ok(Some(fixed_tz_to_py(py, f)?)),
        Some(TzInfo::Timezone(td)) => Ok(Some(PyTzInfo::fixed_offset(py, delta_to_py(py, td)?)?)),
    }
}

fn u8_of(v: u32) -> u8 {
    u8::try_from(v).unwrap_or(u8::MAX)
}

/// Convert a core date/time value into `datetime.datetime`/`date`/`time`.
pub fn temporal_to_py(py: Python<'_>, value: &Temporal) -> PyResult<Py<PyAny>> {
    Ok(match value {
        Temporal::Date(d) => PyDate::new(py, d.year, u8_of(d.month), u8_of(d.day))?
            .into_any()
            .unbind(),
        Temporal::Time(t) => {
            let tz = tz_to_py(py, &t.tz)?;
            PyTime::new(
                py,
                u8_of(t.hour),
                u8_of(t.minute),
                u8_of(t.second),
                t.microsecond,
                tz.as_ref(),
            )?
            .into_any()
            .unbind()
        }
        Temporal::DateTime(dt) => {
            let tz = tz_to_py(py, &dt.time.tz)?;
            PyDateTime::new(
                py,
                dt.date.year,
                u8_of(dt.date.month),
                u8_of(dt.date.day),
                u8_of(dt.time.hour),
                u8_of(dt.time.minute),
                u8_of(dt.time.second),
                dt.time.microsecond,
                tz.as_ref(),
            )?
            .into_any()
            .unbind()
        }
    })
}

// ---------------------------------------------------------------- date_convert

/// `groups[obj]` index semantics, deferred until the index is used.
fn to_index(obj: &Bound<'_, PyAny>) -> PyResult<Index> {
    if obj.is_instance_of::<PyInt>() || obj.hasattr("__index__")? {
        return Ok(match obj.extract::<BigInt>() {
            Ok(v) => i64::try_from(&v).ok().ok_or_else(|| {
                Error::Index(format!(
                    "cannot fit '{}' into an index-sized integer",
                    type_name(obj).unwrap_or_default()
                ))
            }),
            Err(e) => Err(Error::Type(e.to_string())),
        });
    }
    Ok(Err(Error::Type(format!(
        "tuple indices must be integers or slices, not {}",
        type_name(obj)?
    ))))
}

fn opt_index(obj: Option<&Bound<'_, PyAny>>) -> PyResult<Option<Index>> {
    match obj {
        Some(o) if !o.is_none() => Ok(Some(to_index(o)?)),
        _ => Ok(None),
    }
}

/// `d, m, y = d_m_y` (items collected, at most 4, as Python's unpacking does).
fn unpack_items(obj: &Bound<'_, PyAny>) -> PyResult<Result<Vec<Index>, Error>> {
    let iter = match obj.try_iter() {
        Ok(it) => it,
        Err(_) => {
            return Ok(Err(Error::Type(format!(
                "cannot unpack non-iterable {} object",
                type_name(obj)?
            ))))
        }
    };
    let mut items = Vec::new();
    for item in iter.take(4) {
        items.push(to_index(&item?)?);
    }
    Ok(Ok(items))
}

/// `parse.date_convert(string, match, ...)`.
#[pyfunction]
#[pyo3(signature = (string, r#match, ymd=None, mdy=None, dmy=None, d_m_y=None, hms=None, am=None, tz=None, mm=None, dd=None))]
#[allow(clippy::too_many_arguments, reason = "mirrors upstream signature")]
fn date_convert(
    py: Python<'_>,
    string: &Bound<'_, PyAny>,
    r#match: &Bound<'_, PyAny>,
    ymd: Option<&Bound<'_, PyAny>>,
    mdy: Option<&Bound<'_, PyAny>>,
    dmy: Option<&Bound<'_, PyAny>>,
    d_m_y: Option<&Bound<'_, PyAny>>,
    hms: Option<&Bound<'_, PyAny>>,
    am: Option<&Bound<'_, PyAny>>,
    tz: Option<&Bound<'_, PyAny>>,
    mm: Option<&Bound<'_, PyAny>>,
    dd: Option<&Bound<'_, PyAny>>,
) -> PyResult<Py<PyAny>> {
    let _ = string;
    let groups: Vec<Option<String>> = r#match.call_method0("groups")?.extract()?;
    let truthy = |o: Option<&Bound<'_, PyAny>>| -> PyResult<bool> {
        match o {
            Some(o) => o.is_truthy(),
            None => Ok(false),
        }
    };
    let mut args = DateArgs::default();
    if truthy(mm)? && truthy(dd)? {
        if let (Some(mm), Some(dd)) = (mm, dd) {
            args.mm_dd = Some((to_index(mm)?, to_index(dd)?));
        }
    }
    args.ymd = opt_index(ymd)?;
    args.mdy = opt_index(mdy)?;
    args.dmy = opt_index(dmy)?;
    args.d_m_y = match d_m_y {
        Some(o) if !o.is_none() => Some(unpack_items(o)?),
        _ => None,
    };
    args.hms = opt_index(hms)?;
    args.am = opt_index(am)?;
    args.tz = opt_index(tz)?;
    let value = core_convert::date_convert(&groups, &args).py(py)?;
    temporal_to_py(py, &value)
}

// ---------------------------------------------------------------- with_pattern

/// The `decorator` closure returned by `with_pattern`.
#[pyclass(module = "parse", name = "function")]
pub struct WithPatternDecorator {
    pattern: Py<PyAny>,
    regex_group_count: Py<PyAny>,
}

#[pymethods]
impl WithPatternDecorator {
    fn __call__<'py>(&self, func: Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
        let py = func.py();
        func.setattr("pattern", self.pattern.clone_ref(py))?;
        func.setattr("regex_group_count", self.regex_group_count.clone_ref(py))?;
        Ok(func)
    }

    #[getter]
    fn __qualname__(&self) -> &'static str {
        "with_pattern.<locals>.decorator"
    }

    #[getter]
    fn __name__(&self) -> &'static str {
        "decorator"
    }

    fn __repr__(slf: &Bound<'_, Self>) -> String {
        format!(
            "<function with_pattern.<locals>.decorator at {:#x}>",
            slf.as_ptr() as usize
        )
    }
}

/// `parse.with_pattern(pattern, regex_group_count=None)`.
#[pyfunction]
#[pyo3(signature = (pattern, regex_group_count = None))]
fn with_pattern(
    py: Python<'_>,
    pattern: Py<PyAny>,
    regex_group_count: Option<Py<PyAny>>,
) -> WithPatternDecorator {
    WithPatternDecorator {
        pattern,
        regex_group_count: regex_group_count.unwrap_or_else(|| py.None()),
    }
}

/// Adds this task's public objects to the `parse` module.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<IntConvert>()?;
    m.add_class::<ConvertFirst>()?;
    m.add_class::<FixedTzOffset>()?;
    m.add_function(wrap_pyfunction!(percentage, m)?)?;
    m.add_function(wrap_pyfunction!(date_convert, m)?)?;
    m.add_function(wrap_pyfunction!(with_pattern, m)?)?;
    Ok(())
}
