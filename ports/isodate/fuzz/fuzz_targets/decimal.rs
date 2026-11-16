//! `Decimal(text)` and the decimal arithmetic durations use (default context), on
//! two parsed operands.
#![no_main]

use isodate_port_core::decimal::{Decimal, Rounding};
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

const ROUNDINGS: [Rounding; 8] = [
    Rounding::Down,
    Rounding::Up,
    Rounding::HalfUp,
    Rounding::HalfDown,
    Rounding::HalfEven,
    Rounding::Ceiling,
    Rounding::Floor,
    Rounding::Up05,
];

fuzz_target!(|data: &[u8]| {
    let Some((&selector, rest)) = data.split_first() else {
        return;
    };
    let (left, right) = match rest.iter().position(|&b| b == 0) {
        Some(at) => (&rest[..at], &rest[at + 1..]),
        None => (rest, &b"1"[..]),
    };
    let Ok(a) = Decimal::parse(&code_points(left)) else {
        return;
    };
    let _ = (a.is_nan(), a.is_snan(), a.is_infinite(), a.is_signed(), a.is_true());
    let _ = (a.adjusted(), a.is_integer(), a.to_f64());
    let (negative, digits, exponent) = a.to_parts();
    let _ = Decimal::from_parts(negative, &digits, exponent);
    let _ = (a.neg(), a.plus(), a.abs());
    let rounding = ROUNDINGS[usize::from(selector) % ROUNDINGS.len()];
    let _ = a.to_integral(rounding);
    // `int()` of a huge exponent is slow but correct; keep it to plausible sizes.
    if a.adjusted().abs() < 10_000 {
        let _ = a.to_bigint();
    }
    let Ok(b) = Decimal::parse(&code_points(right)) else {
        return;
    };
    let _ = (a.add(&b), a.sub(&b), a.mul(&b), a.div(&b), a.rem(&b));
    let _ = (a.equals(&b), a.compare(&b));
    let _ = a.quantize(&b, rounding);
});
