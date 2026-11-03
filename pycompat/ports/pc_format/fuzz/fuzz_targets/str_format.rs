#![no_main]
//! Fuzzes `pc_format_port_core::str_format`. Python exceptions are expected; only
//! panics, aborts, hangs and runaway allocations are failures.

use arbitrary::Unstructured;
use libfuzzer_sys::fuzz_target;
use pc_format_port_core as core_api;

#[path = "../common/mod.rs"]
mod common;

fn go(u: &mut Unstructured) -> arbitrary::Result<()> {
    let template = if u.ratio(9u8, 10)? { core_api::Value::Str(common::py_str(u, 40)?) } else { common::value(u)? };
    let args = match u.int_in_range(0u8..=3)? {
        0 => core_api::Value::Tuple(vec![]),
        1 | 2 => {
            let n = u.int_in_range(0usize..=4)?;
            core_api::Value::Tuple((0..n).map(|_| common::value(u)).collect::<arbitrary::Result<_>>()?)
        }
        _ => common::value(u)?,
    };
    let kwargs = match u.int_in_range(0u8..=3)? {
        0 => core_api::Value::None,
        1 | 2 => {
            let n = u.int_in_range(0usize..=4)?;
            let pairs = (0..n)
                .map(|_| {
                    let k = if u.ratio(4u8, 5)? { core_api::Value::Str(common::py_str(u, 3)?) } else { common::hashable(u, 2)? };
                    Ok((k, common::value(u)?))
                })
                .collect::<arbitrary::Result<_>>()?;
            core_api::Value::Dict(pairs)
        }
        _ => common::value(u)?,
    };
    common::run(core_api::str_format(&template, &args, &kwargs));
    Ok(())
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let _ = go(&mut u);
});
