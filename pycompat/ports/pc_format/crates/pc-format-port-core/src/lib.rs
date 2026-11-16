//! Experimental, unofficial Rust port of pc_format. Contains no Python.
//!
//! Every behavior of the Python built-ins that `pc_format` wraps (reprs,
//! format-spec parsing, rounding, error messages) is implemented here.
//! Frozen: owned by the planner. Each public API takes and returns
//! [`Value`]s and runs the builtin guard first.
//!
//! Outputs that CPython could not allocate under a 1 GiB address-space
//! limit fail with `MemoryError` (see `CPYTHON_BUDGET` in
//! `format_spec/number.rs`), even where the host has more memory.
#![forbid(unsafe_code)]

mod arith;
mod format_spec;
mod numbers;
mod percent;
mod repr;
mod templates;
mod value;

pub use arith::{
    div_mod_api as div_mod, floor_div_api as floor_div, round_number_api as round_number,
    true_div_api as true_div,
};
pub use format_spec::format_value_api as format_value;
pub use numbers::{
    decimal_digit_value, float_repr_api as float_repr, int_str_api as int_str,
    int_to_float_api as int_to_float, parse_float_api as parse_float, parse_int_api as parse_int,
};
pub use percent::{modulo_api as modulo, percent_format_api as percent_format};
pub use templates::str_format_api as str_format;
pub use value::{check_builtin, check_builtins, ExcKind, PcResult, PyError, PyStr, Value};
