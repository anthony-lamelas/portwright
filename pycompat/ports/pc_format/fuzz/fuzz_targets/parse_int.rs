#![no_main]
//! Fuzzes `pc_format_port_core::parse_int`. Python exceptions are expected; only
//! panics, aborts, hangs and runaway allocations are failures.

use arbitrary::Unstructured;
use libfuzzer_sys::fuzz_target;
use pc_format_port_core as core_api;

#[path = "../common/mod.rs"]
mod common;

fn go(u: &mut Unstructured) -> arbitrary::Result<()> {
    let text = common::text_like(u, 48)?;
    let base = common::small_int_like(u, -2, 40)?;
    common::run(core_api::parse_int(&text, &base));
    common::run(core_api::parse_int(&text, &core_api::Value::int(10)));
    common::run(core_api::parse_int(&text, &core_api::Value::int(0)));
    Ok(())
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let _ = go(&mut u);
});
