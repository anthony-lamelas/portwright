//! Experimental, unofficial Rust port of isodate. Contains no Python.
//!
//! Module owners (see `/work/out/plan.json`):
//! - [`error`]: shared, frozen.
//! - [`rx`], [`tz`]: task `tz`.
//! - [`decimal`], [`duration`]: task `duration`.
//! - [`strf`]: task `strf`.
//! - [`parse`]: task `parse`.
//!
//! A task that needs more files adds child modules under its own file
//! (for example `parse.rs` declaring `mod dates;` in `parse/dates.rs`).
#![forbid(unsafe_code)]

pub mod decimal;
pub mod duration;
pub mod error;
pub mod parse;
pub mod rx;
pub mod strf;
pub mod tz;

pub use error::{DecimalSignal, Error, PyError, PyStr, Result};
/// The vendored CPython `datetime` port, for porters' convenience.
pub use pc_datetime_port_core as pc_datetime;
/// The vendored CPython `re` port, for porters' convenience.
pub use pc_re_port_core as pc_re;
