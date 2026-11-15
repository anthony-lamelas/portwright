//! Fuzzes `pc_format_port_core::round_number`: errors are expected, panics are bugs.
#![no_main]

use libfuzzer_sys::fuzz_target;
use pc_format_port_core as core;
use pc_format_port_core::{Value};
use pcf_fuzz::{Gen};

fuzz_target!(|data: &[u8]| {
    let mut g = Gen::bounded(data);
    let x = match g.pick(8) {
        0..=6 => g.number(),
        _ => g.value(1),
    };
    let nd = match g.pick(10) {
        0..=1 => Value::None,
        2..=5 => Value::int(i64::from(g.byte() as i8)),
        6 => Value::Int(g.bigint()),
        7 => Value::Bool(g.flag()),
        _ => g.value(1),
    };
    let _ = core::round_number(&x, &nd);
});
