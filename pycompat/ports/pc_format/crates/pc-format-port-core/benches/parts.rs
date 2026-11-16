//! Core-only timings for the pieces of the `checks/benchmarks.json` calls:
//! single fields and directives of the template benchmarks, and the
//! `parse_int` paths. Use these to see which part of a benchmark costs time.
#![allow(
    clippy::approx_constant,
    reason = "the float literals mirror benchmarks.json; they are not pi or e"
)]

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use pc_format_port_core as core;
use pc_format_port_core::Value;

fn format_fields(c: &mut Criterion) {
    let cases = [
        ("format int >10", Value::int(42), ">10"),
        ("format float .3e", Value::Float(6.022_140_76e23), ".3e"),
        ("format str ^12", Value::str("'widget'"), "^12"),
        ("format int empty", Value::int(7), ""),
        ("format float 08.3f", Value::Float(3.14159), "08.3f"),
        ("format int +d", Value::int(-17), "+d"),
    ];
    for (name, value, spec) in cases {
        let spec = Value::str(spec);
        c.bench_function(name, |b| {
            b.iter(|| core::format_value(black_box(&value), black_box(&spec)));
        });
    }
}

fn str_format_fields(c: &mut Criterion) {
    let args = Value::List(vec![Value::List(vec![Value::int(7)])]);
    let kwargs = Value::Dict(vec![(Value::str("name"), Value::str("widget"))]);
    for template in ["abc", "{0[0]}", "{name!r:^12}"] {
        let tv = Value::str(template);
        c.bench_function(&format!("str_format {template}"), |b| {
            b.iter(|| core::str_format(black_box(&tv), black_box(&args), black_box(&kwargs)));
        });
    }
}

fn percent_directives(c: &mut Criterion) {
    let values = Value::Dict(vec![
        (Value::str("label"), Value::str("label")),
        (Value::str("v"), Value::Float(3.14159)),
        (Value::str("n"), Value::int(-17)),
        (Value::str("h"), Value::int(255)),
        (Value::str("q"), Value::str("q")),
    ]);
    for template in [
        "abc",
        "%(label)-10s",
        "%(v)08.3f",
        "%(n)+d",
        "%(h)x",
        "%(q)r",
    ] {
        let tv = Value::str(template);
        c.bench_function(&format!("percent {template}"), |b| {
            b.iter(|| core::percent_format(black_box(&tv), black_box(&values)));
        });
    }
}

fn parse_int_paths(c: &mut Criterion) {
    let cases = [
        ("0xdeadbeefcafebabe", 0),
        ("deadbeefcafebabe", 16),
        ("0xdead_beef_cafe_babe_0123", 0),
        ("12345", 10),
    ];
    for (text, base) in cases {
        let tv = Value::str(text);
        let bv = Value::int(base);
        c.bench_function(&format!("parse_int {text}"), |b| {
            b.iter(|| core::parse_int(black_box(&tv), black_box(&bv)));
        });
    }
}

criterion_group!(
    benches,
    format_fields,
    str_format_fields,
    percent_directives,
    parse_int_paths
);
criterion_main!(benches);
