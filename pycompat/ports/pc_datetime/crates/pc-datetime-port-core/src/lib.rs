//! Experimental, unofficial Rust port of pc_datetime. Contains no Python.
//!
//! Every shim function is `fn(&[Value], &Kwargs) -> PyResult<Value>`: it receives the
//! Python arguments as plain [`Value`]s and returns the result tuple or the exception.
#![forbid(unsafe_code)]

pub mod args;
pub mod error;
mod printable;
pub mod pystr;
pub mod value;

pub mod date;
pub mod datetime;
pub mod isoparse;
pub mod strftime;
pub mod strptime;
pub mod timedelta;
pub mod timestamp;

pub use args::Kwargs;
pub use error::{PyError, PyResult};
pub use pystr::PyStr;
pub use value::Value;
