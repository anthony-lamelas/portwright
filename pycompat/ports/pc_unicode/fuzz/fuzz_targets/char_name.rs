#![no_main]

use libfuzzer_sys::fuzz_target;
use pc_unicode_port_core::{names};

#[path = "../common.rs"]
mod common;

fuzz_target!(|data: &[u8]| {
    let mut u = arbitrary::Unstructured::new(data);
    let run = |u: &mut arbitrary::Unstructured| -> arbitrary::Result<()> {
        let ch = common::char_arg(u)?;
        let default = common::opt(u, |u| common::value(u, 1))?;
        let _ = names::char_name(&ch, default.as_ref());
        Ok(())
    };
    // Running out of fuzz bytes is not a bug; neither is any returned Python error.
    let _ = run(&mut u);
});
