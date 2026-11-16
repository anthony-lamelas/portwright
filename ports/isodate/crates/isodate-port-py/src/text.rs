//! Python `str` <-> code points, lone surrogates included. Shared and frozen.
//!
//! Core text is `&[u32]` / `PyStr` (one entry per Python code point), so lengths,
//! indexes and regex positions count as Python does.

use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyString};

/// The code points of a Python `str`, lone surrogates included.
pub fn py_to_cps(s: &Bound<'_, PyString>) -> PyResult<Vec<u32>> {
    if let Ok(text) = s.to_str() {
        return Ok(text.chars().map(u32::from).collect());
    }
    // Lone surrogates: not valid UTF-8, so read UTF-32 with surrogatepass.
    let encoded = s
        .call_method1("encode", ("utf-32-le", "surrogatepass"))?
        .cast_into::<PyBytes>()?;
    let bytes = encoded.as_bytes();
    let (chunks, _) = bytes.as_chunks::<4>();
    Ok(chunks.iter().map(|&c| u32::from_le_bytes(c)).collect())
}

/// A Python `str` from code points, lone surrogates included.
pub fn cps_to_py<'py>(py: Python<'py>, cps: &[u32]) -> PyResult<Bound<'py, PyString>> {
    let text: Option<String> = cps.iter().map(|&c| char::from_u32(c)).collect();
    if let Some(text) = text {
        return Ok(PyString::new(py, &text));
    }
    let bytes: Vec<u8> = cps.iter().flat_map(|c| c.to_le_bytes()).collect();
    let decoded = PyBytes::new(py, &bytes)
        .into_any()
        .call_method1("decode", ("utf-32-le", "surrogatepass"))?;
    Ok(decoded.cast_into::<PyString>()?)
}
