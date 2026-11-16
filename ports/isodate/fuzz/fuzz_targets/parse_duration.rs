//! `parse_duration(text, as_timedelta_if_possible)`, both designator and alternative
//! (`P<datetime>`) formats, and both result kinds.
#![no_main]

use isodate_port_core::parse::{
    duration_timedelta, parse_duration, parse_duration_text, DurationKind, DurationParse,
};
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
    let Some((&flag, rest)) = data.split_first() else {
        return;
    };
    let text = code_points(rest);
    let _ = parse_duration(&text, flag & 1 != 0);
    if let Ok(parsed) = parse_duration_text(&text) {
        if let DurationParse::Fields(fields) = &parsed {
            let _ = fields.is_day_based();
            let _ = duration_timedelta(fields, DurationKind::TimeDelta);
            let _ = duration_timedelta(fields, DurationKind::Duration);
            let _ = fields.to_timedelta();
            let _ = fields.to_duration();
        }
        let _ = parsed.finish(flag & 2 != 0);
    }
});
