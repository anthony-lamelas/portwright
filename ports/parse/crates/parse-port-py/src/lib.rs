//! Thin Python layer over parse-port-core. Python-specific behavior belongs here.
//!
//! Frozen: each task registers its objects from its own file via `register`.
#![forbid(unsafe_code)]

use pyo3::prelude::*;

pub mod convert;
pub mod errors;
pub mod parser;
pub mod strptime;

#[pymodule(name = "parse")]
fn module_init(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", "1.22.3")?;
    errors::register(m)?;
    convert::register(m)?;
    strptime::register(m)?;
    parser::register(m)?;
    Ok(())
}
