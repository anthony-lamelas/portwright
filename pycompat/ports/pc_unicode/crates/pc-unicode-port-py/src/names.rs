//! Bindings for `char_name` and `lookup`.

use pc_unicode_port_core::call::Param;
use pc_unicode_port_core::names::{self, LookupError};
use pyo3::exceptions::PyUnicodeEncodeError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple};
use pyo3::IntoPyObjectExt;

use crate::convert::{bind_call, text_to_py, to_py_err, value_of, Arg};

const CH_DEFAULT: [Param; 2] = [Param::required("ch"), Param::optional("default")];
const NAME: [Param; 1] = [Param::required("name")];

#[pyfunction(signature = (*args, **kwargs))]
fn char_name<'py>(
    py: Python<'py>,
    args: &Bound<'py, PyTuple>,
    kwargs: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyAny>> {
    let mut slots = bind_call("char_name", &CH_DEFAULT, args, kwargs)?.into_iter();
    let ch: Option<Arg<'py>> = slots.next().flatten();
    let default: Option<Arg<'py>> = slots.next().flatten();
    let ch_value = value_of(&ch).unwrap_or(&pc_unicode_port_core::Value::None);
    match names::char_name(ch_value, value_of(&default)).map_err(to_py_err)? {
        Some(name) => name.into_bound_py_any(py),
        None => Ok(default.map_or_else(|| py.None().into_bound(py), |(obj, _)| obj)),
    }
}

#[pyfunction(signature = (*args, **kwargs))]
fn lookup<'py>(
    py: Python<'py>,
    args: &Bound<'py, PyTuple>,
    kwargs: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyAny>> {
    let slot: Option<Arg<'py>> = bind_call("lookup", &NAME, args, kwargs)?
        .into_iter()
        .next()
        .flatten();
    let (obj, value) = match slot {
        Some(arg) => arg,
        None => (py.None().into_bound(py), pc_unicode_port_core::Value::None),
    };
    match names::lookup(&value) {
        Ok(text) => Ok(text_to_py(py, &text)?.into_any()),
        Err(LookupError::Surrogates { start, end }) => Err(PyUnicodeEncodeError::new_err((
            "utf-8",
            obj.unbind(),
            start,
            end,
            LookupError::SURROGATE_REASON,
        ))),
        Err(LookupError::Python(e)) => Err(to_py_err(e)),
        Err(other) => Err(to_py_err(pc_unicode_port_core::Error::type_error(
            other.to_string(),
        ))),
    }
}

/// Adds this module's functions to `m`.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(char_name, m)?)?;
    m.add_function(wrap_pyfunction!(lookup, m)?)?;
    Ok(())
}
