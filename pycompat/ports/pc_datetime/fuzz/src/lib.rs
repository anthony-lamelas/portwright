//! Shared input decoding for the `pc-datetime-port-core` fuzz targets.
//!
//! Each target names its parameters and the shape each one expects. The decoder turns the
//! fuzzer's bytes into mostly well-formed shim arguments (date tuples, offsets, format
//! strings built from real directives) so inputs get past validation, and mixes in
//! near-misses: boundary integers, huge ints, NaN/inf/-0.0, lone surrogates, non-ASCII
//! digits, wrong types, and binding errors (missing, extra, duplicated or unknown keywords).
//! Expected Python exceptions are ignored; only panics count.

use libfuzzer_sys::arbitrary::Unstructured;
use num_bigint::BigInt;
use pc_datetime_port_core::{Kwargs, PyResult, PyStr, Value};

/// The shape a parameter expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Year,
    Month,
    Day,
    Hour,
    Minsec,
    Us,
    Offset,
    Fold,
    Number,
    Td,
    Divisor,
    Date,
    Time,
    Datetime,
    Ordinal,
    Week,
    Isoday,
    Timespec,
    Sep,
    Isotext,
    Format,
    StrptimeText,
    Timestamp,
}

/// A shim entry point: `fn(args, kwargs) -> result`.
pub type Api = fn(&[Value], &Kwargs) -> PyResult<Value>;

/// Decodes `data` into a call of `api` with parameters `params`, makes it, and renders
/// whatever comes back so the result and error formatting code runs too.
pub fn run(data: &[u8], params: &[(&str, Kind)], api: Api) {
    let mut u = Unstructured::new(data);
    let (args, kwargs) = call_args(&mut u, params);
    match api(&args, &kwargs) {
        Ok(v) => {
            let _ = v.repr();
        }
        Err(e) => {
            let _ = e.to_string();
            let _ = e.type_name();
        }
    }
}

fn byte(u: &mut Unstructured<'_>) -> u8 {
    u.arbitrary::<u8>().unwrap_or(0)
}

fn below(u: &mut Unstructured<'_>, n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    usize::from(byte(u)) % n
}

fn chance(u: &mut Unstructured<'_>, one_in: u8) -> bool {
    byte(u).is_multiple_of(one_in)
}

fn pick<'a, T>(u: &mut Unstructured<'_>, items: &'a [T]) -> &'a T {
    &items[below(u, items.len())]
}

fn call_args(
    u: &mut Unstructured<'_>,
    params: &[(&str, Kind)],
) -> (Vec<Value>, Vec<(PyStr, Value)>) {
    let mut values: Vec<Value> = params.iter().map(|&(_, k)| param_value(u, k)).collect();
    // Drop trailing arguments so defaults (or missing-argument errors) are exercised.
    if chance(u, 3) {
        let keep = below(u, values.len() + 1);
        values.truncate(keep);
    }
    // Split into positional and keyword arguments.
    let n_pos = if chance(u, 2) {
        values.len()
    } else {
        below(u, values.len() + 1)
    };
    let mut args = Vec::new();
    let mut kwargs = Vec::new();
    for (i, v) in values.into_iter().enumerate() {
        if i < n_pos {
            args.push(v);
        } else {
            kwargs.push((PyStr::from(params[i].0), v));
        }
    }
    if chance(u, 4) && !kwargs.is_empty() {
        // Keyword order should never matter for valid calls.
        let i = below(u, kwargs.len());
        let j = below(u, kwargs.len());
        kwargs.swap(i, j);
    }
    // Binding near-misses.
    match byte(u) % 24 {
        0 => args.push(any_value(u, 2)),
        1 => kwargs.push((
            PyStr::from(*pick(u, &["x", "self", "", "kwargs", "\u{e9}"])),
            any_value(u, 1),
        )),
        2 if !params.is_empty() => {
            // Duplicate a parameter given positionally (or twice as a keyword).
            let name = params[below(u, params.len())].0.to_string();
            kwargs.push((name.into(), any_value(u, 1)));
        }
        3 => args.insert(0, any_value(u, 1)),
        _ => {}
    }
    (args, kwargs)
}

fn param_value(u: &mut Unstructured<'_>, kind: Kind) -> Value {
    // Occasionally anything at all, so type errors are reached from every parameter.
    if chance(u, 10) {
        return any_value(u, 2);
    }
    match kind {
        Kind::Year => ranged(
            u,
            &[
                1, 2, 4, 1000, 1582, 1970, 2000, 2024, 9998, 9999, 10000, 0, -1,
            ],
            1,
            9999,
        ),
        Kind::Month => ranged(u, &[1, 2, 12, 0, 13, -1], 1, 12),
        Kind::Day => ranged(u, &[1, 28, 29, 30, 31, 0, 32], 1, 31),
        Kind::Hour => ranged(u, &[0, 23, 24, -1], 0, 23),
        Kind::Minsec => ranged(u, &[0, 59, 60, -1], 0, 59),
        Kind::Us => ranged(u, &[0, 1, 999_999, 1_000_000, 500_000, -1], 0, 999_999),
        Kind::Offset => offset(u),
        Kind::Fold => ranged(u, &[0, 1, 2, -1], 0, 1),
        Kind::Number => number(u),
        Kind::Td => td(u),
        Kind::Divisor => {
            if chance(u, 2) {
                td(u)
            } else {
                number(u)
            }
        }
        Kind::Date => tuple_of(u, &[Kind::Year, Kind::Month, Kind::Day]),
        Kind::Time => tuple_of(
            u,
            &[
                Kind::Hour,
                Kind::Minsec,
                Kind::Minsec,
                Kind::Us,
                Kind::Offset,
                Kind::Fold,
            ],
        ),
        Kind::Datetime => tuple_of(
            u,
            &[
                Kind::Year,
                Kind::Month,
                Kind::Day,
                Kind::Hour,
                Kind::Minsec,
                Kind::Minsec,
                Kind::Us,
                Kind::Offset,
                Kind::Fold,
            ],
        ),
        Kind::Ordinal => ranged(u, &[1, 0, -1, 730_120, 3_652_059, 3_652_060], 1, 3_652_059),
        Kind::Week => ranged(u, &[1, 52, 53, 54, 0, -1], 1, 53),
        Kind::Isoday => ranged(u, &[1, 7, 0, 8, -1], 1, 7),
        Kind::Timespec => {
            if chance(u, 4) {
                Value::Str(token_text(
                    u,
                    &[
                        &[
                            "auto",
                            "hours",
                            "minutes",
                            "seconds",
                            "milliseconds",
                            "microseconds",
                        ],
                        ISO_TOKENS,
                    ],
                    3,
                ))
            } else {
                Value::str(*pick(
                    u,
                    &[
                        "auto",
                        "hours",
                        "minutes",
                        "seconds",
                        "milliseconds",
                        "microseconds",
                        "AUTO",
                        "",
                        "nanoseconds",
                    ],
                ))
            }
        }
        Kind::Sep => {
            if chance(u, 3) {
                Value::Str(arbitrary_str(u, 3))
            } else {
                Value::str(*pick(
                    u,
                    &["T", " ", "t", "x", "\u{e9}", "\u{1f600}", "", "TT"],
                ))
            }
        }
        Kind::Isotext => Value::Str(iso_text(u)),
        Kind::Format => Value::Str(format_text(u)),
        Kind::StrptimeText => Value::Str(token_text(u, &[STRPTIME_TOKENS, ISO_TOKENS], 24)),
        Kind::Timestamp => timestamp(u),
    }
}

/// A tuple of the given shapes, sometimes a list or a wrong length.
fn tuple_of(u: &mut Unstructured<'_>, kinds: &[Kind]) -> Value {
    let mut items: Vec<Value> = kinds.iter().map(|&k| param_value(u, k)).collect();
    match byte(u) % 16 {
        0 => {
            let keep = below(u, items.len());
            items.truncate(keep);
        }
        1 => items.push(any_value(u, 1)),
        2 => return Value::List(items),
        _ => {}
    }
    Value::Tuple(items)
}

/// Mostly an in-range int; sometimes a listed edge, a bool, a float, or a huge int.
fn ranged(u: &mut Unstructured<'_>, edges: &[i64], lo: i64, hi: i64) -> Value {
    match byte(u) % 16 {
        0..=7 => {
            let span = u64::try_from(hi - lo + 1).unwrap_or(1);
            let r = u.arbitrary::<u32>().unwrap_or(0);
            Value::int(lo + i64::try_from(u64::from(r) % span).unwrap_or(0))
        }
        8..=11 => Value::int(*pick(u, edges)),
        12 => Value::Bool(chance(u, 2)),
        13 => Value::Float(f64::from(i32::try_from(*pick(u, edges)).unwrap_or(0))),
        14 => big_int(u),
        _ => Value::None,
    }
}

fn offset(u: &mut Unstructured<'_>) -> Value {
    match byte(u) % 10 {
        0..=2 => Value::None,
        3..=5 => {
            let r = u.arbitrary::<u32>().unwrap_or(0) % 172_799;
            Value::int(i64::from(r) - 86_399)
        }
        6 => Value::int(*pick(
            u,
            &[
                0, 3600, -3600, 19_800, 86_399, -86_399, 86_400, -86_400, 1, -1, 59, 61,
            ],
        )),
        7 => Value::int(i64::from(u.arbitrary::<i8>().unwrap_or(0)) * 900),
        8 => big_int(u),
        _ => Value::Float(*pick(u, &[0.0, 3600.0, 0.5, f64::NAN])),
    }
}

fn number(u: &mut Unstructured<'_>) -> Value {
    match byte(u) % 10 {
        0..=3 => Value::int(i64::from(u.arbitrary::<i32>().unwrap_or(0))),
        4 => Value::int(*pick(
            u,
            &[
                0,
                1,
                -1,
                2,
                86_400,
                999_999_999,
                1_000_000_000,
                -999_999_999,
                1_000_000,
            ],
        )),
        5 => big_int(u),
        6 => Value::Float(f64::from_bits(u.arbitrary::<u64>().unwrap_or(0))),
        7 => Value::Float(*pick(
            u,
            &[
                0.0,
                -0.0,
                0.5,
                1.5,
                2.5,
                -0.5,
                1e-7,
                1e18,
                1e300,
                f64::NAN,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::MIN_POSITIVE,
                5e-324,
            ],
        )),
        8 => Value::Float(f64::from(u.arbitrary::<i32>().unwrap_or(0)) / 1024.0),
        _ => Value::Bool(chance(u, 2)),
    }
}

fn td(u: &mut Unstructured<'_>) -> Value {
    let days = ranged(
        u,
        &[0, -1, 999_999_999, -999_999_999, 1_000_000_000, 3_652_059],
        -1_000_000,
        1_000_000,
    );
    let seconds = ranged(u, &[0, 86_399, 86_400, -1], 0, 86_399);
    let us = ranged(u, &[0, 999_999, 1_000_000, -1], 0, 999_999);
    let mut items = vec![days, seconds, us];
    match byte(u) % 16 {
        0 => {
            items.pop();
        }
        1 => return Value::List(items),
        _ => {}
    }
    Value::Tuple(items)
}

fn timestamp(u: &mut Unstructured<'_>) -> Value {
    match byte(u) % 8 {
        0..=2 => Value::int(u.arbitrary::<i64>().unwrap_or(0) % 253_402_300_800),
        3 => Value::Float(f64::from_bits(u.arbitrary::<u64>().unwrap_or(0))),
        4 => {
            let whole = u.arbitrary::<i64>().unwrap_or(0) % 253_402_300_800;
            #[expect(
                clippy::cast_precision_loss,
                reason = "any nearby float is a fine input"
            )]
            let base = whole as f64;
            Value::Float(base + f64::from(u.arbitrary::<u32>().unwrap_or(0)) / f64::from(u32::MAX))
        }
        5 => Value::Float(*pick(
            u,
            &[
                -0.0,
                0.5,
                -0.5,
                1e-7,
                -1e-7,
                0.999_999_5,
                f64::NAN,
                f64::INFINITY,
                1e20,
                -62_135_596_800.0,
                253_402_300_799.999_999,
            ],
        )),
        6 => big_int(u),
        _ => Value::int(*pick::<i64>(
            u,
            &[
                0,
                -1,
                -62_135_596_800,
                -62_135_596_801,
                253_402_300_799,
                253_402_300_800,
            ],
        )),
    }
}

fn big_int(u: &mut Unstructured<'_>) -> Value {
    let len = below(u, 13);
    let bytes: Vec<u8> = (0..len).map(|_| byte(u)).collect();
    let mut n = BigInt::from_signed_bytes_le(&bytes);
    if chance(u, 4) {
        // Exact powers of two sit on C int/long boundaries.
        n = BigInt::from(1) << below(u, 90);
        if chance(u, 2) {
            n = -n;
        }
        if chance(u, 2) {
            n -= 1;
        }
    }
    Value::Int(n)
}

const ISO_TOKENS: &[&str] = &[
    "0",
    "1",
    "2",
    "9",
    "00",
    "01",
    "12",
    "23",
    "24",
    "29",
    "31",
    "59",
    "60",
    "2024",
    "0001",
    "9999",
    "1970",
    "0000",
    "123456",
    "999999",
    "1234567",
    "-",
    "-",
    ":",
    ":",
    "T",
    "t",
    " ",
    "W",
    "W53",
    ".",
    ",",
    "+",
    "-",
    "Z",
    "z",
    "+05:30",
    "-00:00",
    "+24:00",
    "+0530",
    "+05:30:15",
    ".123456",
    ".5",
    "\u{0663}",
    "\u{ff11}",
    "\u{e9}",
    "\\ud800",
    "\u{0}",
    "UTC",
    "\u{1f600}",
    "\u{2028}",
];

const STRPTIME_TOKENS: &[&str] = &[
    "Mon",
    "Monday",
    "tue",
    "SUNDAY",
    "Jan",
    "January",
    "feb",
    "DEC",
    "sept",
    "AM",
    "PM",
    "am",
    "pm",
    "UTC",
    "GMT",
    "utc",
    "Z",
    "+0000",
    "-0530",
    "+05:30",
    "+05:30:15.123",
    "-24:00",
    "z",
    " ",
    "  ",
    "\t",
    "\n",
    "%",
    "/",
    "1",
    "12",
    "001",
    "366",
    "53",
    "7",
    "0",
    "99",
    "1999",
    "\u{0663}",
    "\\ud83d",
    "\u{2003}",
    "\u{130}",
    "\u{17f}",
    "K",
];

const DIRECTIVES: &[char] = &[
    'a', 'A', 'b', 'B', 'c', 'd', 'D', 'e', 'f', 'F', 'g', 'G', 'h', 'H', 'I', 'j', 'k', 'l', 'm',
    'M', 'n', 'p', 'P', 'r', 'R', 's', 'S', 't', 'T', 'u', 'U', 'V', 'w', 'W', 'x', 'X', 'y', 'Y',
    'z', 'Z', '%', ':', 'C', 'E', 'O', '+', 'q', 'Q', 'i', '\u{e9}', ' ',
];

fn format_text(u: &mut Unstructured<'_>) -> PyStr {
    let n = below(u, 12);
    let mut points = Vec::new();
    for _ in 0..n {
        match byte(u) % 8 {
            0..=4 => {
                points.push(u32::from('%'));
                // glibc flags and widths.
                if chance(u, 4) {
                    points.push(u32::from(*pick(
                        u,
                        &['-', '_', '0', '^', '#', 'E', 'O', ':'],
                    )));
                }
                if chance(u, 6) {
                    for c in below(u, 40).to_string().chars() {
                        points.push(u32::from(c));
                    }
                }
                if !chance(u, 30) {
                    points.push(u32::from(*pick(u, DIRECTIVES)));
                }
            }
            5 => push_str(&mut points, pick(u, STRPTIME_TOKENS)),
            6 => push_str(&mut points, pick(u, ISO_TOKENS)),
            _ => points.extend(arbitrary_str(u, 2).code_points()),
        }
    }
    PyStr::from_code_points(points)
}

fn iso_text(u: &mut Unstructured<'_>) -> PyStr {
    if chance(u, 2) {
        return token_text(u, &[ISO_TOKENS], 14);
    }
    // A well-formed date/time/datetime, then possibly perturbed.
    let mut s = String::new();
    let digits = |u: &mut Unstructured<'_>, n: usize| -> String {
        (0..n).map(|_| char::from(b'0' + byte(u) % 10)).collect()
    };
    let has_date = !chance(u, 3);
    if has_date {
        match byte(u) % 4 {
            0 => s = format!("{}-{}-{}", digits(u, 4), digits(u, 2), digits(u, 2)),
            1 => s = format!("{}{}{}", digits(u, 4), digits(u, 2), digits(u, 2)),
            2 => s = format!("{}-W{}-{}", digits(u, 4), digits(u, 2), digits(u, 1)),
            _ => s = format!("{}W{}{}", digits(u, 4), digits(u, 2), digits(u, 1)),
        }
    }
    if !has_date || chance(u, 2) {
        if has_date {
            s.push(*pick(u, &['T', ' ', 't', 'x', '\u{e9}']));
        }
        let sep = !chance(u, 3);
        s.push_str(&digits(u, 2));
        for _ in 0..below(u, 3) {
            if sep {
                s.push(':');
            }
            s.push_str(&digits(u, 2));
        }
        if chance(u, 2) {
            s.push(*pick(u, &['.', ',']));
            let n = below(u, 10);
            s.push_str(&digits(u, n));
        }
        match byte(u) % 4 {
            0 => s.push('Z'),
            1 => {
                s.push(*pick(u, &['+', '-']));
                s.push_str(&digits(u, 2));
                if chance(u, 2) {
                    s.push(':');
                }
                let n = *pick(u, &[0, 2, 4, 5]);
                s.push_str(&digits(u, n));
            }
            _ => {}
        }
    }
    let mut points: Vec<u32> = s.chars().map(u32::from).collect();
    if chance(u, 4) && !points.is_empty() {
        // Swap one character for a token: a non-ASCII digit, a surrogate, or a separator.
        let i = below(u, points.len());
        let mut tok = Vec::new();
        push_str(&mut tok, pick(u, ISO_TOKENS));
        points.splice(i..=i, tok);
    }
    if chance(u, 6) {
        points.truncate(below(u, points.len() + 1));
    }
    PyStr::from_code_points(points)
}

fn token_text(u: &mut Unstructured<'_>, tables: &[&[&str]], max: usize) -> PyStr {
    let n = below(u, max + 1);
    let mut points = Vec::new();
    for _ in 0..n {
        if chance(u, 12) {
            points.extend(arbitrary_str(u, 2).code_points());
        } else {
            let table = pick(u, tables);
            push_str(&mut points, pick(u, table));
        }
    }
    PyStr::from_code_points(points)
}

/// Appends a token. The tables can't hold lone surrogates, so `\\ud800` and `\\ud83d`
/// (backslash, then the hex) stand for those code points.
fn push_str(points: &mut Vec<u32>, s: &str) {
    match s {
        "\\ud800" => points.push(0xD800),
        "\\ud83d" => points.push(0xD83D),
        _ => points.extend(s.chars().map(u32::from)),
    }
}

/// Any code points, including lone surrogates and astral characters.
fn arbitrary_str(u: &mut Unstructured<'_>, max: usize) -> PyStr {
    let n = below(u, max + 1);
    let points = (0..n)
        .map(|_| match byte(u) % 4 {
            0 => u32::from(byte(u) & 0x7f),
            1 => 0xD800 + u32::from(u.arbitrary::<u16>().unwrap_or(0)) % 0x800,
            _ => u.arbitrary::<u32>().unwrap_or(0) % 0x11_0000,
        })
        .collect();
    PyStr::from_code_points(points)
}

/// Any value of any accepted type (plus a non-built-in), nested up to `depth`.
fn any_value(u: &mut Unstructured<'_>, depth: u8) -> Value {
    let top = if depth == 0 { 9 } else { 15 };
    match below(u, top) {
        0 => Value::None,
        1 => Value::Bool(chance(u, 2)),
        2 => Value::int(i64::from(u.arbitrary::<i16>().unwrap_or(0))),
        3 => big_int(u),
        4 => number(u),
        5 => Value::Str(arbitrary_str(u, 6)),
        6 => Value::Str(token_text(u, &[ISO_TOKENS, STRPTIME_TOKENS], 4)),
        7 => Value::Bytes((0..below(u, 5)).map(|_| byte(u)).collect()),
        8 => Value::Other(pick(u, &["Decimal", "object", "datetime"]).to_string()),
        9 => Value::ByteArray((0..below(u, 5)).map(|_| byte(u)).collect()),
        10 => Value::Tuple(items(u, depth)),
        11 => Value::List(items(u, depth)),
        12 => Value::Set(items(u, depth)),
        13 => Value::FrozenSet(items(u, depth)),
        _ => {
            let n = below(u, 4);
            Value::Dict(
                (0..n)
                    .map(|_| (any_value(u, depth - 1), any_value(u, depth - 1)))
                    .collect(),
            )
        }
    }
}

fn items(u: &mut Unstructured<'_>, depth: u8) -> Vec<Value> {
    let n = below(u, 10);
    (0..n).map(|_| any_value(u, depth - 1)).collect()
}
