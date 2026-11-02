//! `datetime.strptime(data, format)`. Input: `data \0 format`.
#![no_main]

use libfuzzer_sys::fuzz_target;
use parse_port_core::strptime::strptime;

fuzz_target!(|data: &[u8]| {
    let s = String::from_utf8_lossy(data);
    let (text, format) = s.split_once('\0').unwrap_or((&s, "%Y-%m-%d %H:%M:%S"));
    if text.chars().count() > 256 || format.chars().count() > 128 {
        return;
    }
    let _ = strptime(text, format);
});
