//! Bindings for `str` case conversions and predicates.

use pc_unicode_port_core::call::Param;
use pc_unicode_port_core::{casing, Value};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyString, PyTuple};

use crate::convert::{bind_call, text_to_py, to_py_err};

const S: [Param; 1] = [Param::required("s")];

/// Binds the single `s` argument of `name`.
fn bind_s(
    name: &str,
    args: &Bound<'_, PyTuple>,
    kwargs: Option<&Bound<'_, PyDict>>,
) -> PyResult<Value> {
    let slots = bind_call(name, &S, args, kwargs)?;
    Ok(slots
        .into_iter()
        .next()
        .flatten()
        .map_or(Value::None, |(_, v)| v))
}

macro_rules! text_fn {
    ($($name:ident),* $(,)?) => {$(
        #[pyfunction(signature = (*args, **kwargs))]
        fn $name<'py>(
            py: Python<'py>,
            args: &Bound<'py, PyTuple>,
            kwargs: Option<&Bound<'py, PyDict>>,
        ) -> PyResult<Bound<'py, PyString>> {
            let s = bind_s(stringify!($name), args, kwargs)?;
            let out = casing::$name(&s).map_err(to_py_err)?;
            text_to_py(py, &out)
        }
    )*};
}

macro_rules! bool_fn {
    ($($name:ident),* $(,)?) => {$(
        #[pyfunction(signature = (*args, **kwargs))]
        fn $name<'py>(
            py: Python<'py>,
            args: &Bound<'py, PyTuple>,
            kwargs: Option<&Bound<'py, PyDict>>,
        ) -> PyResult<Bound<'py, PyBool>> {
            let s = bind_s(stringify!($name), args, kwargs)?;
            let out = casing::$name(&s).map_err(to_py_err)?;
            Ok(PyBool::new(py, out).to_owned())
        }
    )*};
}

text_fn!(upper, lower, casefold, title, capitalize, swapcase);
bool_fn!(
    isalpha,
    isdigit,
    isdecimal,
    isnumeric,
    isalnum,
    isspace,
    isupper,
    islower,
    istitle,
    isidentifier,
    isprintable,
    isascii,
);

/// Adds this task's functions to the module.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(upper, m)?)?;
    m.add_function(wrap_pyfunction!(lower, m)?)?;
    m.add_function(wrap_pyfunction!(casefold, m)?)?;
    m.add_function(wrap_pyfunction!(title, m)?)?;
    m.add_function(wrap_pyfunction!(capitalize, m)?)?;
    m.add_function(wrap_pyfunction!(swapcase, m)?)?;
    m.add_function(wrap_pyfunction!(isalpha, m)?)?;
    m.add_function(wrap_pyfunction!(isdigit, m)?)?;
    m.add_function(wrap_pyfunction!(isdecimal, m)?)?;
    m.add_function(wrap_pyfunction!(isnumeric, m)?)?;
    m.add_function(wrap_pyfunction!(isalnum, m)?)?;
    m.add_function(wrap_pyfunction!(isspace, m)?)?;
    m.add_function(wrap_pyfunction!(isupper, m)?)?;
    m.add_function(wrap_pyfunction!(islower, m)?)?;
    m.add_function(wrap_pyfunction!(istitle, m)?)?;
    m.add_function(wrap_pyfunction!(isidentifier, m)?)?;
    m.add_function(wrap_pyfunction!(isprintable, m)?)?;
    m.add_function(wrap_pyfunction!(isascii, m)?)?;
    Ok(())
}
