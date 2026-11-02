//! `get_regex_for_datetime_format(format)`, then compile the result.
#![no_main]

use libfuzzer_sys::fuzz_target;
use parse_port_core::pyre::dt_format::{
    directive_regex, get_regex_for_datetime_format, get_regex_for_datetime_format_codepoints,
};
use parse_port_core::pyre::{Flags, Regex};

fuzz_target!(|data: &[u8]| {
    let s = String::from_utf8_lossy(data);
    let out = get_regex_for_datetime_format(&s);
    let _ = directive_regex(&s);
    let cps: Vec<u32> = data
        .chunks(3)
        .map(|c| c.iter().fold(0u32, |a, &b| (a << 8) | u32::from(b)) % 0x11_0000)
        .collect();
    let _ = get_regex_for_datetime_format_codepoints(&cps);
    if out.chars().count() <= 256 {
        let _ = Regex::new(&out, Flags::default());
    }
});
