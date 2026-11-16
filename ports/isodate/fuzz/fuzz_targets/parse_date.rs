//! `parse_date(text)` with the default patterns (4 digit years), and
//! caller-supplied `defaultmonth`/`defaultday` of any type.
#![no_main]

use isodate_port_core::parse::{default_date_regexps, parse_date};
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

/// A `defaultmonth`/`defaultday` argument chosen by one byte.
fn default_value(byte: u8) -> Value {
    match byte % 8 {
        0..=3 => Value::int(i64::from(byte >> 3) - 4),
        4 => Value::None,
        5 => Value::Float(f64::from(byte >> 3)),
        6 => Value::Bool(byte & 8 != 0),
        _ => Value::int(i128::from(i64::MAX) + i128::from(byte)),
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&[month, day], rest)) = data.split_first_chunk::<2>() else {
        return;
    };
    let text = code_points(rest);
    let Ok(regexps) = default_date_regexps() else {
        return;
    };
    let _ = parse_date(&text, regexps, &Value::int(1), &Value::int(1));
    let _ = parse_date(&text, regexps, &default_value(month), &default_value(day));
});
