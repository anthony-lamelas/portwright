//! `error(msg, pattern=None, pos=None)`: message formatting, line/column.
#![no_main]
#[path = "common/mod.rs"]
mod common;

use arbitrary::Unstructured;
use libfuzzer_sys::fuzz_target;
use num_bigint::BigInt;
use pc_re_port_core::{api, Value};

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let Ok(msg) = common::text(&mut u, 16) else { return };
    let Ok(pat) = common::text(&mut u, 32) else { return };
    let pos = if u.ratio(1, 3).unwrap_or(false) {
        common::value(&mut u, 1).unwrap_or(Value::None)
    } else {
        Value::Int(BigInt::from(u.int_in_range(-3i64..=40).unwrap_or(0)))
    };
    let _ = api::error_init(&[Value::Str(msg.clone()), Value::Str(pat.clone()), pos.clone()], &[]);
    let _ = api::error_init(&[Value::Str(msg.clone())], &[("pos".into(), pos)]);
    let _ = api::error_init(&[Value::Str(msg)], &[]);
    // Fully arbitrary arguments.
    let n = u.int_in_range(0..=4).unwrap_or(0);
    let args: Vec<Value> = (0..n).map(|_| common::value(&mut u, 2).unwrap_or(Value::None)).collect();
    let _ = api::error_init(&args, &[]);
    common::drain();
});
