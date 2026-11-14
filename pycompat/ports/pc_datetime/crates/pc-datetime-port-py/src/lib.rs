//! Thin Python layer over pc-datetime-port-core: argument conversion, result conversion
//! and exceptions only. Every behavior, including messages, lives in the core.
#![forbid(unsafe_code)]

mod convert;

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple};

use pc_datetime_port_core as core_port;

/// Defines `#[pyfunction]`s taking `*args, **kwargs` that forward to the core.
macro_rules! shim_functions {
    ($($module:ident :: $name:ident),* $(,)?) => {
        $(
            #[pyfunction]
            #[pyo3(signature = (*args, **kwargs))]
            fn $name<'py>(
                py: Python<'py>,
                args: &Bound<'py, PyTuple>,
                kwargs: Option<&Bound<'py, PyDict>>,
            ) -> PyResult<Bound<'py, PyAny>> {
                let (args, kwargs) = convert::call_args(args, kwargs)?;
                match core_port::$module::$name(&args, &kwargs) {
                    Ok(value) => convert::to_py(py, &value),
                    Err(err) => Err(convert::to_py_err(py, err)),
                }
            }
        )*

        #[pymodule(name = "pc_datetime")]
        fn module_init(module: &Bound<'_, PyModule>) -> PyResult<()> {
            $(module.add_function(wrap_pyfunction!($name, module)?)?;)*
            Ok(())
        }
    };
}

shim_functions!(
    timedelta::make_timedelta,
    timedelta::timedelta_str,
    timedelta::timedelta_repr,
    timedelta::timedelta_total_seconds,
    timedelta::timedelta_multiply,
    timedelta::timedelta_divide,
    timedelta::timedelta_floor_divide,
    timedelta::timezone_name,
    date::make_date,
    date::date_toordinal,
    date::date_fromordinal,
    date::weekday,
    date::isoweekday,
    date::isocalendar,
    date::date_fromisocalendar,
    date::date_add,
    date::date_isoformat,
    datetime::make_time,
    datetime::make_datetime,
    datetime::datetime_add,
    datetime::datetime_subtract,
    datetime::datetime_compare,
    datetime::datetime_str,
    datetime::datetime_repr,
    datetime::datetime_isoformat,
    datetime::time_isoformat,
    timestamp::datetime_timestamp,
    timestamp::datetime_fromtimestamp,
    timestamp::datetime_astimezone,
    isoparse::date_fromisoformat,
    isoparse::time_fromisoformat,
    isoparse::datetime_fromisoformat,
    strftime::strftime,
    strptime::strptime,
);
