//! `tz_isoformat`'s offset formatting: any offset in seconds, any format string, and
//! `FixedOffset`'s `timedelta(hours=, minutes=)` with arbitrary values.
#![no_main]

use isodate_port_core::pc_datetime::Value;
use isodate_port_core::tz::{fixed_offset_delta, format_offset, OffsetFormat, SplitOffset};
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
    let Some((head, rest)) = data.split_first_chunk::<8>() else {
        return;
    };
    let seconds = i64::from_le_bytes(*head);
    let format = code_points(rest);
    let _ = OffsetFormat::from_code_points(&format);
    let _ = format_offset(seconds, &format);
    if let Ok(split) = SplitOffset::from_seconds(seconds) {
        for format in [OffsetFormat::Extended, OffsetFormat::Basic, OffsetFormat::Hours] {
            let _ = split.format(format);
        }
    }
    let hours = Value::int(seconds >> 32);
    let minutes = Value::Float(f64::from(seconds as i32));
    let _ = fixed_offset_delta(&hours, &minutes);
    let _ = fixed_offset_delta(&Value::int(seconds), &Value::int(seconds.wrapping_neg()));
});
