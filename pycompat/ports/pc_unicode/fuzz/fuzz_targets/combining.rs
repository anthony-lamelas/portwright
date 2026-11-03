#![no_main]

use libfuzzer_sys::fuzz_target;
use pc_unicode_port_core::{unidata};

#[path = "../common.rs"]
mod common;

fuzz_target!(|data: &[u8]| {
    let mut u = arbitrary::Unstructured::new(data);
    let run = |u: &mut arbitrary::Unstructured| -> arbitrary::Result<()> {
        let ch = common::char_arg(u)?;
        let _ = unidata::combining(&ch);
        Ok(())
    };
    // Running out of fuzz bytes is not a bug; neither is any returned Python error.
    let _ = run(&mut u);
});
