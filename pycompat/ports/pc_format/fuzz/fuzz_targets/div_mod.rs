#![no_main]
//! Fuzzes `pc_format_port_core::div_mod`. Python exceptions are expected; only
//! panics, aborts, hangs and runaway allocations are failures.

use arbitrary::Unstructured;
use libfuzzer_sys::fuzz_target;
use pc_format_port_core as core_api;

#[path = "../common/mod.rs"]
mod common;

fn go(u: &mut Unstructured) -> arbitrary::Result<()> {
    let a = if u.ratio(3u8, 4)? { common::number(u)? } else { common::value(u)? };
    let b = if u.ratio(3u8, 4)? { common::number(u)? } else { common::value(u)? };
    common::run(core_api::div_mod(&a, &b));
    Ok(())
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let _ = go(&mut u);
});
