//! Core-side costs of the Python benchmarks in `/work/checks/benchmarks.json`.
#![allow(missing_docs, reason = "Criterion macros generate undocumented items")]

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use isodate_port_core::decimal::Decimal;
use isodate_port_core::parse;
use isodate_port_core::pc_datetime::Value;
use isodate_port_core::tz;

fn cps(s: &str) -> Vec<u32> {
    s.chars().map(u32::from).collect()
}

fn benches(c: &mut Criterion) {
    let one = Value::int(1);
    let Ok(dates) = parse::default_date_regexps() else {
        return;
    };
    for (name, text) in [
        ("parse_date_ext", "1985-04-12"),
        ("parse_date_week", "1985-W15-5"),
    ] {
        let text = cps(text);
        c.bench_function(name, |b| {
            b.iter(|| parse::parse_date(black_box(&text), dates, &one, &one));
        });
    }
    let time = cps("15:27:46.123456+01:00");
    c.bench_function("parse_time_tz", |b| {
        b.iter(|| parse::parse_time(black_box(&time)));
    });
    let datetime = cps("2012-10-12T08:29:46.069178Z");
    c.bench_function("parse_datetime_utc", |b| {
        b.iter(|| parse::parse_datetime(black_box(&datetime)));
    });
    for (name, text) in [
        ("parse_duration_full", "P3Y6M4DT12H30M5S"),
        ("parse_duration_td", "P1DT2H3M4.5S"),
    ] {
        let text = cps(text);
        c.bench_function(name, |b| {
            b.iter(|| parse::parse_duration(black_box(&text), true));
        });
    }
    // The pieces of `parse_duration`: matching and converting, then building the result.
    let full = cps("P3Y6M4DT12H30M5S");
    c.bench_function("parse_duration_text", |b| {
        b.iter(|| parse::parse_duration_text(black_box(&full)));
    });
    if let Ok(parsed) = parse::parse_duration_text(&full) {
        c.bench_function("duration_finish", |b| {
            b.iter(|| black_box(parsed.clone()).finish(true));
        });
    }
    for (name, text) in [("decimal_parse_int", "3"), ("decimal_parse_frac", "12.25")] {
        let text = cps(text);
        c.bench_function(name, |b| b.iter(|| Decimal::parse(black_box(&text))));
    }
    c.bench_function("fixed_offset_delta_ints", |b| {
        b.iter(|| tz::fixed_offset_delta_ints(black_box(-5), black_box(-30)));
    });
    let tzinfo = cps("-05:30");
    c.bench_function("parse_tzinfo_ext", |b| {
        b.iter(|| tz::parse_tzinfo(black_box(&tzinfo)));
    });
}

criterion_group!(group, benches);
criterion_main!(group);
