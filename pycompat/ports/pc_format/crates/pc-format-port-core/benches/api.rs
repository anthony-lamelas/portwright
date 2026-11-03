//! Core-only timings for the calls in `checks/benchmarks.json` (no Python boundary).
#![allow(
    clippy::approx_constant,
    reason = "the float literals mirror benchmarks.json; they are not pi or e"
)]

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use num_bigint::BigInt;
use pc_format_port_core as core;
use pc_format_port_core::Value;

fn big(text: &str) -> Value {
    Value::Int(BigInt::parse_bytes(text.as_bytes(), 10).unwrap_or_default())
}

fn api(c: &mut Criterion) {
    let fixed = (Value::Float(1_234_567.891_234), Value::str(",.2f"));
    c.bench_function("format_float_fixed", |b| {
        b.iter(|| core::format_value(black_box(&fixed.0), black_box(&fixed.1)));
    });

    let hex = (Value::int(3_735_928_559_u32), Value::str("#018_x"));
    c.bench_function("format_int_hex_padded", |b| {
        b.iter(|| core::format_value(black_box(&hex.0), black_box(&hex.1)));
    });

    let template = Value::str("{name:>10} | {0:08.3f} | {1!r:^12} | {2:,}");
    let args = Value::List(vec![
        Value::Float(3.14159),
        Value::str("abc"),
        Value::int(1_234_567),
    ]);
    let kwargs = Value::Dict(vec![(Value::str("name"), Value::str("total"))]);
    c.bench_function("str_format_template", |b| {
        b.iter(|| core::str_format(black_box(&template), black_box(&args), black_box(&kwargs)));
    });

    let row = Value::str("%-10s %5d %8.3f %x %r");
    let values = Value::Tuple(vec![
        Value::str("item"),
        Value::int(42),
        Value::Float(2.71828),
        Value::int(255),
        Value::str("q"),
    ]);
    c.bench_function("percent_format_row", |b| {
        b.iter(|| core::percent_format(black_box(&row), black_box(&values)));
    });

    let shortest = Value::Float(0.300_000_000_000_000_04);
    c.bench_function("float_repr_shortest", |b| {
        b.iter(|| core::float_repr(black_box(&shortest)));
    });

    let float_text = Value::str("  -1.2345678901234567e-123  ");
    c.bench_function("parse_float_text", |b| {
        b.iter(|| core::parse_float(black_box(&float_text)));
    });

    let int_text = Value::str("123456789012345678901234567890123456789012345678901234567890");
    let base = Value::int(10);
    c.bench_function("parse_int_big", |b| {
        b.iter(|| core::parse_int(black_box(&int_text), black_box(&base)));
    });

    let big_int = big(
        "123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890",
    );
    c.bench_function("int_str_big", |b| {
        b.iter(|| core::int_str(black_box(&big_int)));
    });

    let round = (Value::Float(2.675), Value::int(2));
    c.bench_function("round_float_digits", |b| {
        b.iter(|| core::round_number(black_box(&round.0), black_box(&round.1)));
    });

    let divmod = (Value::Float(-7.25), Value::Float(2.5));
    c.bench_function("div_mod_floats", |b| {
        b.iter(|| core::div_mod(black_box(&divmod.0), black_box(&divmod.1)));
    });
}

criterion_group!(benches, api);
criterion_main!(benches);
