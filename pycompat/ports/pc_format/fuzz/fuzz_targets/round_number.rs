#![no_main]
//! Fuzzes `pc_format_port_core::round_number`. Python exceptions are expected; only
//! panics, aborts, hangs and runaway allocations are failures.

use arbitrary::Unstructured;
use libfuzzer_sys::fuzz_target;
use pc_format_port_core as core_api;

#[path = "../common/mod.rs"]
mod common;

fn go(u: &mut Unstructured) -> arbitrary::Result<()> {
    let x = if u.ratio(3u8, 4)? { common::number(u)? } else { common::value(u)? };
    let nd = match u.int_in_range(0u8..=3)? {
        0 => core_api::Value::None,
        1 | 2 => common::small_int_like(u, -330, 330)?,
        _ => common::value(u)?,
    };
    common::run(core_api::round_number(&x, &nd));
    Ok(())
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let _ = go(&mut u);
});
