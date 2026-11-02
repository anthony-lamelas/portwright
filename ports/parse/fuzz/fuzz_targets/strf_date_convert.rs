//! `strf_date_convert(x, _, type)`. Input: `x \0 type`.
#![no_main]

use libfuzzer_sys::fuzz_target;
use parse_port_core::strptime::strf_date_convert;

fuzz_target!(|data: &[u8]| {
    let s = String::from_utf8_lossy(data);
    let (x, type_) = s.split_once('\0').unwrap_or((&s, "%Y-%m-%d"));
    if x.chars().count() > 256 || type_.chars().count() > 128 {
        return;
    }
    let _ = strf_date_convert(x, type_);
});
