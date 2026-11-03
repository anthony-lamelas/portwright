#![no_main]

use libfuzzer_sys::fuzz_target;
use pc_unicode_port_core::{names};

#[path = "../common.rs"]
mod common;

fuzz_target!(|data: &[u8]| {
    let mut u = arbitrary::Unstructured::new(data);
    let run = |u: &mut arbitrary::Unstructured| -> arbitrary::Result<()> {
        let name = match u.int_in_range(0u8..=9)? {
            0 => common::value(u, 1)?,
            1 => pc_unicode_port_core::Value::Bytes(u.arbitrary()?),
            2 => pc_unicode_port_core::Value::Str(common::text(u)?),
            _ => pc_unicode_port_core::Value::Str(common::name_text(u)?),
        };
        let _ = names::lookup(&name);
        Ok(())
    };
    // Running out of fuzz bytes is not a bug; neither is any returned Python error.
    let _ = run(&mut u);
});
