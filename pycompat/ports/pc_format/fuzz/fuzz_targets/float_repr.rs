//! Fuzzes `pc_format_port_core::float_repr`: errors are expected, panics are bugs.
#![no_main]

use libfuzzer_sys::fuzz_target;
use pc_format_port_core as core;
use pc_format_port_core::{Value};
use pcf_fuzz::{Gen};

fuzz_target!(|data: &[u8]| {
    let mut g = Gen::bounded(data);
    let x = match g.pick(8) {
        0..=5 => Value::Float(g.float()),
        _ => g.value(2),
    };
    let _ = core::float_repr(&x);
});
