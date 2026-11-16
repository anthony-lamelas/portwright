//! Bindings for task `parse`: `parse_date`, `parse_time`, `parse_datetime`,
//! `parse_duration`, `build_date_regexps`, `build_time_regexps`.
//!
//! Parsing runs in `isodate_port_core::parse`; this layer handles what depends on
//! Python objects: argument binding, the `(yeardigits, expanded)` cache key and its
//! `'%d' %` texts, truthiness, and non-`str` inputs.

use std::sync::Arc;

use isodate_port_core::parse::{self as core, DateRegexps, ParsedDuration, TimeFields};
use isodate_port_core::pc_datetime::date::Date;
use isodate_port_core::pc_datetime::Value;
use num_bigint::BigInt;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{
    PyBool, PyDate, PyDateTime, PyDict, PyFloat, PyInt, PyList, PyString, PyTime, PyTuple, PyTzInfo,
};

use crate::duration::duration_to_py;
use crate::errors::{to_py_err, ISO8601Error, IntoPyResult};
use crate::text::py_to_cps;
use crate::tz::{bind_args, delta_to_py, match_text, str_cps, tp_name, tzspec_to_py, RePattern};
use crate::Modules;

/// `isodates.DATE_REGEX_CACHE`: `(yeardigits, expanded)` -> list of patterns.
static DATE_REGEX_CACHE: PyOnceLock<Py<PyDict>> = PyOnceLock::new();
/// The compiled patterns behind each `DATE_REGEX_CACHE` key, out of the user's reach.
static DATE_PATTERNS: PyOnceLock<Py<PyDict>> = PyOnceLock::new();
/// `isotime.TIME_REGEX_CACHE`.
static TIME_REGEX_CACHE: PyOnceLock<Py<PyList>> = PyOnceLock::new();

/// The core patterns of one `DATE_REGEX_CACHE` entry.
#[pyclass(frozen, name = "_DatePatterns", module = "isodate.isodates")]
struct DatePatterns {
    inner: Arc<DateRegexps>,
}

fn dict<'py>(py: Python<'py>, cell: &PyOnceLock<Py<PyDict>>) -> Bound<'py, PyDict> {
    cell.get_or_init(py, || PyDict::new(py).unbind())
        .bind(py)
        .clone()
}

// --------------------------------------------------------------- arguments

/// A Python object as `date()`'s `i` argument format sees it.
fn date_arg(obj: &Bound<'_, PyAny>) -> PyResult<Value> {
    if let Ok(b) = obj.cast::<PyBool>() {
        return Ok(Value::Bool(b.is_true()));
    }
    if obj.is_instance_of::<PyInt>() {
        return Ok(Value::Int(obj.extract::<BigInt>()?));
    }
    if obj.is_instance_of::<PyFloat>() {
        return Ok(Value::Float(obj.extract::<f64>()?));
    }
    if obj.get_type().hasattr("__index__")? {
        return Ok(Value::Int(
            obj.call_method0("__index__")?.extract::<BigInt>()?,
        ));
    }
    Ok(Value::Other(tp_name(obj)?))
}

// ------------------------------------------------------------- date regexps

/// `'%d' % (value,)`, with Python's errors.
fn percent_d(value: &Bound<'_, PyAny>) -> PyResult<Vec<u32>> {
    let py = value.py();
    let text = PyString::new(py, "%d").rem(PyTuple::new(py, [value])?)?;
    py_to_cps(text.cast::<PyString>()?)
}

/// `build_date_regexps(yeardigits, expanded)` through `DATE_REGEX_CACHE`:
/// the cached list and its core patterns.
fn cached_date_regexps<'py>(
    yeardigits: &Bound<'py, PyAny>,
    expanded: &Bound<'py, PyAny>,
) -> PyResult<(Bound<'py, PyAny>, Arc<DateRegexps>)> {
    let py = yeardigits.py();
    let mut expanded = expanded.clone();
    if yeardigits.ne(4)? {
        expanded = PyBool::new(py, true).to_owned().into_any();
    }
    let key = PyTuple::new(py, [yeardigits, &expanded])?;
    let lists = dict(py, &DATE_REGEX_CACHE);
    let sets = dict(py, &DATE_PATTERNS);
    if let Some(list) = lists.get_item(&key)? {
        if let Some(set) = sets.get_item(&key)? {
            if let Ok(set) = set.cast::<DatePatterns>() {
                return Ok((list, Arc::clone(&set.get().inner)));
            }
        }
    }
    let sign = expanded.is_truthy()?;
    let year_text = percent_d(yeardigits)?;
    let century_text = percent_d(&yeardigits.sub(2)?)?;
    let inner = core::date_regexps(sign, &year_text, &century_text).py_err(py)?;
    let list = PyList::empty(py);
    for pattern in inner.patterns() {
        list.append(Py::new(py, RePattern::new(Arc::clone(pattern)))?)?;
    }
    lists.set_item(&key, &list)?;
    let set = Py::new(
        py,
        DatePatterns {
            inner: Arc::clone(&inner),
        },
    )?;
    sets.set_item(&key, set)?;
    Ok((list.into_any(), inner))
}

/// The core patterns for `parse_date`'s arguments; exact `int`/`bool` keys skip the
/// Python cache (they behave identically there).
fn date_regexps_for(
    py: Python<'_>,
    yeardigits: Option<&Bound<'_, PyAny>>,
    expanded: Option<&Bound<'_, PyAny>>,
) -> PyResult<Arc<DateRegexps>> {
    let small = |obj: Option<&Bound<'_, PyAny>>, default: i64| -> Option<i64> {
        match obj {
            None => Some(default),
            Some(o) if o.is_exact_instance_of::<PyInt>() || o.is_exact_instance_of::<PyBool>() => {
                o.extract::<i64>().ok()
            }
            Some(_) => None,
        }
    };
    if let (Some(yd), Some(exp)) = (small(yeardigits, 4), small(expanded, 0)) {
        if yd == 4 && exp == 0 {
            return core::default_date_regexps().map(Arc::clone).py_err(py);
        }
        if let Some(century) = yd.checked_sub(2) {
            let sign = yd != 4 || exp != 0;
            let text = |n: i64| n.to_string().chars().map(u32::from).collect::<Vec<u32>>();
            return core::date_regexps(sign, &text(yd), &text(century)).py_err(py);
        }
    }
    let four = 4_i32.into_pyobject(py)?.into_any();
    let false_ = PyBool::new(py, false).to_owned().into_any();
    let yeardigits = yeardigits.unwrap_or(&four);
    let mut expanded = expanded.unwrap_or(&false_).clone();
    // parse_date's own `if yeardigits != 4: expanded = True`.
    if yeardigits.ne(4)? {
        expanded = PyBool::new(py, true).to_owned().into_any();
    }
    Ok(cached_date_regexps(yeardigits, &expanded)?.1)
}

/// `build_date_regexps(yeardigits=4, expanded=False)`.
#[pyfunction]
#[pyo3(signature = (*args, **kwargs))]
fn build_date_regexps<'py>(
    args: &Bound<'py, PyTuple>,
    kwargs: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyAny>> {
    let py = args.py();
    let mut slots = bind_args(
        "build_date_regexps",
        &["yeardigits", "expanded"],
        0,
        args,
        kwargs,
    )?
    .into_iter();
    let yeardigits = slots
        .next()
        .flatten()
        .map_or_else(|| Ok(4_i32.into_pyobject(py)?.into_any()), Ok::<_, PyErr>)?;
    let expanded = slots
        .next()
        .flatten()
        .unwrap_or_else(|| PyBool::new(py, false).to_owned().into_any());
    Ok(cached_date_regexps(&yeardigits, &expanded)?.0)
}

// ------------------------------------------------------------------ results

fn date_to_py(py: Python<'_>, date: Date) -> PyResult<Bound<'_, PyDate>> {
    PyDate::new(py, date.year(), field(date.month())?, field(date.day())?)
}

fn field(n: i32) -> PyResult<u8> {
    u8::try_from(n).map_err(|_| PyValueError::new_err("field out of range"))
}

fn tzinfo_of<'py>(py: Python<'py>, time: &TimeFields) -> PyResult<Option<Bound<'py, PyTzInfo>>> {
    let tz = tzspec_to_py(py, time.tz.clone())?;
    if tz.is_none() {
        return Ok(None);
    }
    Ok(Some(tz.cast_into::<PyTzInfo>()?))
}

fn time_to_py<'py>(py: Python<'py>, time: &TimeFields) -> PyResult<Bound<'py, PyTime>> {
    PyTime::new(
        py,
        field(time.hour)?,
        field(time.minute)?,
        field(time.second)?,
        u32::try_from(time.microsecond).unwrap_or(0),
        tzinfo_of(py, time)?.as_ref(),
    )
}

/// `datetime.combine(date, time)`.
fn combine<'py>(
    py: Python<'py>,
    date: Date,
    time: &TimeFields,
) -> PyResult<Bound<'py, PyDateTime>> {
    PyDateTime::new(
        py,
        date.year(),
        field(date.month())?,
        field(date.day())?,
        field(time.hour)?,
        field(time.minute)?,
        field(time.second)?,
        u32::try_from(time.microsecond).unwrap_or(0),
        tzinfo_of(py, time)?.as_ref(),
    )
}

// ------------------------------------------------------------------ parsers

fn parse_date_impl<'py>(
    datestring: &Bound<'py, PyAny>,
    yeardigits: Option<&Bound<'py, PyAny>>,
    expanded: Option<&Bound<'py, PyAny>>,
    defaultmonth: Option<&Bound<'py, PyAny>>,
    defaultday: Option<&Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyDate>> {
    let py = datestring.py();
    let regexps = date_regexps_for(py, yeardigits, expanded)?;
    let text = match_text(datestring)?;
    let one = Value::int(1);
    let month = defaultmonth.map(date_arg).transpose()?;
    let day = defaultday.map(date_arg).transpose()?;
    let date = core::parse_date(
        &text,
        &regexps,
        month.as_ref().unwrap_or(&one),
        day.as_ref().unwrap_or(&one),
    )
    .py_err(py)?;
    date_to_py(py, date)
}

/// `parse_date(datestring, yeardigits=4, expanded=False, defaultmonth=1, defaultday=1)`.
#[pyfunction]
#[pyo3(signature = (*args, **kwargs))]
fn parse_date<'py>(
    args: &Bound<'py, PyTuple>,
    kwargs: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyDate>> {
    let slots = bind_args(
        "parse_date",
        &[
            "datestring",
            "yeardigits",
            "expanded",
            "defaultmonth",
            "defaultday",
        ],
        1,
        args,
        kwargs,
    )?;
    let slot = |i: usize| slots.get(i).and_then(Option::as_ref);
    let Some(datestring) = slot(0) else {
        return Err(PyTypeError::new_err("parse_date() missing argument"));
    };
    parse_date_impl(datestring, slot(1), slot(2), slot(3), slot(4))
}

fn parse_time_impl<'py>(timestring: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyTime>> {
    let py = timestring.py();
    let text = match_text(timestring)?;
    let time = core::parse_time(&text).py_err(py)?;
    time_to_py(py, &time)
}

/// `parse_time(timestring)`.
#[pyfunction]
fn parse_time<'py>(timestring: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyTime>> {
    parse_time_impl(timestring)
}

/// `build_time_regexps()`: the same list on every call.
#[pyfunction]
fn build_time_regexps(py: Python<'_>) -> PyResult<Bound<'_, PyList>> {
    let list = TIME_REGEX_CACHE.get_or_try_init(py, || -> PyResult<Py<PyList>> {
        let list = PyList::empty(py);
        for pattern in core::time_regexps().py_err(py)? {
            list.append(Py::new(py, RePattern::new(pattern))?)?;
        }
        Ok(list.unbind())
    })?;
    Ok(list.bind(py).clone())
}

/// `parse_datetime(datetimestring)`.
#[pyfunction]
fn parse_datetime<'py>(datetimestring: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    let py = datetimestring.py();
    if datetimestring.is_exact_instance_of::<PyString>() {
        let text = str_cps(datetimestring.cast::<PyString>()?)?;
        let (date, time) = core::parse_datetime(&text).py_err(py)?;
        return Ok(combine(py, date, &time)?.into_any());
    }
    // Any other object: `datestring, timestring = datetimestring.split("T")`.
    let parts = datetimestring.call_method1("split", ("T",))?;
    let missing = || -> PyResult<PyErr> {
        let repr = datetimestring.repr()?;
        let repr = py_to_cps(&repr)?;
        let repr: String = repr.iter().filter_map(|&c| char::from_u32(c)).collect();
        Ok(to_py_err(py, core::missing_designator(&repr)))
    };
    let mut items = Vec::with_capacity(2);
    for item in parts.try_iter()? {
        match item {
            Ok(item) => items.push(item),
            Err(e) if e.is_instance_of::<PyValueError>(py) => return Err(missing()?),
            Err(e) => return Err(e),
        }
        if items.len() > 2 {
            return Err(missing()?);
        }
    }
    let [datestring, timestring] = items.as_slice() else {
        return Err(missing()?);
    };
    let date = parse_date_impl(datestring, None, None, None, None)?;
    let time = parse_time_impl(timestring)?;
    let datetime_type = py.import("datetime")?.getattr("datetime")?;
    datetime_type.call_method1("combine", (date, time))
}

/// `parse_duration(datestring, as_timedelta_if_possible=True)`.
#[pyfunction]
#[pyo3(signature = (*args, **kwargs))]
fn parse_duration<'py>(
    args: &Bound<'py, PyTuple>,
    kwargs: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyAny>> {
    let py = args.py();
    let slots = bind_args(
        "parse_duration",
        &["datestring", "as_timedelta_if_possible"],
        1,
        args,
        kwargs,
    )?;
    let slot = |i: usize| slots.get(i).and_then(Option::as_ref);
    let Some(datestring) = slot(0) else {
        return Err(PyTypeError::new_err("parse_duration() missing argument"));
    };
    let Ok(text) = datestring.cast::<PyString>() else {
        let message = PyString::new(py, "Expecting a string %r").rem(datestring)?;
        return Err(PyTypeError::new_err(message.unbind()));
    };
    let parsed = core::parse_duration_text(&str_cps(text)?).py_err(py)?;
    let as_timedelta = match slot(1) {
        Some(flag) => flag.is_truthy()?,
        None => true,
    };
    match parsed.finish(as_timedelta).py_err(py)? {
        ParsedDuration::TimeDelta(delta) => Ok(delta_to_py(py, delta)?.into_any()),
        ParsedDuration::Duration {
            years,
            months,
            tdelta,
        } => duration_to_py(py, &years, &months, delta_to_py(py, tdelta)?.as_any()),
    }
}

// ----------------------------------------------------------------- register

/// Adds this task's names to their submodules.
pub(crate) fn register(modules: &Modules<'_>) -> PyResult<()> {
    let py = modules.root.py();
    let error = py.get_type::<ISO8601Error>();

    let dates = &modules.isodates;
    dates.add_function(wrap_pyfunction!(build_date_regexps, dates)?)?;
    dates.add_function(wrap_pyfunction!(parse_date, dates)?)?;
    dates.add("DATE_REGEX_CACHE", dict(py, &DATE_REGEX_CACHE))?;
    dates.add("ISO8601Error", &error)?;

    let times = &modules.isotime;
    times.add_function(wrap_pyfunction!(build_time_regexps, times)?)?;
    times.add_function(wrap_pyfunction!(parse_time, times)?)?;
    times.add("TIME_REGEX_CACHE", build_time_regexps(py)?)?;
    times.add("ISO8601Error", &error)?;

    let datetimes = &modules.isodatetime;
    datetimes.add_function(wrap_pyfunction!(parse_datetime, datetimes)?)?;
    datetimes.add("ISO8601Error", &error)?;
    if let Some(parse_date) = dates.dict().get_item("parse_date")? {
        datetimes.add("parse_date", parse_date)?;
    }
    if let Some(parse_time) = times.dict().get_item("parse_time")? {
        datetimes.add("parse_time", parse_time)?;
    }

    let durations = &modules.isoduration;
    durations.add_function(wrap_pyfunction!(parse_duration, durations)?)?;
    durations.add("ISO8601Error", &error)?;
    if let Some(parse_datetime) = datetimes.dict().get_item("parse_datetime")? {
        durations.add("parse_datetime", parse_datetime)?;
    }
    Ok(())
}
