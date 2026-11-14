//! Converting arguments to core [`Value`]s, results back to Python, and core errors to
//! exceptions. Nothing here runs datetime logic.

use num_bigint::BigInt;
use pc_datetime_port_core::{PyError, PyStr, Value};
use pyo3::exceptions::{
    PyAttributeError, PyIndexError, PyKeyError, PyOSError, PyOverflowError, PyRecursionError,
    PyTypeError, PyValueError, PyZeroDivisionError,
};
use pyo3::prelude::*;
use pyo3::type_object::PyTypeInfo;
use pyo3::types::{
    PyBool, PyByteArray, PyBytes, PyDict, PyFloat, PyFrozenSet, PyInt, PyList, PySet, PyString,
    PyTuple,
};

/// Containers nested deeper than this raise RecursionError, as the shim's recursive check would.
const MAX_DEPTH: usize = 900;

/// Converts an argument by its exact type. Subclasses of built-ins become `Value::Other`,
/// as the shim's `type(value) in _BUILTIN_TYPES` check treats them.
pub fn to_value(obj: &Bound<'_, PyAny>) -> PyResult<Value> {
    to_value_at(obj, 0)
}

fn to_value_at(obj: &Bound<'_, PyAny>, depth: usize) -> PyResult<Value> {
    if depth > MAX_DEPTH {
        return Err(PyRecursionError::new_err(
            "maximum recursion depth exceeded",
        ));
    }
    if obj.is_none() {
        return Ok(Value::None);
    }
    // One read of the type pointer, compared against each exact built-in type; this is
    // what `is_exact_instance_of` checks, without fetching the type objects each time.
    let py = obj.py();
    let ty = obj.get_type_ptr();
    let is = |raw: *mut pyo3::ffi::PyTypeObject| std::ptr::eq(ty, raw);
    let items = |iter: Bound<'_, pyo3::types::PyIterator>| -> PyResult<Vec<Value>> {
        iter.map(|item| to_value_at(&item?, depth + 1)).collect()
    };
    Ok(if is(PyInt::type_object_raw(py)) {
        // Most ints fit a C long; PyLong_AsLongLong is far cheaper than the byte-array path.
        match obj.extract::<i64>() {
            Ok(n) => Value::Int(BigInt::from(n)),
            Err(_) => Value::Int(obj.extract::<BigInt>()?),
        }
    } else if is(PyTuple::type_object_raw(py)) {
        // A tuple's items, read in place: the same items, in order, as iterating it,
        // without a reference count change per item.
        let items = obj.cast::<PyTuple>()?.as_slice();
        let mut out = Vec::with_capacity(items.len());
        for item in items {
            out.push(to_value_at(item, depth + 1)?);
        }
        Value::Tuple(out)
    } else if is(PyList::type_object_raw(py)) {
        let list = obj.cast::<PyList>()?;
        let mut out = Vec::with_capacity(list.len());
        for item in list.iter() {
            out.push(to_value_at(&item, depth + 1)?);
        }
        Value::List(out)
    } else if is(PyBool::type_object_raw(py)) {
        Value::Bool(obj.extract::<bool>()?)
    } else if is(PyFloat::type_object_raw(py)) {
        Value::Float(obj.extract::<f64>()?)
    } else if is(PyString::type_object_raw(py)) {
        Value::Str(str_to_core(obj.cast::<PyString>()?)?)
    } else if is(PyBytes::type_object_raw(py)) {
        Value::Bytes(obj.cast::<PyBytes>()?.as_bytes().to_vec())
    } else if is(PyByteArray::type_object_raw(py)) {
        Value::ByteArray(obj.cast::<PyByteArray>()?.to_vec())
    } else if is(PySet::type_object_raw(py)) {
        Value::Set(items(obj.try_iter()?)?)
    } else if is(PyFrozenSet::type_object_raw(py)) {
        Value::FrozenSet(items(obj.try_iter()?)?)
    } else if is(PyDict::type_object_raw(py)) {
        let dict = obj.cast::<PyDict>()?;
        let mut pairs = Vec::with_capacity(dict.len());
        for (key, item) in dict.iter() {
            pairs.push((
                to_value_at(&key, depth + 1)?,
                to_value_at(&item, depth + 1)?,
            ));
        }
        Value::Dict(pairs)
    } else {
        Value::Other(obj.get_type().name()?.to_string())
    })
}

/// Reads a `str`, keeping lone surrogates. The UTF-32 encode is only a lossless
/// conversion; pyo3 has no safe accessor for strings that aren't valid UTF-8.
fn str_to_core(s: &Bound<'_, PyString>) -> PyResult<PyStr> {
    if let Ok(text) = s.to_str() {
        // Same code points as `PyStr::from(text)`, collected into a pre-sized buffer.
        let mut points = Vec::with_capacity(text.len());
        if text.is_ascii() {
            points.extend(text.bytes().map(u32::from));
        } else {
            points.extend(text.chars().map(u32::from));
        }
        return Ok(PyStr::from_code_points(points));
    }
    let encoded = s.call_method1("encode", ("utf-32-le", "surrogatepass"))?;
    let bytes = encoded.cast::<PyBytes>()?.as_bytes();
    let points = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect();
    Ok(PyStr::from_code_points(points))
}

/// Builds a Python `str`, keeping lone surrogates.
fn str_to_py<'py>(py: Python<'py>, s: &PyStr) -> PyResult<Bound<'py, PyAny>> {
    let points = s.code_points();
    if points.iter().all(|&c| c < 0x80) {
        // ASCII: one byte per code point, so the bytes are the UTF-8 text.
        let bytes: Vec<u8> = points
            .iter()
            .map(|&c| u8::try_from(c).unwrap_or(b'?'))
            .collect();
        if let Ok(text) = std::str::from_utf8(&bytes) {
            return Ok(PyString::new(py, text).into_any());
        }
    }
    let mut text = String::with_capacity(points.len());
    let valid = points.iter().all(|&c| match char::from_u32(c) {
        Some(ch) => {
            text.push(ch);
            true
        }
        None => false,
    });
    if valid {
        return Ok(PyString::new(py, &text).into_any());
    }
    let bytes: Vec<u8> = s
        .code_points()
        .iter()
        .flat_map(|c| c.to_le_bytes())
        .collect();
    let bytes = PyBytes::new(py, &bytes);
    Ok(
        PyString::from_encoded_object(&bytes, Some(c"utf-32-le"), Some(c"surrogatepass"))?
            .into_any(),
    )
}

/// Converts a core result value to a Python object.
pub fn to_py<'py>(py: Python<'py>, value: &Value) -> PyResult<Bound<'py, PyAny>> {
    let seq = |items: &[Value]| -> PyResult<Vec<Bound<'py, PyAny>>> {
        let mut out = Vec::with_capacity(items.len());
        for item in items {
            out.push(to_py(py, item)?);
        }
        Ok(out)
    };
    Ok(match value {
        Value::Int(n) => match i64::try_from(n) {
            Ok(small) => small.into_pyobject(py)?.into_any(),
            Err(_) => n.into_pyobject(py)?.into_any(),
        },
        Value::None => py.None().into_bound(py),
        Value::Tuple(items) => PyTuple::new(py, seq(items)?)?.into_any(),
        Value::Bool(b) => PyBool::new(py, *b).to_owned().into_any(),
        Value::Float(f) => PyFloat::new(py, *f).into_any(),
        Value::Str(s) => str_to_py(py, s)?,
        Value::Bytes(b) => PyBytes::new(py, b).into_any(),
        Value::ByteArray(b) => PyByteArray::new(py, b).into_any(),
        Value::List(items) => PyList::new(py, seq(items)?)?.into_any(),
        Value::Set(items) => PySet::new(py, seq(items)?)?.into_any(),
        Value::FrozenSet(items) => PyFrozenSet::new(py, seq(items)?)?.into_any(),
        Value::Dict(pairs) => {
            let dict = PyDict::new(py);
            for (key, item) in pairs {
                dict.set_item(to_py(py, key)?, to_py(py, item)?)?;
            }
            dict.into_any()
        }
        Value::Other(name) => {
            return Err(PyTypeError::new_err(format!("cannot return a {name}")));
        }
    })
}

/// Exception classes the core raises that aren't built-ins.
mod foreign {
    #![allow(non_camel_case_types, reason = "the Python class is named `re.error`")]

    pyo3::create_exception!(re, error, pyo3::exceptions::PyException);
}

/// Raises the exception a core error describes, with the same single argument.
pub fn to_py_err(py: Python<'_>, err: PyError) -> PyErr {
    let message = |m: &PyStr| str_to_py(py, m).map(Bound::unbind);
    let result = match &err {
        PyError::TypeError(m) => message(m).map(PyTypeError::new_err),
        PyError::ValueError(m) => message(m).map(PyValueError::new_err),
        PyError::OverflowError(m) => message(m).map(PyOverflowError::new_err),
        PyError::ZeroDivisionError(m) => message(m).map(PyZeroDivisionError::new_err),
        PyError::IndexError(m) => message(m).map(PyIndexError::new_err),
        PyError::RecursionError(m) => message(m).map(PyRecursionError::new_err),
        PyError::AttributeError(m) => message(m).map(PyAttributeError::new_err),
        PyError::ReError(m) => message(m).map(foreign::error::new_err),
        PyError::OsError { errno, strerror } => {
            message(strerror).map(|m| PyOSError::new_err((*errno, m)))
        }
        PyError::KeyError(key) => to_py(py, key).map(|k| PyKeyError::new_err(k.unbind())),
    };
    result.unwrap_or_else(|e| e)
}

/// Positional and keyword arguments as core values.
pub type CallArgs = (Vec<Value>, Vec<(PyStr, Value)>);

/// Converts `*args` and `**kwargs` to core values.
pub fn call_args(
    args: &Bound<'_, PyTuple>,
    kwargs: Option<&Bound<'_, PyDict>>,
) -> PyResult<CallArgs> {
    let items = args.as_slice();
    let mut positional = Vec::with_capacity(items.len());
    for item in items {
        positional.push(to_value(item)?);
    }
    let mut keywords = Vec::with_capacity(kwargs.map_or(0, |k| k.len()));
    if let Some(kwargs) = kwargs {
        for (key, item) in kwargs.iter() {
            // Lossless: binding errors echo the name, lone surrogates included.
            let key = str_to_core(key.cast::<PyString>()?)?;
            keywords.push((key, to_value(&item)?));
        }
    }
    Ok((positional, keywords))
}
