//! Experimental, unofficial Rust port of pc_re (CPython 3.12 `re` for str
//! patterns). Contains no Python. See `/work/out/decisions.md`.
#![forbid(unsafe_code)]

mod alpha;
pub mod api;
pub mod args;
pub mod casefix;
pub mod compiler;
pub mod debug;
mod engine;
pub mod error;
pub mod flags;
pub mod frames;
pub mod matching;
pub mod parser;
pub mod pattern;
pub mod pyrepr;
pub mod scan;
pub mod sre;
#[doc(hidden)]
pub mod stdout;
pub mod template;
pub mod text;
pub mod unicode;
#[rustfmt::skip]
mod unicode_tables;
pub mod value;

pub use error::{Error, ReError, Result};
pub use matching::Match;
pub use pattern::Pattern;
pub use text::PyStr;
pub use value::Value;
