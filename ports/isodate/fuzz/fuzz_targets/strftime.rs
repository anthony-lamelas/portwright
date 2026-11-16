//! `isostrf.strftime(tdt, format, yeardigits)` for date/time, `timedelta` and
//! `Duration` targets, with attribute values of any numeric type (ints of any size,
//! floats including NaN and infinities, Decimals, non-numbers).
#![no_main]

use isodate_port_core::decimal::{Decimal, Exponent};
use isodate_port_core::strf::{is_negative, strftime, DateMethod, Num, StrfSource, Target};
use isodate_port_core::tz::{format_offset, OffsetFormat};
use isodate_port_core::{Error, PyStr};
use libfuzzer_sys::fuzz_target;
use num_bigint::BigInt;
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

/// Reads structured values off the front of the input, then hands back the rest.
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn byte(&mut self) -> u8 {
        let Some((&b, rest)) = self.0.split_first() else {
            return 0;
        };
        self.0 = rest;
        b
    }

    fn i64(&mut self) -> i64 {
        let mut bytes = [0u8; 8];
        for b in &mut bytes {
            *b = self.byte();
        }
        i64::from_le_bytes(bytes)
    }

    fn rest(&self) -> &'a [u8] {
        self.0
    }
}

const ATTRS: &[&str] = &[
    "year", "month", "day", "hour", "minute", "second", "microsecond", "days", "seconds",
    "microseconds", "years", "months",
];

/// A value read by `getattr` or a method call.
fn num(r: &mut Reader<'_>) -> Num {
    match r.byte() % 10 {
        0..=3 => Num::Int(BigInt::from(i64::from(r.byte() as i8))),
        4 => Num::Int(BigInt::from(r.i64())),
        5 => Num::Int(BigInt::from(r.i64()) << usize::from(r.byte() % 200)),
        6 => Num::Float(f64::from_bits(r.i64() as u64)),
        7 => Num::Float(f64::from(r.byte() as i8) / 4.0),
        8 => {
            let negative = r.byte() & 1 != 0;
            let digits: Vec<u8> = (0..r.byte() % 30).map(|_| r.byte() % 10).collect();
            let exponent = match r.byte() % 8 {
                0 => Exponent::Infinite,
                1 => Exponent::Nan,
                2 => Exponent::SNan,
                _ => Exponent::Finite(i64::from(r.byte() as i8)),
            };
            Decimal::from_parts(negative, &digits, exponent)
                .map_or_else(|_| Num::Other("Decimal".into()), Num::Decimal)
        }
        _ => Num::Other("str".into()),
    }
}

/// The object being formatted: each attribute is present or missing.
struct Source {
    attrs: Vec<(&'static str, Num)>,
    methods: [Num; 3],
    offset: Option<i64>,
}

impl StrfSource for Source {
    type Error = Error;

    fn attr(&self, name: &'static str) -> Result<Num, Error> {
        self.attrs
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.clone())
            .ok_or_else(|| Error::attribute(format!("object has no attribute '{name}'")))
    }

    fn call(&self, method: DateMethod) -> Result<Num, Error> {
        let index = match method {
            DateMethod::ToOrdinal => 0,
            DateMethod::IsoWeekday => 1,
            DateMethod::IsoWeek => 2,
        };
        Ok(self.methods[index].clone())
    }

    fn tz_isoformat(&self, format: OffsetFormat) -> Result<PyStr, Error> {
        let Some(seconds) = self.offset else {
            return Ok(PyStr::from_code_points(Vec::new()));
        };
        let format = match format {
            OffsetFormat::Extended => "%Z",
            OffsetFormat::Basic => "%z",
            OffsetFormat::Hours => "%h",
        };
        format_offset(seconds, &format.chars().map(u32::from).collect::<Vec<u32>>())
    }
}

/// `yeardigits`: the width of `%Y`/`%C`. Widths up to a few hundred, or past
/// `isize::MAX` (an error); widths in between only build gigabyte strings, as in Python.
fn yeardigits(r: &mut Reader<'_>) -> Num {
    match r.byte() % 8 {
        0..=3 => Num::Int(BigInt::from(i64::from(r.byte() as i8))),
        // |n| >= 2**63: past isize::MAX, so the width is rejected before any allocation.
        4 => Num::Int(-(BigInt::from(r.i64() as u64 | 1 << 63) << usize::from(r.byte() % 70))),
        5 => Num::Int(BigInt::from(r.i64() as u64 | 1 << 63) << usize::from(r.byte() % 70)),
        6 => Num::Float(match r.byte() % 4 {
            0 => f64::NAN,
            1 => f64::INFINITY,
            2 => -1e300,
            _ => f64::from(r.byte() as i8) / 3.0,
        }),
        _ => match num(r) {
            Num::Int(_) | Num::Float(_) => Num::Other("str".into()),
            Num::Decimal(d) if d.adjusted() > 3 => Num::Other("NoneType".into()),
            other => other,
        },
    }
}

fuzz_target!(|data: &[u8]| {
    let mut r = Reader(data);
    let target = match r.byte() % 3 {
        0 => Target::DateTime,
        1 => Target::TimeDelta,
        _ => Target::Duration,
    };
    let present = u16::from_le_bytes([r.byte(), r.byte()]);
    let mut attrs = Vec::new();
    for (i, &name) in ATTRS.iter().enumerate() {
        if present & (1 << i) == 0 {
            attrs.push((name, num(&mut r)));
        }
    }
    let methods = [num(&mut r), num(&mut r), num(&mut r)];
    let offset = (r.byte() & 1 != 0).then(|| r.i64() % 1_000_000);
    let yeardigits = yeardigits(&mut r);
    let _ = is_negative(&yeardigits);
    let source = Source {
        attrs,
        methods,
        offset,
    };
    let format = code_points(r.rest());
    let _ = strftime(&source, target, &format, &yeardigits);
});
