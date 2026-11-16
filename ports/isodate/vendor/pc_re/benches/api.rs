//! Criterion benchmarks mirroring `/work/checks/benchmarks.json`, called
//! through the core API (no Python boundary).
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use num_bigint::BigInt;
use pc_re_port_core::matching::Mode;
use pc_re_port_core::text::cps;
use pc_re_port_core::{api, scan, Match, Value};
use std::sync::Arc;

fn s(text: &str) -> Value {
    Value::Str(text.chars().map(u32::from).collect())
}

type ApiFn = fn(&[Value], &[(String, Value)]) -> pc_re_port_core::Result<Value>;

fn bench_api(c: &mut Criterion) {
    let cases: Vec<(&str, ApiFn, Vec<Value>)> = vec![
        (
            "search_word",
            api::search,
            vec![
                s(r"\b(?P<word>w\w+)\b"),
                s("the quick brown fox jumps over the lazy dog and walks away"),
            ],
        ),
        (
            "findall_numbers",
            api::findall,
            vec![
                s(r"-?\d+(?:\.\d+)?"),
                s("x=12, y=-3.5, z=1000, w=0.25, v=7; total 1020.75 over 12 items"),
            ],
        ),
        (
            "sub_template",
            api::sub,
            vec![
                s(r"(\w+)@(\w+)\.com"),
                s(r"<\2:\1>"),
                s("alice@example.com, bob@test.com; carol@mail.com"),
            ],
        ),
        (
            "split_csvish",
            api::split,
            vec![
                s(r"\s*[,;]\s*"),
                s("alpha, beta ;gamma,delta ; epsilon,zeta,eta ;theta"),
            ],
        ),
        (
            "fullmatch_iso_date",
            api::fullmatch,
            vec![
                s(r"(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})(?::(\d{2}))?"),
                s("2024-02-29T13:45:07"),
            ],
        ),
        (
            "ignorecase_unicode",
            api::findall,
            vec![
                s("(?i)straße|ſtrasse|[kσ]+"),
                s("STRASSE Straße ſtrasse KkK σΣς"),
            ],
        ),
        (
            "escape_text",
            api::escape,
            vec![s("path/to/file (copy) [1].txt? *.* $HOME ^x|y")],
        ),
        (
            "compile_verbose",
            api::compile,
            vec![
                s(r"(?x) ^ (?P<user>[\w.+-]+) @ (?P<host>[\w-]+ (?:\.[\w-]+)+) $"),
                Value::Int(BigInt::from(0)),
            ],
        ),
    ];
    for (name, f, args) in cases {
        c.bench_function(name, |b| b.iter(|| f(black_box(&args), &[])));
    }
}

/// The matcher alone, on patterns compiled up front (no argument handling or cache).
fn bench_engine(c: &mut Criterion) {
    let cases = [
        ("engine/fixed_cost", "a", "a", Mode::Match),
        (
            "engine/search_word",
            r"\b(?P<word>w\w+)\b",
            "the quick brown fox jumps over the lazy dog and walks away",
            Mode::Search,
        ),
        (
            "engine/fullmatch_iso_date",
            r"(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})(?::(\d{2}))?",
            "2024-02-29T13:45:07",
            Mode::FullMatch,
        ),
        (
            "engine/fixed_repeats_in_branch",
            r"(\d{2})x|(\d{2})-(\w{3})(?=\d{2})",
            "a1-b2 13-c 12-abc34x56-def78x",
            Mode::Search,
        ),
    ];
    for (name, pattern, subject, mode) in cases {
        let Ok(p) = api::compile_cached(&cps(pattern), &Value::Int(BigInt::from(0))) else {
            continue;
        };
        let subject: Arc<[u32]> = cps(subject).into();
        let len = subject.len();
        c.bench_function(name, |b| {
            b.iter(|| Match::run(black_box(&p), &subject, 0, len, mode))
        });
    }
}

/// Repeated searches (findall / split) on patterns compiled up front.
fn bench_scan(c: &mut Criterion) {
    let cases = [
        (
            "scan/findall_numbers",
            r"-?\d+(?:\.\d+)?",
            "x=12, y=-3.5, z=1000, w=0.25, v=7; total 1020.75 over 12 items",
            false,
        ),
        (
            "scan/split_csvish",
            r"\s*[,;]\s*",
            "alpha, beta ;gamma,delta ; epsilon,zeta,eta ;theta",
            true,
        ),
        (
            "scan/ignorecase_unicode",
            "(?i)stra\u{df}e|\u{17f}trasse|[k\u{3c3}]+",
            "STRASSE Stra\u{df}e \u{17f}trasse KkK \u{3c3}\u{3a3}\u{3c2}",
            false,
        ),
    ];
    for (name, pattern, subject, split) in cases {
        let Ok(p) = api::compile_cached(&cps(pattern), &Value::Int(BigInt::from(0))) else {
            continue;
        };
        let subject = cps(subject);
        let len = subject.len();
        c.bench_function(name, |b| {
            b.iter(|| {
                if split {
                    scan::split(black_box(&p), &subject, 0)
                } else {
                    scan::findall(black_box(&p), &subject, 0, len)
                }
            })
        });
    }
}

criterion_group!(benches, bench_api, bench_engine, bench_scan);
criterion_main!(benches);
