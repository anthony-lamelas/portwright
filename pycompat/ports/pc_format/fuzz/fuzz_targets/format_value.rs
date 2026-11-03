#![no_main]
//! Fuzzes `pc_format_port_core::format_value`. Python exceptions are expected; only
//! panics, aborts, hangs and runaway allocations are failures.

use arbitrary::Unstructured;
use libfuzzer_sys::fuzz_target;
use pc_format_port_core as core_api;

#[path = "../common/mod.rs"]
mod common;

fn go(u: &mut Unstructured) -> arbitrary::Result<()> {
    let v = if u.ratio(1u8, 2)? { common::number(u)? } else { common::value(u)? };
    let spec = if u.ratio(9u8, 10)? { core_api::Value::Str(common::py_str(u, 24)?) } else { common::value(u)? };
    common::run(core_api::format_value(&v, &spec));
    Ok(())
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let _ = go(&mut u);
});
