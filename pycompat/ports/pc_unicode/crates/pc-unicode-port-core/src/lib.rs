//! Experimental, unofficial Rust port of pc_unicode. Contains no Python.
//!
//! Every public API takes already-bound [`value::Value`] arguments and performs the
//! guard, type checks and the operation itself, with CPython 3.12 / Unicode 15.0 semantics.
#![forbid(unsafe_code)]

mod args;
pub mod call;
pub mod casing;
pub mod error;
mod hangul;
pub mod layout;
pub mod names;
pub mod normalization;
mod tables;
pub mod unidata;
pub mod value;

pub use error::{Error, ErrorKind, Result};
pub use value::{Text, Value};
