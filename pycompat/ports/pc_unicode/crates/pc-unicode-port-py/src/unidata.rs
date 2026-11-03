//! Bindings for the per-character `unicodedata` properties.

use pc_unicode_port_core::call::Param;
use pc_unicode_port_core::{unidata, Result as CoreResult, Value};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple};
use pyo3::IntoPyObjectExt;

use crate::convert::{bind_call, to_py_err, value_of, Arg};

const CH: [Param; 1] = [Param::required("ch")];
const CH_DEFAULT: [Param; 2] = [Param::required("ch"), Param::optional("default")];

/// Binds the single `ch` argument of `name`.
fn bind_ch(
    name: &str,
    args: &Bound<'_, PyTuple>,
    kwargs: Option<&Bound<'_, PyDict>>,
) -> PyResult<Value> {
    let slots = bind_call(name, &CH, args, kwargs)?;
    slots
        .into_iter()
        .next()
        .flatten()
        .map(|(_, v)| v)
        .ok_or_else(|| to_py_err(pc_unicode_port_core::Error::type_error("missing ch")))
}

/// Calls a `(ch, default=None)` core function; returns the result or the original default.
fn with_default<'py, T>(
    py: Python<'py>,
    name: &str,
    args: &Bound<'py, PyTuple>,
    kwargs: Option<&Bound<'py, PyDict>>,
    f: impl FnOnce(&Value, Option<&Value>) -> CoreResult<Option<T>>,
) -> PyResult<Bound<'py, PyAny>>
where
    T: IntoPyObject<'py>,
{
    let mut slots = bind_call(name, &CH_DEFAULT, args, kwargs)?.into_iter();
    let ch: Option<Arg<'py>> = slots.next().flatten();
    let default: Option<Arg<'py>> = slots.next().flatten();
    let ch_value = value_of(&ch).unwrap_or(&Value::None);
    match f(ch_value, value_of(&default)).map_err(to_py_err)? {
        Some(v) => Ok(v.into_bound_py_any(py)?),
        None => Ok(default.map_or_else(|| py.None().into_bound(py), |(obj, _)| obj)),
    }
}

#[pyfunction(signature = (*args, **kwargs))]
fn unidata_version<'py>(
    py: Python<'py>,
    args: &Bound<'py, PyTuple>,
    kwargs: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyAny>> {
    bind_call("unidata_version", &[], args, kwargs)?;
    unidata::unidata_version().into_bound_py_any(py)
}

macro_rules! char_fn {
    ($name:ident) => {
        #[pyfunction(signature = (*args, **kwargs))]
        fn $name<'py>(
            py: Python<'py>,
            args: &Bound<'py, PyTuple>,
            kwargs: Option<&Bound<'py, PyDict>>,
        ) -> PyResult<Bound<'py, PyAny>> {
            let ch = bind_ch(stringify!($name), args, kwargs)?;
            unidata::$name(&ch)
                .map_err(to_py_err)?
                .into_bound_py_any(py)
        }
    };
}

char_fn!(category);
char_fn!(bidirectional);
char_fn!(combining);
char_fn!(east_asian_width);
char_fn!(mirrored);
char_fn!(decomposition);

#[pyfunction(signature = (*args, **kwargs))]
fn numeric_value<'py>(
    py: Python<'py>,
    args: &Bound<'py, PyTuple>,
    kwargs: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyAny>> {
    with_default(py, "numeric_value", args, kwargs, unidata::numeric_value)
}

#[pyfunction(signature = (*args, **kwargs))]
fn decimal_value<'py>(
    py: Python<'py>,
    args: &Bound<'py, PyTuple>,
    kwargs: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyAny>> {
    with_default(py, "decimal_value", args, kwargs, unidata::decimal_value)
}

#[pyfunction(signature = (*args, **kwargs))]
fn digit_value<'py>(
    py: Python<'py>,
    args: &Bound<'py, PyTuple>,
    kwargs: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyAny>> {
    with_default(py, "digit_value", args, kwargs, unidata::digit_value)
}

/// Adds this module's functions to `m`.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(unidata_version, m)?)?;
    m.add_function(wrap_pyfunction!(category, m)?)?;
    m.add_function(wrap_pyfunction!(bidirectional, m)?)?;
    m.add_function(wrap_pyfunction!(combining, m)?)?;
    m.add_function(wrap_pyfunction!(east_asian_width, m)?)?;
    m.add_function(wrap_pyfunction!(mirrored, m)?)?;
    m.add_function(wrap_pyfunction!(decomposition, m)?)?;
    m.add_function(wrap_pyfunction!(numeric_value, m)?)?;
    m.add_function(wrap_pyfunction!(decimal_value, m)?)?;
    m.add_function(wrap_pyfunction!(digit_value, m)?)?;
    Ok(())
}
