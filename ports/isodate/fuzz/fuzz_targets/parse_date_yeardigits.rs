//! `parse_date(text, yeardigits, expanded)`: builds the patterns for an arbitrary
//! `yeardigits` (including odd ones that make `{0}`, `{-1}` or huge repeats) and parses.
#![no_main]

use isodate_port_core::parse::{date_regexps, parse_date};
use isodate_port_core::pc_datetime::Value;
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

/// `yeardigits` values beyond the small range: repeat-count limits and overflow edges.
const EDGES: &[i64] = &[
    65_535,
    65_536,
    4_294_967_295,
    4_294_967_296,
    i64::MAX,
    i64::MIN + 2,
    -4_294_967_296,
];

fuzz_target!(|data: &[u8]| {
    let Some((&[selector, flags], rest)) = data.split_first_chunk::<2>() else {
        return;
    };
    let yeardigits = if selector >= 248 {
        EDGES[usize::from(selector - 248) % EDGES.len()]
    } else {
        i64::from(selector % 48) - 16
    };
    let Some(century) = yeardigits.checked_sub(2) else {
        return;
    };
    // `build_date_regexps`: any yeardigits other than 4 forces the sign.
    let expanded = yeardigits != 4 || flags & 1 != 0;
    let as_text = |n: i64| n.to_string().chars().map(u32::from).collect::<Vec<u32>>();
    let Ok(regexps) = date_regexps(expanded, &as_text(yeardigits), &as_text(century)) else {
        return;
    };
    let _ = regexps.patterns().count();
    let text = code_points(rest);
    let default = Value::int(i64::from(flags >> 1) - 8);
    let _ = parse_date(&text, &regexps, &Value::int(1), &Value::int(1));
    let _ = parse_date(&text, &regexps, &default, &default);
});
