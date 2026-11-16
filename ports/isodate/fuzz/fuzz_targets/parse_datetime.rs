//! `parse_datetime(text)`: the `T` split, then the date and time parsers.
#![no_main]

use isodate_port_core::parse::{parse_datetime, split_datetime};
use libfuzzer_sys::fuzz_target;

/// Bytes to Python code points. Valid UTF-8 is decoded; each invalid byte becomes the
/// lone surrogate U+DC80..U+DCFF (like `surrogateescape`), so the parsers also see
/// text that only exists in Python.
fn code_points(data: &[u8]) -> Vec<u32> {
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.utf8_chunks() {
        out.extend(chunk.valid().chars().map(u32::from));
        out.extend(chunk.invalid().iter().map(|&b| 0xDC00 + u32::from(b)));
    }
    out
}

fuzz_target!(|data: &[u8]| {
    let text = code_points(data);
    let _ = split_datetime(&text);
    let _ = parse_datetime(&text);
});
