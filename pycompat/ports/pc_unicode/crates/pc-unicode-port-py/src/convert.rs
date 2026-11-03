//! The only bridge between Python objects and the core. No logic lives here:
//! classify objects by exact type, copy their data, build results, raise errors.

use num_bigint::BigInt;
use pc_unicode_port_core::call::{bind, guard, Param};
use pc_unicode_port_core::value::check_builtin;
use pc_unicode_port_core::{Error, ErrorKind, Value};
use pyo3::exceptions::{
    PyKeyError, PyMemoryError, PyOverflowError, PyRecursionError, PyTypeError, PyValueError,
};
use pyo3::prelude::*;
use pyo3::types::{
    PyBool, PyByteArray, PyBytes, PyDict, PyFloat, PyFrozenSet, PyInt, PyList, PySet, PyString,
    PyStringData, PyTuple,
};

/// Deepest nesting level conversion accepts; one level deeper raises `RecursionError`.
/// Upstream's recursive `_check_builtin` hits CPython's default limit (1000 frames) at
/// nesting depth 996 when called from the harness worker (3 caller frames + `checked`).
const MAX_DEPTH: usize = 995;

/// A bound argument: the original object (to return defaults unchanged) and its [`Value`].
pub type Arg<'py> = (Bound<'py, PyAny>, Value);

/// Turns a core error into the matching Python exception.
pub fn to_py_err(err: Error) -> PyErr {
    let Error { kind, message } = err;
    match kind {
        ErrorKind::Type => PyTypeError::new_err(message),
        ErrorKind::Value => PyValueError::new_err(message),
        ErrorKind::Key => PyKeyError::new_err(message),
        ErrorKind::Overflow => PyOverflowError::new_err(message),
        ErrorKind::Memory if message.is_empty() => PyMemoryError::new_err(()),
        ErrorKind::Memory => PyMemoryError::new_err(message),
        ErrorKind::Recursion => PyRecursionError::new_err(message),
    }
}

/// Code points of a Python `str`, lone surrogates included.
///
/// Copies CPython's canonical (Latin-1 / UCS-2 / UCS-4) buffer directly: no UTF-8
/// encoding or decoding, and surrogates need no special path.
pub fn text_of(s: &Bound<'_, PyString>) -> PyResult<Vec<u32>> {
    // SAFETY: `s` is a live, exact-or-sub `str` borrowed for this call; the returned slice
    // is copied before the borrow ends and the GIL is held throughout.
    #[expect(unsafe_code, reason = "only API that reads lone surrogates")]
    let data = unsafe { s.data()? };
    Ok(match data {
        PyStringData::Ucs1(d) => d.iter().map(|&c| u32::from(c)).collect(),
        PyStringData::Ucs2(d) => d.iter().map(|&c| u32::from(c)).collect(),
        PyStringData::Ucs4(d) => d.to_vec(),
    })
}

/// Builds a Python `str` from code points (lone surrogates allowed).
pub fn text_to_py<'py>(py: Python<'py>, text: &[u32]) -> PyResult<Bound<'py, PyString>> {
    text_to_py_with(py, text, &mut String::new())
}

/// Wide texts shorter than this go through UTF-8; the bytes object the UTF-32 path
/// needs costs more than it saves on very short strings.
const MIN_UTF32_LEN: usize = 8;

/// [`text_to_py`] using `buf` as scratch space, so a caller can reuse one buffer.
///
/// Picks CPython's cheapest decoder for the text: UTF-8 for ASCII, Latin-1 (a plain copy)
/// when every code point is below 256, and otherwise BOM-prefixed UTF-32. `"latin-1"` and
/// `"utf-32"` are on `PyUnicode_Decode`'s built-in fast path (unlike `"utf-32-le"`, which
/// costs a codec-registry lookup); `surrogatepass` keeps lone surrogates.
fn text_to_py_with<'py>(
    py: Python<'py>,
    text: &[u32],
    buf: &mut String,
) -> PyResult<Bound<'py, PyString>> {
    if text.iter().all(|&c| c < 0x80) {
        buf.clear();
        // `as u8` keeps every value: all are below 0x80.
        #[expect(clippy::cast_possible_truncation, reason = "every value is below 0x80")]
        buf.extend(text.iter().map(|&c| char::from(c as u8)));
        return Ok(PyString::new(py, buf));
    }
    if text.iter().all(|&c| c < 0x100) {
        let bytes = PyBytes::new_with(py, text.len(), |out| {
            for (byte, &c) in out.iter_mut().zip(text) {
                // Never falls back: every value is below 0x100.
                *byte = u8::try_from(c).unwrap_or(0);
            }
            Ok(())
        })?;
        return PyString::from_encoded_object(bytes.as_any(), Some(c"latin-1"), None);
    }
    if text.len() < MIN_UTF32_LEN {
        buf.clear();
        if text
            .iter()
            .all(|&c| char::from_u32(c).map(|ch| buf.push(ch)).is_some())
        {
            return Ok(PyString::new(py, buf));
        }
    }
    let bytes = PyBytes::new_with(py, (text.len() + 1) * 4, |out| {
        let (words, _) = out.as_chunks_mut::<4>();
        if let Some((bom, rest)) = words.split_first_mut() {
            *bom = 0xFEFF_u32.to_le_bytes();
            for (word, &c) in rest.iter_mut().zip(text) {
                *word = c.to_le_bytes();
            }
        }
        Ok(())
    })?;
    PyString::from_encoded_object(bytes.as_any(), Some(c"utf-32"), Some(c"surrogatepass"))
}

/// Builds a Python `list[str]`.
pub fn texts_to_py<'py, T: AsRef<[u32]>>(
    py: Python<'py>,
    texts: &[T],
) -> PyResult<Bound<'py, PyList>> {
    let mut buf = String::new();
    let items = texts
        .iter()
        .map(|t| text_to_py_with(py, t.as_ref(), &mut buf))
        .collect::<PyResult<Vec<_>>>()?;
    PyList::new(py, items)
}

/// Classifies `obj` by exact type and copies it into a [`Value`], running upstream's
/// builtin-type guard on the way (depth-first, dict key before value).
pub fn to_value(obj: &Bound<'_, PyAny>) -> PyResult<Value> {
    to_value_at(obj, 0)
}

fn to_value_at(obj: &Bound<'_, PyAny>, depth: usize) -> PyResult<Value> {
    if depth > MAX_DEPTH {
        return Err(PyRecursionError::new_err(
            "maximum recursion depth exceeded",
        ));
    }
    let items = |it: Vec<Bound<'_, PyAny>>| -> PyResult<Vec<Value>> {
        it.iter().map(|x| to_value_at(x, depth + 1)).collect()
    };
    Ok(if obj.is_none() {
        Value::None
    } else if let Ok(b) = obj.cast_exact::<PyBool>() {
        Value::Bool(b.is_true())
    } else if let Ok(i) = obj.cast_exact::<PyInt>() {
        // Most ints fit in an i64, which converts much faster than a full BigInt.
        match i.extract::<i64>() {
            Ok(small) => Value::Int(BigInt::from(small)),
            Err(_) => Value::Int(i.extract::<BigInt>()?),
        }
    } else if let Ok(f) = obj.cast_exact::<PyFloat>() {
        Value::Float(f.value())
    } else if let Ok(s) = obj.cast_exact::<PyString>() {
        Value::Str(text_of(s)?)
    } else if let Ok(b) = obj.cast_exact::<PyBytes>() {
        Value::Bytes(b.as_bytes().to_vec())
    } else if let Ok(b) = obj.cast_exact::<PyByteArray>() {
        Value::ByteArray(b.to_vec())
    } else if let Ok(l) = obj.cast_exact::<PyList>() {
        Value::List(items(l.iter().collect())?)
    } else if let Ok(t) = obj.cast_exact::<PyTuple>() {
        Value::Tuple(items(t.iter().collect())?)
    } else if let Ok(s) = obj.cast_exact::<PySet>() {
        Value::Set(items(s.iter().collect())?)
    } else if let Ok(s) = obj.cast_exact::<PyFrozenSet>() {
        Value::FrozenSet(items(s.iter().collect())?)
    } else if let Ok(d) = obj.cast_exact::<PyDict>() {
        let pairs = d
            .iter()
            .map(|(k, v)| Ok((to_value_at(&k, depth + 1)?, to_value_at(&v, depth + 1)?)))
            .collect::<PyResult<Vec<_>>>()?;
        Value::Dict(pairs)
    } else {
        // Upstream's guard walks in this same order and stops at the first non-builtin,
        // so report it now, before any deeper value can raise `RecursionError`.
        let other = Value::Other(obj.get_type().name()?.to_string_lossy().into_owned());
        check_builtin(&other).map_err(to_py_err)?;
        other
    })
}

/// Upstream's `checked(*args, **kwargs)` wrapper: guard every value, then bind to
/// `params` with CPython's call errors. `None` slots mean "parameter defaulted".
pub fn bind_call<'py>(
    name: &str,
    params: &[Param],
    args: &Bound<'py, PyTuple>,
    kwargs: Option<&Bound<'py, PyDict>>,
) -> PyResult<Vec<Option<Arg<'py>>>> {
    let n_args = args.len();
    let no_kwargs = kwargs.is_none_or(|kw| kw.is_empty());
    if no_kwargs && n_args <= params.len() && params.iter().skip(n_args).all(|p| !p.required) {
        // Positional-only call that binds without error: same slots as `bind` gives,
        // built directly. The guard still runs over every value, in order.
        let mut slots: Vec<Option<Arg<'py>>> = Vec::with_capacity(params.len());
        for a in args.iter() {
            let value = to_value(&a)?;
            slots.push(Some((a, value)));
        }
        guard(slots.iter().flatten().map(|(_, v)| v), []).map_err(to_py_err)?;
        slots.resize_with(params.len(), || None);
        return Ok(slots);
    }
    let mut positional: Vec<Arg<'py>> = Vec::with_capacity(n_args);
    for a in args.iter() {
        let value = to_value(&a)?;
        positional.push((a, value));
    }
    let mut keyword = Vec::new();
    if let Some(kw) = kwargs {
        for (k, v) in kw.iter() {
            let key = k.cast::<PyString>()?.to_string_lossy().into_owned();
            let value = to_value(&v)?;
            keyword.push((key, (v, value)));
        }
    }
    guard(
        positional.iter().map(|(_, v)| v),
        keyword.iter().map(|(_, (_, v))| v),
    )
    .map_err(to_py_err)?;
    bind(name, params, positional, keyword).map_err(to_py_err)
}

/// The [`Value`] in a bound slot, if the caller supplied it.
pub fn value_of<'a>(slot: &'a Option<Arg<'_>>) -> Option<&'a Value> {
    slot.as_ref().map(|(_, v)| v)
}
