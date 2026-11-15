//! Fuzzes `pc_format_port_core::int_to_float`: errors are expected, panics are bugs.
#![no_main]

use libfuzzer_sys::fuzz_target;
use pc_format_port_core as core;
use pc_format_port_core::{Value};
use pcf_fuzz::{Gen};

fuzz_target!(|data: &[u8]| {
    let mut g = Gen::new(data);
    let n = match g.pick(8) {
        0..=4 => Value::Int(g.bigint()),
        5 => Value::Bool(g.flag()),
        6 => Value::Float(g.float()),
        _ => g.value(2),
    };
    let _ = core::int_to_float(&n);
});
