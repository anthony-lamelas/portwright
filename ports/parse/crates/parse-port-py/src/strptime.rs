//! `strf_date_convert` (task strptime).
//! (`get_regex_for_datetime_format` belongs to the `pyre` task; keep it.)

use parse_port_core::strptime::{self as core_strptime, Strptime};
use parse_port_core::types::{Temporal, TzInfo};
use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::{PyByteArray, PyBytes, PyDict, PyMemoryView, PyString, PyTuple};

use crate::convert::temporal_to_py;
use crate::errors::IntoPyResult;

/// `get_regex_for_datetime_format(format_)`.
#[pyfunction]
#[pyo3(signature = (format_))]
fn get_regex_for_datetime_format(format_: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    if let Ok(s) = format_.cast::<PyString>() {
        if let Ok(text) = s.to_str() {
            let out = parse_port_core::pyre::dt_format::get_regex_for_datetime_format(text);
            return Ok(PyString::new(format_.py(), &out).into_any().unbind());
        }
        // Lone surrogates: work on code points via UTF-32.
        let raw = s.call_method1("encode", ("utf-32-le", "surrogatepass"))?;
        let bytes: Vec<u8> = raw.extract()?;
        let cps: Vec<u32> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| u32::from_le_bytes(*b))
            .collect();
        let out = parse_port_core::pyre::dt_format::get_regex_for_datetime_format_codepoints(&cps);
        let out_bytes: Vec<u8> = out.iter().flat_map(|c| c.to_le_bytes()).collect();
        let py_bytes = PyBytes::new(format_.py(), &out_bytes);
        return Ok(py_bytes
            .call_method1("decode", ("utf-32-le", "surrogatepass"))?
            .unbind());
    }
    if format_.is_instance_of::<PyBytes>()
        || format_.is_instance_of::<PyByteArray>()
        || format_.is_instance_of::<PyMemoryView>()
    {
        return Err(PyTypeError::new_err(
            "cannot use a string pattern on a bytes-like object",
        ));
    }
    let tn = format_.get_type().name()?;
    Err(PyTypeError::new_err(format!(
        "expected string or bytes-like object, got '{tn}'"
    )))
}

/// `type(obj)` as C `PyArg_Parse` names it: `None`, a heap type's name, or
/// a static type's `tp_name` (e.g. `datetime.date`).
fn arg_type_name(obj: &Bound<'_, PyAny>) -> PyResult<String> {
    if obj.is_none() {
        return Ok("None".to_owned());
    }
    let ty = obj.get_type();
    let flags: u64 = ty.getattr("__flags__")?.extract()?;
    let name = ty.name()?.to_string();
    if flags & (1 << 9) != 0 {
        return Ok(name);
    }
    let module = ty.module()?.to_string();
    Ok(if module == "builtins" {
        name
    } else {
        format!("{module}.{name}")
    })
}

fn strptime_type_error(index: usize, obj: &Bound<'_, PyAny>) -> PyResult<PyErr> {
    Ok(PyTypeError::new_err(format!(
        "strptime() argument {index} must be str, not {}",
        arg_type_name(obj)?
    )))
}

/// Convert a core result, attaching `timezone(offset, name)` when `%Z` named it.
pub fn result_to_py(py: Python<'_>, r: &Strptime<Temporal>) -> PyResult<Py<PyAny>> {
    let obj = temporal_to_py(py, &r.value)?;
    let (Some(name), Some(TzInfo::Timezone(_))) = (
        &r.tzname,
        match &r.value {
            Temporal::DateTime(dt) => dt.time.tz.as_ref(),
            Temporal::Time(t) => t.tz.as_ref(),
            Temporal::Date(_) => None,
        },
    ) else {
        return Ok(obj);
    };
    let bound = obj.bind(py);
    let offset = bound.call_method0("utcoffset")?;
    let tz = py
        .import("datetime")?
        .getattr("timezone")?
        .call1((offset, name.as_str()))?;
    let kwargs = PyDict::new(py);
    kwargs.set_item("tzinfo", tz)?;
    Ok(bound.call_method("replace", (), Some(&kwargs))?.unbind())
}

/// Bind `(x, _, type)` like a Python function would (a Rust parameter
/// can't be named `_`), with CPython's argument error messages.
fn bind_args<'py>(
    args: &Bound<'py, PyTuple>,
    kwargs: Option<&Bound<'py, PyDict>>,
) -> PyResult<[Bound<'py, PyAny>; 3]> {
    const NAMES: [&str; 3] = ["x", "_", "type"];
    const FN: &str = "strf_date_convert()";
    if args.len() > 3 {
        return Err(PyTypeError::new_err(format!(
            "{FN} takes 3 positional arguments but {} were given",
            args.len()
        )));
    }
    let mut slots: [Option<Bound<'py, PyAny>>; 3] = [None, None, None];
    for (i, a) in args.iter().enumerate() {
        slots[i] = Some(a);
    }
    if let Some(kw) = kwargs {
        for (k, v) in kw.iter() {
            let key: String = k.str()?.to_string();
            let Some(i) = NAMES.iter().position(|&n| n == key) else {
                return Err(PyTypeError::new_err(format!(
                    "{FN} got an unexpected keyword argument '{key}'"
                )));
            };
            if slots[i].is_some() {
                return Err(PyTypeError::new_err(format!(
                    "{FN} got multiple values for argument '{key}'"
                )));
            }
            slots[i] = Some(v);
        }
    }
    let missing: Vec<String> = NAMES
        .iter()
        .zip(&slots)
        .filter(|(_, s)| s.is_none())
        .map(|(n, _)| format!("'{n}'"))
        .collect();
    if let [Some(a), Some(b), Some(c)] = slots {
        return Ok([a, b, c]);
    }
    let list = match missing.as_slice() {
        [a, b] => format!("{a} and {b}"),
        [rest @ .., last] if !rest.is_empty() => format!("{}, and {last}", rest.join(", ")),
        _ => missing.join(""),
    };
    let plural = if missing.len() == 1 {
        "argument"
    } else {
        "arguments"
    };
    Err(PyTypeError::new_err(format!(
        "{FN} missing {} required positional {plural}: {list}",
        missing.len()
    )))
}

/// `obj += other` for a str `other`: `__iadd__`, then `__add__`, then
/// CPython's "unsupported operand" TypeError (str has no `__radd__`).
fn inplace_add<'py>(obj: &Bound<'py, PyAny>, other: &str) -> PyResult<Bound<'py, PyAny>> {
    let py = obj.py();
    let ty = obj.get_type();
    for slot in ["__iadd__", "__add__"] {
        if let Ok(method) = ty.getattr(slot) {
            let r = method.call1((obj, other))?;
            if !r.is(py.NotImplemented()) {
                return Ok(r);
            }
        }
    }
    Err(PyTypeError::new_err(format!(
        "unsupported operand type(s) for +=: '{}' and 'str'",
        ty.name()?
    )))
}

/// `strf_date_convert(x, _, type)`.
#[pyfunction]
#[pyo3(signature = (*args, **kwargs), text_signature = "(x, _, type)")]
fn strf_date_convert(
    py: Python<'_>,
    args: &Bound<'_, PyTuple>,
    kwargs: Option<&Bound<'_, PyDict>>,
) -> PyResult<Py<PyAny>> {
    let [x, _, r#type] = bind_args(args, kwargs)?;
    let (x, r#type) = (&x, &r#type);
    if let (Ok(xs), Ok(ts)) = (x.cast::<PyString>(), r#type.cast::<PyString>()) {
        let r = core_strptime::strf_date_convert(xs.to_str()?, ts.to_str()?).py(py)?;
        return result_to_py(py, &r);
    }
    // Slow path for non-str arguments: reproduce Python's operations
    // (`in`, `%`, `+=`) on the objects themselves, for the same errors.
    let mut format = r#type.clone();
    let mut data = x.clone();
    let mut is_date = false;
    for c in "aAwdbBmyYjUW".chars() {
        if format.contains(format!("%{c}"))? {
            is_date = true;
            break;
        }
    }
    let mut is_time = false;
    for c in "HIpMSfz".chars() {
        if format.contains(format!("%{c}"))? {
            is_time = true;
            break;
        }
    }
    if !format.contains("%y")? && !format.contains("%Y")? {
        let year = parse_port_core::types::current_year();
        data = PyString::new(py, &format!("{} {year}", x.str()?)).into_any();
        format = inplace_add(&format, " %Y")?;
    }
    let Ok(ds) = data.cast::<PyString>() else {
        return Err(strptime_type_error(1, &data)?);
    };
    let Ok(fs) = format.cast::<PyString>() else {
        return Err(strptime_type_error(2, &format)?);
    };
    // Both are str now, so the format already had a year directive.
    let r = core_strptime::strptime(ds.to_str()?, fs.to_str()?).py(py)?;
    let value = match (is_date, is_time) {
        (true, true) => Temporal::DateTime(r.value),
        (true, false) => Temporal::Date(r.value.date),
        (false, true) => Temporal::Time(r.value.time),
        (false, false) => {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "Datetime not a date nor a time?",
            ))
        }
    };
    result_to_py(
        py,
        &Strptime {
            value,
            tzname: r.tzname,
        },
    )
}

/// Adds this task's public objects to the `parse` module.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(get_regex_for_datetime_format, m)?)?;
    m.add_function(wrap_pyfunction!(strf_date_convert, m)?)?;
    Ok(())
}
