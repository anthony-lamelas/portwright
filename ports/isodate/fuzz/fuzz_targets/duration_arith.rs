//! The `Duration` year/month arithmetic: `Number` operators, `fquotmod`,
//! `max_days_in_month` and `shift_date` (date + Duration) on ints and Decimals.
#![no_main]

use isodate_port_core::decimal::Decimal;
use isodate_port_core::duration::{
    fquotmod, max_days_in_month, shift_date, Direction, Number,
};
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

    /// A short text: a length byte, then that many bytes as code points.
    fn text(&mut self) -> Vec<u32> {
        let len = usize::from(self.byte() % 32).min(self.0.len());
        let (text, rest) = self.0.split_at(len);
        self.0 = rest;
        code_points(text)
    }

}

/// An int (small, 64-bit or huge) or a parsed Decimal.
fn number(r: &mut Reader<'_>) -> Number {
    match r.byte() % 6 {
        0 | 1 => Number::Int(BigInt::from(i64::from(r.byte() as i8))),
        2 => Number::Int(BigInt::from(r.i64())),
        3 => Number::Int(BigInt::from(r.i64()) << usize::from(r.byte() % 128)),
        // `int()` of a Decimal with a huge exponent is correct but takes minutes in
        // CPython too, so those are left to the `decimal` target.
        _ => match Decimal::parse(&r.text()) {
            Ok(d) if d.adjusted().abs() < 4_000 => Number::Decimal(d),
            _ => Number::Int(BigInt::from(0)),
        },
    }
}

fuzz_target!(|data: &[u8]| {
    let mut r = Reader(data);
    let a = number(&mut r);
    let b = number(&mut r);
    let c = number(&mut r);
    let _ = (a.add(&b), a.sub(&b), a.mul(&b), a.true_div(&b), a.rem(&b), a.neg());
    let _ = (a.equals(&b), a.compare(&b), a.is_true(), a.to_f64(), a.to_integral_floor());
    let _ = a.to_int();
    let _ = fquotmod(&a, &b, &c);
    let _ = max_days_in_month(&a, &b);
    let fields = [number(&mut r), number(&mut r), number(&mut r)];
    let direction = if r.byte() & 1 == 0 {
        Direction::Add
    } else {
        Direction::Subtract
    };
    let _ = shift_date(&a, &b, direction, |name| {
        Ok(match name {
            "year" => fields[0].clone(),
            "month" => fields[1].clone(),
            _ => fields[2].clone(),
        })
    });
});
