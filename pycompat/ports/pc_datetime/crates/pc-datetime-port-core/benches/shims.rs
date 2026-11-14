//! The Python-side benchmarks (`checks/benchmarks.json`), called on the core directly.
use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use pc_datetime_port_core::{
    date, datetime, isoparse, strftime, strptime, timedelta, timestamp, Kwargs, PyResult, Value,
};

type Shim = fn(&[Value], &Kwargs) -> PyResult<Value>;

fn ints(items: &[i64]) -> Value {
    Value::List(items.iter().map(|&n| Value::int(n)).collect())
}

fn bench(c: &mut Criterion, name: &str, shim: Shim, args: &[Value], kwargs: &Kwargs) {
    // A benchmark that raises would time the error path instead.
    assert!(shim(args, kwargs).is_ok(), "{name} raised");
    c.bench_function(name, |b| {
        b.iter(|| shim(black_box(args), black_box(kwargs)));
    });
}

fn shims(c: &mut Criterion) {
    let none: &Kwargs = &[];
    bench(
        c,
        "iso_parse_datetime",
        isoparse::datetime_fromisoformat,
        &[Value::str("2024-02-29T13:05:09.123456+05:30")],
        none,
    );
    bench(
        c,
        "iso_format_datetime",
        datetime::datetime_isoformat,
        &[ints(&[2024, 2, 29, 13, 5, 9, 123_456, 19800, 0])],
        &[("timespec".into(), Value::str("milliseconds"))],
    );
    bench(
        c,
        "strptime_full",
        strptime::strptime,
        &[
            Value::str("Thu Feb 29 13:05:09 2024 +0530"),
            Value::str("%a %b %d %H:%M:%S %Y %z"),
        ],
        none,
    );
    bench(
        c,
        "strftime_mixed",
        strftime::strftime,
        &[
            ints(&[2024, 2, 29, 13, 5, 9, 123_456, -18000, 0]),
            Value::str("%Y-%m-%dT%H:%M:%S.%f%z %A %j %U %G-W%V-%u"),
        ],
        none,
    );
    bench(
        c,
        "datetime_add",
        datetime::datetime_add,
        &[
            ints(&[2024, 2, 29, 13, 5, 9, 123_456, 3600, 0]),
            ints(&[400, 7261, 999_999]),
        ],
        none,
    );
    bench(
        c,
        "timedelta_float",
        timedelta::make_timedelta,
        &[1.5, 86_399.999_999_5, 0.25, 2.5, 1.75, 0.5, 3.25].map(Value::Float),
        none,
    );
    bench(
        c,
        "fromtimestamp",
        timestamp::datetime_fromtimestamp,
        &[Value::Float(1_709_211_909.123_456), Value::int(-34200)],
        none,
    );
    bench(
        c,
        "isocalendar",
        date::isocalendar,
        &[ints(&[2021, 1, 3])],
        none,
    );
}

criterion_group!(benches, shims);
criterion_main!(benches);
