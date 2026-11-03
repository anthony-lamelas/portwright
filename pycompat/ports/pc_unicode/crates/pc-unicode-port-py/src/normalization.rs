//! Bindings for `unicodedata.normalize` / `unicodedata.is_normalized`.

use pc_unicode_port_core::call::Param;
use pc_unicode_port_core::{normalization, Value};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyString, PyTuple};

use crate::convert::{bind_call, text_to_py, to_py_err};

const FORM_S: [Param; 2] = [Param::required("form"), Param::required("s")];

/// Binds `(form, s)` for `name`.
fn bind_form_s(
    name: &str,
    args: &Bound<'_, PyTuple>,
    kwargs: Option<&Bound<'_, PyDict>>,
) -> PyResult<(Value, Value)> {
    let mut slots = bind_call(name, &FORM_S, args, kwargs)?
        .into_iter()
        .map(|slot| slot.map_or(Value::None, |(_, v)| v));
    let form = slots.next().unwrap_or(Value::None);
    let s = slots.next().unwrap_or(Value::None);
    Ok((form, s))
}

#[pyfunction(signature = (*args, **kwargs))]
fn normalize<'py>(
    py: Python<'py>,
    args: &Bound<'py, PyTuple>,
    kwargs: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyString>> {
    let (form, s) = bind_form_s("normalize", args, kwargs)?;
    let out = normalization::normalize(&form, &s).map_err(to_py_err)?;
    text_to_py(py, &out)
}

#[pyfunction(signature = (*args, **kwargs))]
fn is_normalized<'py>(
    py: Python<'py>,
    args: &Bound<'py, PyTuple>,
    kwargs: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyBool>> {
    let (form, s) = bind_form_s("is_normalized", args, kwargs)?;
    let out = normalization::is_normalized(&form, &s).map_err(to_py_err)?;
    Ok(PyBool::new(py, out).to_owned())
}

/// Adds this module's functions to `m`.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(normalize, m)?)?;
    m.add_function(wrap_pyfunction!(is_normalized, m)?)?;
    Ok(())
}
