//! `percentage(string, match)`.
#![no_main]

use libfuzzer_sys::fuzz_target;
use parse_port_core::convert::percentage;

fuzz_target!(|data: &[u8]| {
    let _ = percentage(&String::from_utf8_lossy(data));
});
