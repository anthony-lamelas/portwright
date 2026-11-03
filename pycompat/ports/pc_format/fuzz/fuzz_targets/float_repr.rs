#![no_main]
//! Fuzzes `pc_format_port_core::float_repr`. Python exceptions are expected; only
//! panics, aborts, hangs and runaway allocations are failures.

use arbitrary::Unstructured;
use libfuzzer_sys::fuzz_target;
use pc_format_port_core as core_api;

#[path = "../common/mod.rs"]
mod common;

fn go(u: &mut Unstructured) -> arbitrary::Result<()> {
    let x = if u.ratio(2u8, 3)? { core_api::Value::Float(common::float(u)?) } else { common::value(u)? };
    common::run(core_api::float_repr(&x));
    Ok(())
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let _ = go(&mut u);
});
