#![no_main]
//! Fuzzes `pc_format_port_core::int_to_float`. Python exceptions are expected; only
//! panics, aborts, hangs and runaway allocations are failures.

use arbitrary::Unstructured;
use libfuzzer_sys::fuzz_target;
use pc_format_port_core as core_api;

#[path = "../common/mod.rs"]
mod common;

fn go(u: &mut Unstructured) -> arbitrary::Result<()> {
    let n = if u.ratio(2u8, 3)? { common::number(u)? } else { common::text_like(u, 40)? };
    common::run(core_api::int_to_float(&n));
    Ok(())
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let _ = go(&mut u);
});
