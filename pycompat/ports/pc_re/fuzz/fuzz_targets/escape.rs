//! `escape(pattern)`, and that the escaped text compiles.
#![no_main]
#[path = "common/mod.rs"]
mod common;

use arbitrary::Unstructured;
use libfuzzer_sys::fuzz_target;
use pc_re_port_core::{api, Value};

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let Ok(s) = common::text(&mut u, 64) else { return };
    if let Ok(e) = api::escape(&[Value::Str(s.clone())], &[]) {
        let _ = api::fullmatch(&[e, Value::Str(s)], &[]);
    }
    if let Ok(v) = common::value(&mut u, 1) {
        let _ = api::escape(&[], &[("pattern".to_string(), v)]);
    }
    common::drain();
});
