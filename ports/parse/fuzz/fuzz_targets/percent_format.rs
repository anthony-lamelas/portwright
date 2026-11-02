//! `template % arg` with a str argument. Input: `template \0 arg`.
#![no_main]

use libfuzzer_sys::fuzz_target;
use parse_port_core::format::percent_format;

fuzz_target!(|data: &[u8]| {
    let s = String::from_utf8_lossy(data);
    let (template, arg) = s.split_once('\0').unwrap_or((&s, ""));
    let _ = percent_format(template, arg);
});
