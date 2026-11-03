//! Thin Python layer over pc-unicode-port-core: convert arguments, call the core,
//! convert results, raise errors. No behavior lives here.
#![deny(unsafe_code)]

use pyo3::prelude::*;

mod casing;
pub mod convert;
mod layout;
mod names;
mod normalization;
mod unidata;

#[pymodule(name = "pc_unicode")]
fn module_init(module: &Bound<'_, PyModule>) -> PyResult<()> {
    unidata::register(module)?;
    casing::register(module)?;
    layout::register(module)?;
    normalization::register(module)?;
    names::register(module)?;
    Ok(())
}
