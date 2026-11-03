//! Thin Python layer over pc-format-port-core.
//!
//! It only converts arguments to [`Value`]s, calls the core, converts the
//! result back and raises the core's errors. Every behavior (reprs,
//! messages, the builtin guard) lives in the core. Strict bindings: no
//! `repr()`, `str()`, attribute reads or method calls on Python objects
//! (lone surrogates cross through the C API, see `py_str_to_core`).
// Unsafe is limited to the two C-API string copies that carry lone
// surrogates across without running Python code on the objects.
#![deny(unsafe_op_in_unsafe_fn)]

use num_bigint::BigInt;
use pc_format_port_core as core;
use pc_format_port_core::{ExcKind, PcResult, PyError, PyStr, Value};
use pyo3::exceptions::{
    PyAttributeError, PyIndexError, PyKeyError, PyMemoryError, PyNotImplementedError,
    PyOverflowError, PyRecursionError, PyTypeError, PyValueError, PyZeroDivisionError,
};
use pyo3::ffi;
use pyo3::prelude::*;
use pyo3::types::{
    PyBool, PyByteArray, PyBytes, PyDict, PyFloat, PyFrozenSet, PyInt, PyList, PyNone, PySet,
    PyString, PyTuple,
};

// ---------------------------------------------------------------- Python -> Value

/// Reads a `str` as code points, keeping lone surrogates.
fn py_str_to_core(s: &Bound<'_, PyString>) -> PyResult<PyStr> {
    if let Ok(text) = s.to_str() {
        // Pre-sized: `PyStr::from(&str)` grows its buffer from the chars'
        // size hint, which reallocates several times for longer text.
        let mut code_points = Vec::with_capacity(text.len());
        if text.is_ascii() {
            code_points.extend(text.bytes().map(u32::from));
        } else {
            code_points.extend(text.chars().map(u32::from));
        }
        return Ok(PyStr::from_code_points(code_points));
    }
    // Lone surrogates: copy the UCS-4 buffer through the C API rather than
    // calling `str.encode`, which would run CPython code on the object.
    let len = s.len()?;
    // SAFETY: `s` is a live `str`; `PyUnicode_AsUCS4Copy` returns a new
    // `PyMem_Malloc` buffer of `len + 1` code points (or null with an error
    // set), which is read once and freed here.
    let code_points = unsafe {
        let buf = ffi::PyUnicode_AsUCS4Copy(s.as_ptr());
        if buf.is_null() {
            return Err(PyErr::fetch(s.py()));
        }
        let out = std::slice::from_raw_parts(buf, len).to_vec();
        ffi::PyMem_Free(buf.cast());
        out
    };
    Ok(PyStr::from_code_points(code_points))
}

/// Containers nested deeper than this raise `RecursionError`, like
/// upstream's recursive `_check_builtin` under the default recursion limit
/// (about 1000 frames). Cyclic containers end up here too.
const MAX_DEPTH: usize = 900;

/// Depth-first conversion state, in `_check_builtin`'s visiting order.
#[derive(Default)]
struct Converter {
    depth: usize,
    /// A non-built-in was already seen: upstream raises its `TypeError`
    /// before reaching anything deeper, so depth no longer matters.
    saw_foreign: bool,
}

impl Converter {
    fn items<'py>(
        &mut self,
        iter: impl Iterator<Item = Bound<'py, PyAny>>,
    ) -> PyResult<Vec<Value>> {
        iter.map(|item| self.convert(&item)).collect()
    }

    fn convert(&mut self, obj: &Bound<'_, PyAny>) -> PyResult<Value> {
        if self.depth >= MAX_DEPTH {
            if self.saw_foreign {
                // The core's guard reports the earlier foreign value first.
                return Ok(Value::None);
            }
            return Err(PyRecursionError::new_err(
                "maximum recursion depth exceeded",
            ));
        }
        self.depth += 1;
        let value = to_core_with(obj, self);
        self.depth -= 1;
        let value = value?;
        if matches!(value, Value::Foreign(_)) {
            self.saw_foreign = true;
        }
        Ok(value)
    }
}

/// Converts a Python object to a [`Value`]. Only exact built-in types are
/// converted; anything else (including subclasses) becomes
/// [`Value::Foreign`] carrying `type(obj).__name__`.
fn to_core(obj: &Bound<'_, PyAny>) -> PyResult<Value> {
    Converter::default().convert(obj)
}

/// Converts the arguments of one call, in order, sharing the depth state.
fn args_to_core<const N: usize>(objs: [&Bound<'_, PyAny>; N]) -> PyResult<[Value; N]> {
    let mut conv = Converter::default();
    let mut out: [Value; N] = std::array::from_fn(|_| Value::None);
    for (slot, obj) in out.iter_mut().zip(objs) {
        *slot = conv.convert(obj)?;
    }
    Ok(out)
}

/// Reads an exact `int`. Values that fit an `i64` skip the byte-array
/// conversion; `PyLong_AsLongLongAndOverflow` reports overflow through a
/// flag instead of raising, so bigger ints don't pay for an exception.
fn int_to_core(obj: &Bound<'_, PyAny>) -> PyResult<BigInt> {
    let mut overflow: std::os::raw::c_int = 0;
    // SAFETY: `obj` is a live exact `int`; the call only reads it and
    // writes the overflow flag.
    let small = unsafe { ffi::PyLong_AsLongLongAndOverflow(obj.as_ptr(), &mut overflow) };
    if overflow == 0 {
        if small == -1 {
            if let Some(err) = PyErr::take(obj.py()) {
                return Err(err);
            }
        }
        return Ok(BigInt::from(small));
    }
    obj.extract::<BigInt>()
}

fn to_core_with(obj: &Bound<'_, PyAny>, conv: &mut Converter) -> PyResult<Value> {
    Ok(if obj.is_none() {
        Value::None
    } else if obj.is_exact_instance_of::<PyBool>() {
        Value::Bool(obj.extract::<bool>()?)
    } else if obj.is_exact_instance_of::<PyInt>() {
        Value::Int(int_to_core(obj)?)
    } else if obj.is_exact_instance_of::<PyFloat>() {
        Value::Float(obj.extract::<f64>()?)
    } else if obj.is_exact_instance_of::<PyString>() {
        Value::Str(py_str_to_core(obj.cast::<PyString>()?)?)
    } else if obj.is_exact_instance_of::<PyBytes>() {
        Value::Bytes(obj.cast::<PyBytes>()?.as_bytes().to_vec())
    } else if obj.is_exact_instance_of::<PyByteArray>() {
        Value::ByteArray(obj.cast::<PyByteArray>()?.to_vec())
    } else if obj.is_exact_instance_of::<PyTuple>() {
        Value::Tuple(conv.items(obj.cast::<PyTuple>()?.iter())?)
    } else if obj.is_exact_instance_of::<PyList>() {
        Value::List(conv.items(obj.cast::<PyList>()?.iter())?)
    } else if obj.is_exact_instance_of::<PySet>() {
        Value::Set(conv.items(obj.cast::<PySet>()?.iter())?)
    } else if obj.is_exact_instance_of::<PyFrozenSet>() {
        Value::FrozenSet(conv.items(obj.cast::<PyFrozenSet>()?.iter())?)
    } else if obj.is_exact_instance_of::<PyDict>() {
        let pairs = obj
            .cast::<PyDict>()?
            .iter()
            .map(|(k, v)| Ok((conv.convert(&k)?, conv.convert(&v)?)))
            .collect::<PyResult<Vec<_>>>()?;
        Value::Dict(pairs)
    } else {
        let name = obj.get_type().name()?;
        Value::Foreign(name.to_str()?.to_owned())
    })
}

// ---------------------------------------------------------------- Value -> Python

fn core_str_to_py<'py>(py: Python<'py>, s: &PyStr) -> PyResult<Bound<'py, PyAny>> {
    // Straight from the code points: CPython finds the narrowest kind itself,
    // so there is no intermediate UTF-8 `String`.
    let cps = s.code_points();
    let len = ffi::Py_ssize_t::try_from(cps.len())
        .map_err(|_| pyo3::exceptions::PyOverflowError::new_err("string too long"))?;
    // SAFETY: `cps` is a valid buffer of `len` UCS-4 code points, each
    // `<= 0x10FFFF`; CPython copies it into a new `str` (a new reference, or
    // null with an error set, which `from_owned_ptr_or_err` handles).
    unsafe {
        let ptr = ffi::PyUnicode_FromKindAndData(
            ffi::PyUnicode_4BYTE_KIND as std::os::raw::c_int,
            cps.as_ptr().cast(),
            len,
        );
        Bound::from_owned_ptr_or_err(py, ptr)
    }
}

fn items_to_py<'py>(py: Python<'py>, items: &[Value]) -> PyResult<Vec<Bound<'py, PyAny>>> {
    items.iter().map(|item| to_py(py, item)).collect()
}

/// Converts a core [`Value`] back to the Python object it stands for.
fn to_py<'py>(py: Python<'py>, value: &Value) -> PyResult<Bound<'py, PyAny>> {
    Ok(match value {
        Value::None => PyNone::get(py).to_owned().into_any(),
        Value::Bool(b) => PyBool::new(py, *b).to_owned().into_any(),
        Value::Int(n) => match i64::try_from(n) {
            Ok(small) => small.into_pyobject(py)?.into_any(),
            Err(_) => n.into_pyobject(py)?.into_any(),
        },
        Value::Float(f) => PyFloat::new(py, *f).into_any(),
        Value::Str(s) => core_str_to_py(py, s)?,
        Value::Bytes(b) => PyBytes::new(py, b).into_any(),
        Value::ByteArray(b) => PyByteArray::new(py, b).into_any(),
        // Short tuples (divmod's pair) skip the intermediate Vec.
        Value::Tuple(items) => match items.as_slice() {
            [a, b] => PyTuple::new(py, [to_py(py, a)?, to_py(py, b)?])?.into_any(),
            _ => PyTuple::new(py, items_to_py(py, items)?)?.into_any(),
        },
        Value::List(items) => PyList::new(py, items_to_py(py, items)?)?.into_any(),
        Value::Set(items) => PySet::new(py, items_to_py(py, items)?)?.into_any(),
        Value::FrozenSet(items) => PyFrozenSet::new(py, items_to_py(py, items)?)?.into_any(),
        Value::Dict(pairs) => {
            let dict = PyDict::new(py);
            for (k, v) in pairs {
                dict.set_item(to_py(py, k)?, to_py(py, v)?)?;
            }
            dict.into_any()
        }
        Value::Foreign(name) => {
            return Err(PyTypeError::new_err(format!(
                "cannot return a {name} from the core"
            )))
        }
    })
}

/// Raises the core error as the matching Python exception, with the same
/// single argument.
fn to_py_err(py: Python<'_>, err: PyError) -> PyErr {
    let arg = match to_py(py, &err.arg) {
        Ok(arg) => arg.unbind(),
        Err(e) => return e,
    };
    match err.kind {
        ExcKind::ValueError => PyValueError::new_err((arg,)),
        ExcKind::TypeError => PyTypeError::new_err((arg,)),
        ExcKind::OverflowError => PyOverflowError::new_err((arg,)),
        ExcKind::ZeroDivisionError => PyZeroDivisionError::new_err((arg,)),
        ExcKind::KeyError => PyKeyError::new_err((arg,)),
        ExcKind::IndexError => PyIndexError::new_err((arg,)),
        ExcKind::AttributeError => PyAttributeError::new_err((arg,)),
        // CPython's `PyErr_NoMemory` raises `MemoryError()` with no arguments.
        ExcKind::MemoryError => PyMemoryError::new_err(()),
        ExcKind::RecursionError => PyRecursionError::new_err((arg,)),
        ExcKind::NotImplementedError => PyNotImplementedError::new_err((arg,)),
        _ => PyNotImplementedError::new_err((arg,)),
    }
}

fn finish<'py>(py: Python<'py>, result: PcResult<Value>) -> PyResult<Bound<'py, PyAny>> {
    match result {
        Ok(value) => to_py(py, &value),
        Err(err) => Err(to_py_err(py, err)),
    }
}

// ---------------------------------------------------------------- API

macro_rules! unary {
    ($name:ident) => {
        #[pyfunction]
        fn $name<'py>(x: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
            let py = x.py();
            let x = to_core(x)?;
            finish(py, core::$name(&x))
        }
    };
}

macro_rules! binary {
    ($name:ident, $a:ident, $b:ident) => {
        #[pyfunction]
        fn $name<'py>(
            $a: &Bound<'py, PyAny>,
            $b: &Bound<'py, PyAny>,
        ) -> PyResult<Bound<'py, PyAny>> {
            let py = $a.py();
            let [$a, $b] = args_to_core([$a, $b])?;
            finish(py, core::$name(&$a, &$b))
        }
    };
}

unary!(float_repr);
unary!(int_str);
unary!(parse_float);
unary!(int_to_float);
binary!(format_value, value, spec);
binary!(percent_format, template, values);
binary!(floor_div, a, b);
binary!(modulo, a, b);
binary!(div_mod, a, b);
binary!(true_div, a, b);

#[pyfunction]
#[pyo3(signature = (x, ndigits=None))]
fn round_number<'py>(
    x: &Bound<'py, PyAny>,
    ndigits: Option<&Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    let py = x.py();
    let mut conv = Converter::default();
    let x = conv.convert(x)?;
    let ndigits = ndigits
        .map(|n| conv.convert(n))
        .transpose()?
        .unwrap_or(Value::None);
    finish(py, core::round_number(&x, &ndigits))
}

/// Binds `pos` and `kw` to `params` the way a Python `def` does, so that an
/// explicit `None` stays distinguishable from a missing argument.
fn bind_args<'py>(
    name: &str,
    params: &[&str],
    required: usize,
    pos: &Bound<'py, PyTuple>,
    kw: Option<&Bound<'py, PyDict>>,
) -> PyResult<Vec<Option<Bound<'py, PyAny>>>> {
    if pos.len() > params.len() {
        return Err(PyTypeError::new_err(format!(
            "{name}() takes from {required} to {} positional arguments but {} were given",
            params.len(),
            pos.len()
        )));
    }
    let mut bound: Vec<Option<Bound<'py, PyAny>>> = vec![None; params.len()];
    for (slot, item) in bound.iter_mut().zip(pos.iter()) {
        *slot = Some(item);
    }
    if let Some(kw) = kw {
        for (key, item) in kw.iter() {
            let key = key.cast::<PyString>()?.to_str()?;
            let Some(index) = params.iter().position(|p| *p == key) else {
                return Err(PyTypeError::new_err(format!(
                    "{name}() got an unexpected keyword argument '{key}'"
                )));
            };
            if bound[index].is_some() {
                return Err(PyTypeError::new_err(format!(
                    "{name}() got multiple values for argument '{key}'"
                )));
            }
            bound[index] = Some(item);
        }
    }
    let missing: Vec<String> = params[..required]
        .iter()
        .zip(&bound)
        .filter(|(_, b)| b.is_none())
        .map(|(p, _)| format!("'{p}'"))
        .collect();
    if !missing.is_empty() {
        let plural = if missing.len() == 1 { "" } else { "s" };
        return Err(PyTypeError::new_err(format!(
            "{name}() missing {} required positional argument{plural}: {}",
            missing.len(),
            missing.join(" and ")
        )));
    }
    Ok(bound)
}

#[pyfunction]
#[pyo3(signature = (*pos, **kw))]
fn parse_int<'py>(
    pos: &Bound<'py, PyTuple>,
    kw: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyAny>> {
    let py = pos.py();
    let bound = bind_args("parse_int", &["text", "base"], 1, pos, kw)?;
    let [text, base] = <[_; 2]>::try_from(bound).map_err(|_| PyTypeError::new_err("parse_int"))?;
    let mut conv = Converter::default();
    let text = text
        .as_ref()
        .map(|o| conv.convert(o))
        .transpose()?
        .unwrap_or(Value::None);
    let base = base
        .as_ref()
        .map(|o| conv.convert(o))
        .transpose()?
        .unwrap_or(Value::int(10));
    finish(py, core::parse_int(&text, &base))
}

#[pyfunction]
#[pyo3(signature = (*pos, **kw))]
fn str_format<'py>(
    pos: &Bound<'py, PyTuple>,
    kw: Option<&Bound<'py, PyDict>>,
) -> PyResult<Bound<'py, PyAny>> {
    let py = pos.py();
    let bound = bind_args("str_format", &["template", "args", "kwargs"], 1, pos, kw)?;
    let [template, args, kwargs] =
        <[_; 3]>::try_from(bound).map_err(|_| PyTypeError::new_err("str_format"))?;
    let mut conv = Converter::default();
    let template = template
        .as_ref()
        .map(|o| conv.convert(o))
        .transpose()?
        .unwrap_or(Value::None);
    let args = args
        .as_ref()
        .map(|o| conv.convert(o))
        .transpose()?
        .unwrap_or(Value::Tuple(Vec::new()));
    let kwargs = kwargs
        .as_ref()
        .map(|o| conv.convert(o))
        .transpose()?
        .unwrap_or(Value::None);
    finish(py, core::str_format(&template, &args, &kwargs))
}

#[pymodule(name = "pc_format")]
fn module_init(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(format_value, module)?)?;
    module.add_function(wrap_pyfunction!(str_format, module)?)?;
    module.add_function(wrap_pyfunction!(percent_format, module)?)?;
    module.add_function(wrap_pyfunction!(float_repr, module)?)?;
    module.add_function(wrap_pyfunction!(int_str, module)?)?;
    module.add_function(wrap_pyfunction!(round_number, module)?)?;
    module.add_function(wrap_pyfunction!(parse_int, module)?)?;
    module.add_function(wrap_pyfunction!(parse_float, module)?)?;
    module.add_function(wrap_pyfunction!(int_to_float, module)?)?;
    module.add_function(wrap_pyfunction!(floor_div, module)?)?;
    module.add_function(wrap_pyfunction!(modulo, module)?)?;
    module.add_function(wrap_pyfunction!(div_mod, module)?)?;
    module.add_function(wrap_pyfunction!(true_div, module)?)?;
    Ok(())
}
