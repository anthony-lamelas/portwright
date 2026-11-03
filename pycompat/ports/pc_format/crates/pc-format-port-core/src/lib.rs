//! Experimental, unofficial Rust port of pc_format. Contains no Python.
//!
//! Every behavior of the Python built-ins that `pc_format` wraps (reprs,
//! format-spec parsing, rounding, error messages) is implemented here.
//! Frozen: owned by the planner. Each public API takes and returns
//! [`Value`]s and runs the builtin guard first.
#![forbid(unsafe_code)]

pub mod arith;
pub mod format_spec;
pub mod numbers;
pub mod percent;
pub mod repr;
pub mod templates;
pub mod value;

pub use arith::{
    div_mod_api as div_mod, floor_div_api as floor_div, round_number_api as round_number,
    true_div_api as true_div,
};
pub use format_spec::format_value_api as format_value;
pub use numbers::{
    float_repr_api as float_repr, int_str_api as int_str, int_to_float_api as int_to_float,
    parse_float_api as parse_float, parse_int_api as parse_int,
};
pub use percent::{modulo_api as modulo, percent_format_api as percent_format};
pub use templates::str_format_api as str_format;
pub use value::{check_builtin, check_builtins, ExcKind, PcResult, PyError, PyStr, Value};
