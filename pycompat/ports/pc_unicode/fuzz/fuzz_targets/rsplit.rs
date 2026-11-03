#![no_main]

use libfuzzer_sys::fuzz_target;
use pc_unicode_port_core::{layout};

#[path = "../common.rs"]
mod common;

fuzz_target!(|data: &[u8]| {
    let mut u = arbitrary::Unstructured::new(data);
    let run = |u: &mut arbitrary::Unstructured| -> arbitrary::Result<()> {
        let s = common::str_arg(u)?;
        let sep = common::opt_str_or_none(u)?;
        let maxsplit = common::opt(u, common::int_arg)?;
        let _ = layout::rsplit(&s, sep.as_ref(), maxsplit.as_ref());
        Ok(())
    };
    // Running out of fuzz bytes is not a bug; neither is any returned Python error.
    let _ = run(&mut u);
});
