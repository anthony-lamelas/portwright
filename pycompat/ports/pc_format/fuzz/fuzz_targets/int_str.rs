#![no_main]
//! Fuzzes `pc_format_port_core::int_str`. Python exceptions are expected; only
//! panics, aborts, hangs and runaway allocations are failures.

use arbitrary::Unstructured;
use libfuzzer_sys::fuzz_target;
use pc_format_port_core as core_api;

#[path = "../common/mod.rs"]
mod common;

fn go(u: &mut Unstructured) -> arbitrary::Result<()> {
    let n = if u.ratio(1u8, 2)? { core_api::Value::Int(common::big_int(u)?) } else { common::value(u)? };
    common::run(core_api::int_str(&n));
    Ok(())
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let _ = go(&mut u);
});
