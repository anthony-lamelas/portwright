//! Fuzzes `pc_format_port_core::percent_format`: errors are expected, panics are bugs.
#![no_main]

use libfuzzer_sys::fuzz_target;
use pc_format_port_core as core;
use pc_format_port_core::{PyStr, Value};
use pcf_fuzz::{Gen};

/// A %-template as str, bytes or bytearray, or any value.
fn percent_template(g: &mut Gen<'_>) -> Value {
    match g.pick(10) {
        0..=5 => Value::Str(PyStr::from_code_points(g.percent_template_cps())),
        6 | 7 => {
            let t = g.percent_template_cps();
            Value::Bytes(g.bytes_from_cps(&t))
        }
        8 => {
            let t = g.percent_template_cps();
            Value::ByteArray(g.bytes_from_cps(&t))
        }
        _ => g.value(1),
    }
}

/// The right operand of `%`: a tuple, a mapping, or a single value.
fn percent_values(g: &mut Gen<'_>) -> Value {
    match g.pick(8) {
        0..=3 => Value::Tuple((0..g.pick(6)).map(|_| g.format_arg()).collect()),
        4 | 5 => Value::Dict(
            (0..g.pick(5))
                .map(|_| {
                    let k = g.key();
                    (k, g.format_arg())
                })
                .collect(),
        ),
        _ => g.format_arg(),
    }
}

fuzz_target!(|data: &[u8]| {
    let mut g = Gen::bounded(data);
    let template = percent_template(&mut g);
    let values = percent_values(&mut g);
    let _ = core::percent_format(&template, &values);
});
