//! `parse_tzinfo(text)`, then building and formatting the offset it describes.
#![no_main]

use isodate_port_core::pc_datetime::Value;
use isodate_port_core::tz::{fixed_offset_delta, format_offset, parse_tzinfo, TzSpec};
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
    let Ok(TzSpec::Fixed { hours, minutes, .. }) = parse_tzinfo(&text) else {
        return;
    };
    let Ok(delta) = fixed_offset_delta(&Value::int(hours), &Value::int(minutes)) else {
        return;
    };
    let seconds = i64::from(delta.days()) * 86_400 + i64::from(delta.seconds());
    for format in ["%Z", "%z", "%h", "%"] {
        let format: Vec<u32> = format.chars().map(u32::from).collect();
        let _ = format_offset(seconds, &format);
    }
});
