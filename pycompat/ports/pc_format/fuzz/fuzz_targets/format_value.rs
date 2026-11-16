//! Fuzzes `pc_format_port_core::format_value`: errors are expected, panics are bugs.
#![no_main]

use libfuzzer_sys::fuzz_target;
use pc_format_port_core as core;
use pc_format_port_core::{Value};
use pcf_fuzz::{Gen};

fuzz_target!(|data: &[u8]| {
    let mut g = Gen::bounded(data);
    let value = g.format_arg();
    let spec = match g.pick(10) {
        0 => g.value(1),
        _ => Value::Str(g.spec()),
    };
    let _ = core::format_value(&value, &spec);
});
