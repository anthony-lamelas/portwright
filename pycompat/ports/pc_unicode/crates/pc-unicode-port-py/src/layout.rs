//! Bindings for `str` splitting, stripping, padding and tab expansion.

use pc_unicode_port_core::call::Param;
use pc_unicode_port_core::{layout, Value};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyString, PyTuple};

use crate::convert::{bind_call, text_to_py, texts_to_py, to_py_err, value_of, Arg};

const SPLIT: [Param; 3] = [
    Param::required("s"),
    Param::optional("sep"),
    Param::optional("maxsplit"),
];
const SPLITLINES: [Param; 2] = [Param::required("s"), Param::optional("keepends")];
const STRIP: [Param; 2] = [Param::required("s"), Param::optional("chars")];
const JUSTIFY: [Param; 3] = [
    Param::required("s"),
    Param::required("width"),
    Param::optional("fillchar"),
];
const ZFILL: [Param; 2] = [Param::required("s"), Param::required("width")];
const EXPANDTABS: [Param; 2] = [Param::required("s"), Param::optional("tabsize")];

/// The value in slot `i`, if supplied.
fn slot<'a>(slots: &'a [Option<Arg<'_>>], i: usize) -> Option<&'a Value> {
    slots.get(i).and_then(value_of)
}

/// A required slot; binding has already rejected missing required arguments.
fn required<'a>(slots: &'a [Option<Arg<'_>>], i: usize) -> &'a Value {
    slot(slots, i).unwrap_or(&Value::None)
}

macro_rules! split_fn {
    ($($name:ident => $core:ident),*) => {$(
        #[pyfunction(signature = (*args, **kwargs))]
        fn $name<'py>(
            py: Python<'py>,
            args: &Bound<'py, PyTuple>,
            kwargs: Option<&Bound<'py, PyDict>>,
        ) -> PyResult<Bound<'py, PyList>> {
            let slots = bind_call(stringify!($name), &SPLIT, args, kwargs)?;
            let out = layout::$core(required(&slots, 0), slot(&slots, 1), slot(&slots, 2))
                .map_err(to_py_err)?;
            texts_to_py(py, &out)
        }
    )*};
}

macro_rules! strip_fn {
    ($($name:ident),*) => {$(
        #[pyfunction(signature = (*args, **kwargs))]
        fn $name<'py>(
            py: Python<'py>,
            args: &Bound<'py, PyTuple>,
            kwargs: Option<&Bound<'py, PyDict>>,
        ) -> PyResult<Bound<'py, PyString>> {
            let slots = bind_call(stringify!($name), &STRIP, args, kwargs)?;
            let out = layout::$name(required(&slots, 0), slot(&slots, 1)).map_err(to_py_err)?;
            text_to_py(py, &out)
        }
    )*};
}

macro_rules! justify_fn {
    ($($name:ident),*) => {$(
        #[pyfunction(signature = (*args, **kwargs))]
        fn $name<'py>(
            py: Python<'py>,
            args: &Bound<'py, PyTuple>,
            kwargs: Option<&Bound<'py, PyDict>>,
        ) -> PyResult<Bound<'py, PyString>> {
            let slots = bind_call(stringify!($name), &JUSTIFY, args, kwargs)?;
            let out = layout::$name(required(&slots, 0), required(&slots, 1), slot(&slots, 2))
                .map_err(to_py_err)?;
            text_to_py(py, &out)
        }
    )*};
}

split_fn!(split => split_slices, rsplit => rsplit_slices);
strip_fn!(strip, lstrip, rstrip);
justify_fn!(center, ljust, rjust);

#[pyfunction(signature = (*args, **kwargs))]
fn splitlines<'py>(
    py: Python<'py>,
    args: &Bound<'py, PyTuple>,
    kwargs: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyList>> {
    let slots = bind_call("splitlines", &SPLITLINES, args, kwargs)?;
    let out = layout::splitlines_slices(required(&slots, 0), slot(&slots, 1)).map_err(to_py_err)?;
    texts_to_py(py, &out)
}

#[pyfunction(signature = (*args, **kwargs))]
fn zfill<'py>(
    py: Python<'py>,
    args: &Bound<'py, PyTuple>,
    kwargs: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyString>> {
    let slots = bind_call("zfill", &ZFILL, args, kwargs)?;
    let out = layout::zfill(required(&slots, 0), required(&slots, 1)).map_err(to_py_err)?;
    text_to_py(py, &out)
}

#[pyfunction(signature = (*args, **kwargs))]
fn expandtabs<'py>(
    py: Python<'py>,
    args: &Bound<'py, PyTuple>,
    kwargs: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyString>> {
    let slots = bind_call("expandtabs", &EXPANDTABS, args, kwargs)?;
    let out = layout::expandtabs(required(&slots, 0), slot(&slots, 1)).map_err(to_py_err)?;
    text_to_py(py, &out)
}

/// Adds this task's functions to the module.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(split, m)?)?;
    m.add_function(wrap_pyfunction!(rsplit, m)?)?;
    m.add_function(wrap_pyfunction!(splitlines, m)?)?;
    m.add_function(wrap_pyfunction!(strip, m)?)?;
    m.add_function(wrap_pyfunction!(lstrip, m)?)?;
    m.add_function(wrap_pyfunction!(rstrip, m)?)?;
    m.add_function(wrap_pyfunction!(center, m)?)?;
    m.add_function(wrap_pyfunction!(ljust, m)?)?;
    m.add_function(wrap_pyfunction!(rjust, m)?)?;
    m.add_function(wrap_pyfunction!(zfill, m)?)?;
    m.add_function(wrap_pyfunction!(expandtabs, m)?)?;
    Ok(())
}
