//! Bindings for task `strf`: `isodate.isostrf` (constants, `strftime`) and the
//! `*_isoformat` wrappers of `isodates`, `isotime`, `isodatetime` and `isoduration`.
//!
//! The formatting runs in the core; this layer reads the object's attributes and
//! calls its methods (`toordinal`, `isoweekday`, `isocalendar`) lazily, one Python
//! operation per request, so the errors are Python's own.

use isodate_port_core::strf::{self as core, DateMethod, Num, StrfSource, Target};
use isodate_port_core::tz::OffsetFormat;
use isodate_port_core::{Error, PyStr};
use num_bigint::BigInt;
use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{PyByteArray, PyBytes, PyDelta, PyFloat, PyInt, PyMemoryView, PyString, PyType};

use crate::duration::{decimal_from_py, int_str, is_duration_instance};
use crate::errors::{to_py_err, IntoPyResult};
use crate::text::{cps_to_py, py_to_cps};
use crate::tz::tp_name;
use crate::Modules;

static DECIMAL: PyOnceLock<Py<PyType>> = PyOnceLock::new();
/// `isodate.isotzinfo.tz_isoformat`, which `%z`, `%Z` and `%h` call.
static TZ_ISOFORMAT: PyOnceLock<Py<PyAny>> = PyOnceLock::new();

// ------------------------------------------------------------------ values

/// A Python object as a core [`Num`].
fn to_num(obj: &Bound<'_, PyAny>) -> PyResult<Num> {
    let py = obj.py();
    if let Ok(int) = obj.cast::<PyInt>() {
        return Ok(Num::Int(int.extract::<BigInt>()?));
    }
    if let Ok(float) = obj.cast::<PyFloat>() {
        return Ok(Num::Float(float.value()));
    }
    if obj.is_instance(DECIMAL.import(py, "decimal", "Decimal")?)? {
        return Ok(Num::Decimal(decimal_from_py(obj)?));
    }
    Ok(Num::Other(tp_name(obj)?))
}

/// An argument that may be omitted (`Default`), distinct from an explicit `None`.
enum Arg<'py> {
    Given(Bound<'py, PyAny>),
    Default,
}

impl<'a, 'py> FromPyObject<'a, 'py> for Arg<'py> {
    type Error = PyErr;

    fn extract(obj: Borrowed<'a, 'py, PyAny>) -> PyResult<Self> {
        Ok(Self::Given(obj.to_owned()))
    }
}

// ------------------------------------------------------------------ source

/// A core or Python error raised while formatting.
enum StrfError {
    Core(Error),
    Py(PyErr),
}

impl From<Error> for StrfError {
    fn from(error: Error) -> Self {
        Self::Core(error)
    }
}

impl From<PyErr> for StrfError {
    fn from(error: PyErr) -> Self {
        Self::Py(error)
    }
}

impl StrfError {
    fn into_py(self, py: Python<'_>) -> PyErr {
        match self {
            Self::Core(error) => to_py_err(py, error),
            Self::Py(error) => error,
        }
    }
}

/// The object being formatted.
struct Source<'a, 'py>(&'a Bound<'py, PyAny>);

impl StrfSource for Source<'_, '_> {
    type Error = StrfError;

    fn attr(&self, name: &'static str) -> Result<Num, StrfError> {
        Ok(to_num(&self.0.getattr(name)?)?)
    }

    fn call(&self, method: DateMethod) -> Result<Num, StrfError> {
        let value = match method {
            DateMethod::ToOrdinal => self.0.call_method0("toordinal")?,
            DateMethod::IsoWeekday => self.0.call_method0("isoweekday")?,
            DateMethod::IsoWeek => self.0.call_method0("isocalendar")?.get_item(1)?,
        };
        Ok(to_num(&value)?)
    }

    fn tz_isoformat(&self, format: OffsetFormat) -> Result<PyStr, StrfError> {
        let py = self.0.py();
        let code = match format {
            OffsetFormat::Extended => "%Z",
            OffsetFormat::Basic => "%z",
            OffsetFormat::Hours => "%h",
        };
        let func = TZ_ISOFORMAT
            .get(py)
            .ok_or_else(|| PyTypeError::new_err("tz_isoformat is not registered"))?;
        let text = func.bind(py).call1((self.0, code))?;
        let text = text.cast_into::<PyString>().map_err(PyErr::from)?;
        Ok(PyStr::from_code_points(py_to_cps(&text)?))
    }

    fn check_int_str(&self, n: &BigInt) -> Result<(), StrfError> {
        int_str(self.0.py(), n)?;
        Ok(())
    }
}

/// The format string's code points, with `re.sub`'s errors for other types.
fn format_text(format: &Bound<'_, PyAny>) -> PyResult<Vec<u32>> {
    if let Ok(text) = format.cast::<PyString>() {
        return py_to_cps(text);
    }
    if format.is_instance_of::<PyBytes>()
        || format.is_instance_of::<PyByteArray>()
        || format.is_instance_of::<PyMemoryView>()
    {
        return Err(PyTypeError::new_err(
            "cannot use a string pattern on a bytes-like object",
        ));
    }
    Err(PyTypeError::new_err(format!(
        "expected string or bytes-like object, got '{}'",
        tp_name(format)?
    )))
}

fn target(obj: &Bound<'_, PyAny>) -> Target {
    if is_duration_instance(obj) {
        Target::Duration
    } else if obj.is_instance_of::<PyDelta>() {
        Target::TimeDelta
    } else {
        Target::DateTime
    }
}

fn run<'py>(
    tdt: &Bound<'py, PyAny>,
    format: &Bound<'py, PyAny>,
    yeardigits: &Num,
) -> PyResult<Bound<'py, PyString>> {
    let py = tdt.py();
    let target = target(tdt);
    let format = format_text(format)?;
    let out =
        core::strftime(&Source(tdt), target, &format, yeardigits).map_err(|e| e.into_py(py))?;
    cps_to_py(py, &out)
}

fn format_or<'py>(py: Python<'py>, format: Arg<'py>, default: &str) -> Bound<'py, PyAny> {
    match format {
        Arg::Given(f) => f,
        Arg::Default => PyString::new(py, default).into_any(),
    }
}

fn yeardigits_num(yeardigits: Arg<'_>) -> PyResult<Num> {
    match yeardigits {
        Arg::Given(y) => to_num(&y),
        Arg::Default => Ok(Num::Int(BigInt::from(4))),
    }
}

// ------------------------------------------------------------------ functions

/// `strftime(tdt, format, yeardigits=4)`.
#[pyfunction]
#[pyo3(signature = (tdt, format, yeardigits = Arg::Default))]
fn strftime<'py>(
    tdt: &Bound<'py, PyAny>,
    format: &Bound<'py, PyAny>,
    yeardigits: Arg<'py>,
) -> PyResult<Bound<'py, PyString>> {
    run(tdt, format, &yeardigits_num(yeardigits)?)
}

/// `date_isoformat(tdate, format=DATE_EXT_COMPLETE, yeardigits=4)`.
#[pyfunction]
#[pyo3(signature = (tdate, format = Arg::Default, yeardigits = Arg::Default))]
fn date_isoformat<'py>(
    tdate: &Bound<'py, PyAny>,
    format: Arg<'py>,
    yeardigits: Arg<'py>,
) -> PyResult<Bound<'py, PyString>> {
    let format = format_or(tdate.py(), format, core::DATE_EXT_COMPLETE);
    run(tdate, &format, &yeardigits_num(yeardigits)?)
}

/// `time_isoformat(ttime, format=TIME_EXT_COMPLETE + TZ_EXT)`.
#[pyfunction]
#[pyo3(signature = (ttime, format = Arg::Default))]
fn time_isoformat<'py>(
    ttime: &Bound<'py, PyAny>,
    format: Arg<'py>,
) -> PyResult<Bound<'py, PyString>> {
    let format = format_or(ttime.py(), format, "%H:%M:%S%Z");
    run(ttime, &format, &Num::Int(BigInt::from(4)))
}

/// `datetime_isoformat(tdt, format=DATE_EXT_COMPLETE + "T" + TIME_EXT_COMPLETE + TZ_EXT)`.
#[pyfunction]
#[pyo3(signature = (tdt, format = Arg::Default))]
fn datetime_isoformat<'py>(
    tdt: &Bound<'py, PyAny>,
    format: Arg<'py>,
) -> PyResult<Bound<'py, PyString>> {
    let format = format_or(tdt.py(), format, core::DT_EXT_COMPLETE);
    run(tdt, &format, &Num::Int(BigInt::from(4)))
}

/// `value < 0` for a Duration's `years`/`months`.
fn below_zero(value: &Bound<'_, PyAny>) -> PyResult<bool> {
    match to_num(value)? {
        Num::Other(_) => value.lt(0),
        num => core::is_negative(&num).py_err(value.py()),
    }
}

/// `td < timedelta(0)`.
fn delta_below_zero(td: &Bound<'_, PyAny>) -> PyResult<bool> {
    if td.is_instance_of::<PyDelta>() {
        let days: BigInt = td.getattr("days")?.extract()?;
        return Ok(days < BigInt::from(0));
    }
    td.lt(PyDelta::new(td.py(), 0, 0, 0, false)?)
}

/// `duration_isoformat(tduration, format=D_DEFAULT)`.
#[pyfunction]
#[pyo3(signature = (tduration, format = Arg::Default))]
fn duration_isoformat<'py>(
    tduration: &Bound<'py, PyAny>,
    format: Arg<'py>,
) -> PyResult<Bound<'py, PyString>> {
    let py = tduration.py();
    let negative = if is_duration_instance(tduration) {
        below_zero(&tduration.getattr("years")?)?
            || below_zero(&tduration.getattr("months")?)?
            || delta_below_zero(&tduration.getattr("tdelta")?)?
    } else {
        tduration.is_instance_of::<PyDelta>() && delta_below_zero(tduration)?
    };
    let format = format_or(py, format, core::D_DEFAULT);
    let text = run(tduration, &format, &Num::Int(BigInt::from(4)))?;
    if negative {
        let mut cps = vec![u32::from('-')];
        cps.extend(py_to_cps(&text)?);
        return cps_to_py(py, &cps);
    }
    Ok(text)
}

// ------------------------------------------------------------------ register

/// Adds this task's names to their submodules.
pub(crate) fn register(modules: &Modules<'_>) -> PyResult<()> {
    let py = modules.root.py();
    let tz_isoformat = modules.isotzinfo.getattr("tz_isoformat")?;
    let _ = TZ_ISOFORMAT.set(py, tz_isoformat.clone().unbind());

    let strf = &modules.isostrf;
    for &(name, value) in core::CONSTANTS {
        strf.add(name, value)?;
    }
    let strftime = wrap_pyfunction!(strftime, strf)?;
    strf.add_function(strftime.clone())?;
    strf.add("tz_isoformat", tz_isoformat)?;
    if let Ok(duration) = modules.duration.getattr("Duration") {
        strf.add("Duration", duration)?;
    }

    // Each wrapper's module also imports `strftime` and the constants it uses.
    let wrappers = [
        (&modules.isodates, &["DATE_EXT_COMPLETE"][..]),
        (&modules.isotime, &["TIME_EXT_COMPLETE", "TZ_EXT"][..]),
        (
            &modules.isodatetime,
            &["DATE_EXT_COMPLETE", "TIME_EXT_COMPLETE", "TZ_EXT"][..],
        ),
        (&modules.isoduration, &["D_DEFAULT"][..]),
    ];
    for (module, names) in wrappers {
        module.add("strftime", &strftime)?;
        for name in names {
            module.add(*name, strf.getattr(*name)?)?;
        }
    }
    modules
        .isodates
        .add_function(wrap_pyfunction!(date_isoformat, &modules.isodates)?)?;
    modules
        .isotime
        .add_function(wrap_pyfunction!(time_isoformat, &modules.isotime)?)?;
    modules
        .isodatetime
        .add_function(wrap_pyfunction!(datetime_isoformat, &modules.isodatetime)?)?;
    modules
        .isoduration
        .add_function(wrap_pyfunction!(duration_isoformat, &modules.isoduration)?)?;
    Ok(())
}
