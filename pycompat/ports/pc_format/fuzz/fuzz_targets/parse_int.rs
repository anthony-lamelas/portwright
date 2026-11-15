//! Fuzzes `pc_format_port_core::parse_int`: errors are expected, panics are bugs.
#![no_main]

use libfuzzer_sys::fuzz_target;
use pc_format_port_core as core;
use pc_format_port_core::{Value};
use pcf_fuzz::{Gen};

/// Numeric text as str, bytes or bytearray, or any value.
fn numeric_value(g: &mut Gen<'_>) -> Value {
    match g.pick(10) {
        0..=5 => Value::Str(g.numeric_text()),
        6 => {
            let t = g.numeric_text();
            Value::Bytes(g.bytes_from_cps(t.code_points()))
        }
        7 => {
            let t = g.numeric_text();
            Value::ByteArray(g.bytes_from_cps(t.code_points()))
        }
        _ => g.value(1),
    }
}

fuzz_target!(|data: &[u8]| {
    let mut g = Gen::new(data);
    let text = numeric_value(&mut g);
    let base = match g.pick(8) {
        0..=2 => Value::int(10),
        3 => Value::int(0),
        4 => Value::int(i64::from(g.pick(40)) - 1),
        5 => Value::Bool(g.flag()),
        _ => g.value(1),
    };
    let _ = core::parse_int(&text, &base);
});
