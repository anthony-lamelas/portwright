//! Fuzzes `pc_format_port_core::div_mod`: errors are expected, panics are bugs.
#![no_main]

use libfuzzer_sys::fuzz_target;
use pc_format_port_core as core;
use pc_format_port_core::{Value};
use pcf_fuzz::{Gen};

/// Two operands: numbers mostly, sometimes any value.
fn binary_operands(g: &mut Gen<'_>) -> (Value, Value) {
    let one = |g: &mut Gen<'_>| match g.pick(8) {
        0..=6 => g.number(),
        _ => g.value(1),
    };
    let a = one(g);
    let b = one(g);
    (a, b)
}

fuzz_target!(|data: &[u8]| {
    let mut g = Gen::bounded(data);
    let (a, b) = binary_operands(&mut g);
    let _ = core::div_mod(&a, &b);
});
