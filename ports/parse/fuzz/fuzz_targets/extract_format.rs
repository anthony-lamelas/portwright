//! `extract_format(spec, extra_types)`; the first byte picks whether the
//! spec's type counts as an extra type.
#![no_main]

use libfuzzer_sys::fuzz_target;
use parse_port_core::format::{extract_format, has_datetime_directive};
use parse_port_core::Error;

fuzz_target!(|data: &[u8]| {
    let Some((&flag, rest)) = data.split_first() else {
        return;
    };
    let spec = String::from_utf8_lossy(rest);
    let _ = has_datetime_directive(&spec);
    let _ = extract_format::<Error>(&spec, |_| Ok(flag & 1 == 1));
});
