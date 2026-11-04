//! `compile(pattern, flags)`: parser, compiler, DEBUG dump, reprs, accessors.
#![no_main]
#[path = "common/mod.rs"]
mod common;

use arbitrary::Unstructured;
use libfuzzer_sys::fuzz_target;
use pc_re_port_core::{api, pyrepr, Value};

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let Ok(p) = common::any_pattern(&mut u) else { return };
    let Ok(flags) = common::flags(&mut u) else { return };
    let r = api::compile(&[Value::Str(p.clone())], &[("flags".to_string(), flags)]);
    if let Ok(pat) = &r {
        let _ = pyrepr::repr(pat);
        let one = [pat.clone()];
        let _ = api::pattern_text(&one, &[]);
        let _ = api::pattern_flags(&one, &[]);
        let _ = api::pattern_groups(&one, &[]);
        let _ = api::pattern_groupindex(&one, &[]);
    }
    if let Err(e) = &r {
        let _ = format!("{e:?}");
    }
    // Default flags.
    let _ = api::compile(&[Value::Str(p)], &[]);
    common::drain();
});
