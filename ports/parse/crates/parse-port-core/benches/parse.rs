//! Benchmarks mirroring the Python-level benchmarks: build, compile, match.
use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use indexmap::IndexMap;
use parse_port_core::convert::{self, DateArgs};
use parse_port_core::format::{ExtraType, Parser};
use parse_port_core::strptime;

const CASES: &[(&str, &str, &str)] = &[
    (
        "simple_parse",
        "hello {name}, you are {age:d}",
        "hello Bob, you are 42",
    ),
    (
        "numeric_mix",
        "{:f} {:e} {:g} {:x} {:n} {:%}",
        "3.14 1.5e10 2.5 ff 1,234,567 50%",
    ),
    (
        "iso_datetime",
        "at {when:ti} level={lvl:w}",
        "at 2023-11-21T13:23:27.123+05:30 level=INFO",
    ),
    (
        "strftime_field",
        "{dt:%Y-%m-%d %H:%M:%S}",
        "2023-11-21 13:23:27",
    ),
    (
        "compile_complex",
        "{a.b:>10d} {c[d]:^8} {e:tg} {:%H:%M} {f:.3f}",
        "",
    ),
    (
        "apache_log",
        "{ip} - - [{ts:th}] \"{method} {path} HTTP/{ver}\" {status:d} {size:d}",
        "127.0.0.1 - - [21/Nov/2011:10:21:36 +1000] \"GET /a/b HTTP/1.1\" 200 5123",
    ),
];

fn build(fmt: &str) -> Option<Parser> {
    let mut extra: IndexMap<String, ExtraType> = IndexMap::new();
    Parser::new(fmt, &mut extra, false).ok()
}

fn benches(c: &mut Criterion) {
    for &(name, fmt, text) in CASES {
        c.bench_function(&format!("{name}/new"), |b| b.iter(|| build(black_box(fmt))));
        c.bench_function(&format!("{name}/new+compile"), |b| {
            b.iter(|| {
                let p = build(black_box(fmt));
                black_box(p.map(|p| p.match_regex().is_ok()))
            })
        });
        c.bench_function(&format!("{name}/new_cached+compile"), |b| {
            b.iter(|| {
                Parser::new_cached(black_box(fmt), false)
                    .and_then(|p| p.match_regex())
                    .is_ok()
            })
        });
        if let Some(Ok(re)) = build(fmt).map(|p| p.match_regex()) {
            let chars: Vec<char> = text.chars().collect();
            c.bench_function(&format!("{name}/match"), |b| {
                b.iter(|| black_box(re.match_at(black_box(&chars), 0, chars.len())))
            });
        }
    }
}

/// `search` cases: the pattern doesn't match at most start positions.
fn searches(c: &mut Criterion) {
    let cases: &[(&str, &str, &str)] = &[
        (
            "search_log",
            "took {ms:d}ms",
            "GET /index.html 200 OK took 1532ms from 10.0.0.1 agent=curl",
        ),
        (
            "findall_numbers",
            "<{:d}>",
            "<1> <22> <333> <4444> <55555> <6> <77> <888> <9999> <0>",
        ),
    ];
    for &(name, fmt, text) in cases {
        if let Some(Ok(re)) = build(fmt).map(|p| p.search_regex()) {
            let chars: Vec<char> = text.chars().collect();
            c.bench_function(&format!("{name}/search"), |b| {
                b.iter(|| black_box(re.search(black_box(&chars), 0, chars.len())))
            });
        }
    }
}

fn conversions(c: &mut Criterion) {
    c.bench_function("convert/int", |b| {
        b.iter(|| convert::int_convert(black_box("1,234,567"), None))
    });
    c.bench_function("convert/percentage", |b| {
        b.iter(|| convert::percentage(black_box("50%")))
    });
    // `{:ti}`: groups of `2023-11-21T13:23:27.123+05:30`.
    let groups: Vec<Option<String>> = [
        Some("2023-11-21T13:23:27.123+05:30"),
        Some("2023-11-21"),
        Some("13:23:27.123"),
        Some("+05:30"),
    ]
    .iter()
    .map(|g| g.map(str::to_owned))
    .collect();
    let args = DateArgs {
        ymd: Some(Ok(1)),
        hms: Some(Ok(2)),
        tz: Some(Ok(3)),
        ..DateArgs::default()
    };
    c.bench_function("convert/date_iso", |b| {
        b.iter(|| convert::date_convert(black_box(&groups), black_box(&args)))
    });
    // `{:th}`: groups of `21/Nov/2011:10:21:36 +1000`.
    let th: Vec<Option<&str>> = vec![
        Some("21/Nov/2011:10:21:36 +1000"),
        Some("21/Nov/2011"),
        Some("10:21:36"),
        Some("+1000"),
    ];
    let th_args = DateArgs {
        dmy: Some(Ok(1)),
        hms: Some(Ok(2)),
        tz: Some(Ok(3)),
        ..DateArgs::default()
    };
    c.bench_function("convert/date_th", |b| {
        b.iter(|| convert::date_convert(black_box(&th), black_box(&th_args)))
    });
    c.bench_function("convert/strptime", |b| {
        b.iter(|| strptime::strptime(black_box("2023-11-21 13:23:27"), "%Y-%m-%d %H:%M:%S"))
    });
    c.bench_function("convert/strf_date", |b| {
        b.iter(|| {
            strptime::strf_date_convert(black_box("2023-11-21 13:23:27"), "%Y-%m-%d %H:%M:%S")
        })
    });
}

criterion_group!(group, benches, searches, conversions);
criterion_main!(group);
