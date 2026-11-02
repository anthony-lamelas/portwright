//! Experimental, unofficial Rust port of parse. Contains no Python.
//!
//! Layout (see `/work/out/decisions.md`):
//! - [`error`], [`pystr`], [`types`]: frozen shared foundation.
//! - [`pyre`]: Python-`re` (sre) compatible backtracking regex engine.
//! - [`convert`]: `int_convert`, `percentage`, `date_convert`, `FixedTzOffset` helpers.
//! - [`strptime`]: port of CPython `_strptime` plus `strf_date_convert`.
//! - [`format`]: `extract_format` and the `Parser` expression builder.
#![forbid(unsafe_code)]

pub mod convert;
pub mod error;
pub mod format;
pub mod pyre;
pub mod pystr;
pub mod strptime;
pub mod types;

pub use error::{Error, RegexError};
