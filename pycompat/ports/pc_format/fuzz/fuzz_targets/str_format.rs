//! Fuzzes `pc_format_port_core::str_format`: errors are expected, panics are bugs.
#![no_main]

use libfuzzer_sys::fuzz_target;
use pc_format_port_core as core;
use pc_format_port_core::{Value};
use pcf_fuzz::{Gen, nested_fields_bounded, tame_floats};

fuzz_target!(|data: &[u8]| {
    let mut g = Gen::bounded(data);
    let template = match g.pick(12) {
        0 => g.value(1),
        _ => Value::Str(g.format_template()),
    };
    // A nested field could splice an arg's str() into a spec.
    let mut has_nested = false;
    if let Value::Str(t) = &template {
        let cps = t.code_points();
        if !nested_fields_bounded(cps) {
            return;
        }
        has_nested = cps
            .iter()
            .position(|&c| c == u32::from(':'))
            .is_some_and(|i| cps[i..].contains(&u32::from('{')));
    }
    let args = match g.pick(8) {
        0..=4 => Value::Tuple((0..g.pick(6)).map(|_| g.format_arg()).collect()),
        5 => Value::List((0..g.pick(6)).map(|_| g.format_arg()).collect()),
        _ => g.value(2),
    };
    let kwargs = match g.pick(8) {
        0..=4 => Value::Dict(
            (0..g.pick(5))
                .map(|_| {
                    let k = g.key();
                    (k, g.format_arg())
                })
                .collect(),
        ),
        5 => Value::None,
        _ => g.value(2),
    };
    let (args, kwargs) = if has_nested {
        (tame_floats(args), tame_floats(kwargs))
    } else {
        (args, kwargs)
    };
    let _ = core::str_format(&template, &args, &kwargs);
});
