//! Shared input generation for the `pc-format-port-core` fuzz targets.
//!
//! Every target decodes its raw fuzz bytes into structured [`Value`]s with
//! [`Gen`], calls one public API, and ignores the `PcResult`: Python
//! exceptions are expected outcomes. Only panics (including arithmetic
//! overflow, slice indexing, `unwrap`) and aborts count as crashes.
//!
//! Output-size safety: CPython-style outputs can legitimately be ~1 GB
//! (width up to the core's `MemoryError` budget). To keep the fuzzer from
//! hitting libFuzzer's RSS limit on *correct* behavior, generators used for
//! formatting targets cap runs of decimal digits (widths/precisions stay
//! small) and use ints that are either small or far beyond the budget.

use arbitrary::Unstructured;
use num_bigint::{BigInt, Sign};
use pc_format_port_core::{decimal_digit_value, PyStr, Value};

/// Characters that drive parsing in format specs, templates, int/float text.
const SYNTAX: &[u8] = b"{}{}[]!:.,_%%#0-+ *()=<>^srafdixXoObBcCeEgGnzZ%.0123456789_jJ\t\nlLhq";

/// Interesting non-ASCII code points: Unicode digits, whitespace, wide
/// chars, characters that changed between Unicode versions, astral.
const SPECIAL: &[u32] = &[
    0x00A0, 0x00B2, 0x0660, 0x0663, 0x06F5, 0x07C0, 0x0966, 0x0BEF, 0x0E50, 0x1040, 0x17E0,
    0x1810, 0x1946, 0x19D0, 0x1A80, 0x1B50, 0x1C40, 0xA620, 0xA8D0, 0xA900, 0xFF10, 0xFF15,
    0xFF19, 0x104A0, 0x1D7CE, 0x1D7FF, 0x1E950, 0x1FBF0, 0x1FBF9, 0x11F50, 0x16AC0, 0x1E4F0,
    0x2000, 0x2007, 0x200B, 0x2028, 0x2029, 0x202F, 0x205F, 0x3000, 0x1680, 0x0085, 0x001C,
    0x001F, 0x00E9, 0x0100, 0x0394, 0x4E00, 0xFFFD, 0xFEFF, 0x10FFFF, 0x1F600, 0xD800, 0xDBFF,
    0xDC00, 0xDFFF, 0x0130, 0x00DF, 0x2212, 0x221E, 0x0131,
];

/// Byte-driven value generator.
pub struct Gen<'a> {
    u: Unstructured<'a>,
    /// Longest run of consecutive decimal digits any generated text or
    /// bytes may contain (`usize::MAX` = unlimited).
    max_digit_run: usize,
}

impl<'a> Gen<'a> {
    /// A generator for parsing targets: digit runs unlimited.
    pub fn new(data: &'a [u8]) -> Self {
        Gen {
            u: Unstructured::new(data),
            max_digit_run: usize::MAX,
        }
    }

    /// A generator for formatting targets: widths/precisions stay < 1000.
    pub fn bounded(data: &'a [u8]) -> Self {
        Gen {
            u: Unstructured::new(data),
            max_digit_run: 3,
        }
    }

    pub fn byte(&mut self) -> u8 {
        self.u.arbitrary::<u8>().unwrap_or(0)
    }

    pub fn pick(&mut self, n: u8) -> u8 {
        if n == 0 {
            0
        } else {
            self.byte() % n
        }
    }

    pub fn flag(&mut self) -> bool {
        self.byte() & 1 == 1
    }

    fn len(&mut self, max: usize) -> usize {
        let b = usize::from(self.byte());
        if max == 0 {
            0
        } else {
            b % (max + 1)
        }
    }

    // ---- ints ---------------------------------------------------------

    /// An int that is safe to use as a width: |n| <= 999, a boundary, or
    /// >= 2**31 in magnitude (beyond every allocation budget).
    pub fn bigint(&mut self) -> BigInt {
        match self.pick(8) {
            0..=3 => {
                let small = i16::from_le_bytes([self.byte(), self.byte()]);
                BigInt::from(small % 1000)
            }
            4 => BigInt::from([0i64, 1, -1, 2, 10, 16, 36, 37, -2, 255, 256, 127][usize::from(self.pick(12))]),
            5 => {
                let edges: [i128; 14] = [
                    1 << 31,
                    -(1 << 31),
                    1 << 32,
                    i64::MAX as i128,
                    i64::MIN as i128,
                    (i64::MAX as i128) + 1,
                    u64::MAX as i128,
                    (u64::MAX as i128) + 1,
                    1 << 53,
                    (1 << 53) + 1,
                    -(1 << 53) - 1,
                    i128::MAX,
                    i128::MIN,
                    -(1 << 63),
                ];
                BigInt::from(edges[usize::from(self.pick(14))])
            }
            6 => {
                // 10**k +- 1, around float/precision boundaries.
                let k = 10 + u32::from(self.byte()) + u32::from(self.pick(80));
                let p = num_bigint::BigInt::from(10u8).pow(k);
                match self.pick(3) {
                    0 => p,
                    1 => p - 1,
                    _ => -p,
                }
            }
            _ => {
                // 5..=600 random bytes, forced to be >= 2**32.
                let n = 5 + self.len(595);
                let mut bytes: Vec<u8> = (0..n).map(|_| self.byte()).collect();
                if let Some(last) = bytes.last_mut() {
                    *last |= 1;
                }
                let sign = if self.flag() { Sign::Minus } else { Sign::Plus };
                BigInt::from_bytes_le(sign, &bytes)
            }
        }
    }

    // ---- floats -------------------------------------------------------

    pub fn float(&mut self) -> f64 {
        match self.pick(5) {
            0 => {
                const SPECIALS: [f64; 24] = [
                    0.0,
                    -0.0,
                    f64::NAN,
                    -f64::NAN,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::MAX,
                    f64::MIN,
                    f64::MIN_POSITIVE,
                    5e-324,
                    -5e-324,
                    1e16,
                    1e15,
                    9007199254740993.0,
                    0.5,
                    1.5,
                    2.5,
                    -2.5,
                    2.675,
                    0.1,
                    1e22,
                    1e23,
                    123456789.0,
                    0.3,
                ];
                SPECIALS[usize::from(self.pick(24))]
            }
            1 => {
                let n = i32::from(i16::from_le_bytes([self.byte(), self.byte()]));
                let scale = 10f64.powi(i32::from(self.pick(8)));
                f64::from(n) / scale
            }
            2 => {
                let n = i32::from(i16::from_le_bytes([self.byte(), self.byte()]));
                f64::from(n) + 0.5
            }
            _ => {
                let mut b = [0u8; 8];
                for x in &mut b {
                    *x = self.byte();
                }
                f64::from_le_bytes(b)
            }
        }
    }

    // ---- text ---------------------------------------------------------

    /// One code point, biased toward syntax characters.
    fn code_point(&mut self) -> u32 {
        match self.pick(10) {
            0..=5 => u32::from(SYNTAX[usize::from(self.byte()) % SYNTAX.len()]),
            6 => u32::from(self.byte() & 0x7F),
            7 => SPECIAL[usize::from(self.byte()) % SPECIAL.len()],
            8 => 0xD800 + u32::from(u16::from_le_bytes([self.byte(), self.byte()]) & 0x7FF),
            _ => {
                let v = u32::from_le_bytes([self.byte(), self.byte(), self.byte() & 0x1F, 0]);
                v % 0x11_0000
            }
        }
    }

    fn is_digit(cp: u32) -> bool {
        char::from_u32(cp).and_then(decimal_digit_value).is_some()
    }

    /// Appends `cp`, replacing it with `'x'` if it would make a digit run
    /// longer than allowed.
    fn push_cp(&self, out: &mut Vec<u32>, cp: u32) {
        let cp = if self.max_digit_run != usize::MAX && Self::is_digit(cp) {
            let run = out.iter().rev().take_while(|&&c| Self::is_digit(c)).count();
            if run >= self.max_digit_run {
                u32::from('x')
            } else {
                cp
            }
        } else {
            cp
        };
        out.push(cp);
    }

    fn push_ascii(&self, out: &mut Vec<u32>, s: &str) {
        for c in s.chars() {
            self.push_cp(out, u32::from(c));
        }
    }

    /// Free-form text of up to `max` code points.
    pub fn text(&mut self, max: usize) -> PyStr {
        let n = self.len(max);
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            let cp = self.code_point();
            self.push_cp(&mut out, cp);
        }
        PyStr::from_code_points(out)
    }

    /// A format spec following `[[fill]align][sign][z][#][0][width][grouping][.prec][type]`,
    /// each part optional, sometimes with a stray character.
    pub fn spec(&mut self) -> PyStr {
        if self.pick(6) == 0 {
            return self.text(12);
        }
        let mut out = Vec::new();
        self.spec_into(&mut out);
        PyStr::from_code_points(out)
    }

    fn spec_into(&mut self, out: &mut Vec<u32>) {
        if self.flag() {
            if self.flag() {
                let fill = self.code_point();
                self.push_cp(out, fill);
            }
            let a = ["<", ">", "^", "="][usize::from(self.pick(4))];
            self.push_ascii(out, a);
        }
        if self.flag() {
            let s = ["+", "-", " "][usize::from(self.pick(3))];
            self.push_ascii(out, s);
        }
        if self.pick(4) == 0 {
            self.push_ascii(out, "z");
        }
        if self.pick(3) == 0 {
            self.push_ascii(out, "#");
        }
        if self.pick(3) == 0 {
            self.push_ascii(out, "0");
        }
        if self.flag() {
            self.digits_into(out);
        }
        if self.pick(3) == 0 {
            let g = [",", "_", ",_", "__"][usize::from(self.pick(4))];
            self.push_ascii(out, g);
        }
        if self.flag() {
            self.push_ascii(out, ".");
            if self.pick(5) != 0 {
                self.digits_into(out);
            }
        }
        if self.flag() {
            let t = ["b", "c", "d", "e", "E", "f", "F", "g", "G", "n", "o", "s", "x", "X", "%", "r", "a", "i"]
                [usize::from(self.pick(18))];
            self.push_ascii(out, t);
        }
        if self.pick(8) == 0 {
            let cp = self.code_point();
            self.push_cp(out, cp);
        }
    }

    fn digits_into(&mut self, out: &mut Vec<u32>) {
        let n = 1 + self.len(4);
        for _ in 0..n {
            let d = if self.pick(10) == 0 {
                SPECIAL[usize::from(self.pick(30))] // mostly Unicode digits
            } else {
                u32::from(b'0' + self.pick(10))
            };
            self.push_cp(out, d);
        }
    }

    /// A `str.format` template: literal text and `{field[!conv][:spec]}`
    /// pieces, with occasional nested fields and stray braces.
    pub fn format_template(&mut self) -> PyStr {
        if self.pick(8) == 0 {
            return self.text(40);
        }
        let mut out = Vec::new();
        let pieces = self.len(6);
        for _ in 0..pieces {
            match self.pick(10) {
                0..=1 => {
                    let t = self.text(6);
                    for &cp in t.code_points() {
                        self.push_cp(&mut out, cp);
                    }
                }
                2 => {
                    let s = ["{{", "}}", "{", "}", "{}", "{!}", "{:", "{0", "{a["][usize::from(self.pick(9))];
                    self.push_ascii(&mut out, s);
                }
                _ => self.field_into(&mut out, true),
            }
        }
        PyStr::from_code_points(out)
    }

    fn field_into(&mut self, out: &mut Vec<u32>, allow_nested: bool) {
        self.push_ascii(out, "{");
        match self.pick(6) {
            0 | 1 => {}
            2 => self.digits_into(out),
            3 => {
                let k = ["a", "b", "name", "x", "0", "1", "a_b", "é"][usize::from(self.pick(8))];
                self.push_ascii(out, k);
            }
            4 => {
                // index / attribute chains
                let base = ["0", "a", "", "1"][usize::from(self.pick(4))];
                self.push_ascii(out, base);
                let n = 1 + self.len(2);
                for _ in 0..n {
                    match self.pick(3) {
                        0 => {
                            self.push_ascii(out, "[");
                            let t = self.text(3);
                            for &cp in t.code_points() {
                                if cp != u32::from(']') {
                                    self.push_cp(out, cp);
                                }
                            }
                            self.push_ascii(out, "]");
                        }
                        1 => {
                            self.push_ascii(out, "[");
                            self.digits_into(out);
                            self.push_ascii(out, "]");
                        }
                        _ => self.push_ascii(out, ".real"),
                    }
                }
            }
            _ => {
                let t = self.text(4);
                for &cp in t.code_points() {
                    if cp != u32::from('{') && cp != u32::from('}') {
                        self.push_cp(out, cp);
                    }
                }
            }
        }
        if self.pick(3) == 0 {
            self.push_ascii(out, "!");
            let c = ["r", "s", "a", "x", "", "rr"][usize::from(self.pick(6))];
            self.push_ascii(out, c);
        }
        if allow_nested && self.flag() {
            self.push_ascii(out, ":");
            if self.pick(4) == 0 {
                // One nested field, kept away from digits so the
                // substituted spec can't become a huge width.
                let before = self.spec();
                for &cp in before.code_points() {
                    if !Self::is_digit(cp) && cp != u32::from('{') && cp != u32::from('}') {
                        out.push(cp);
                    }
                }
                self.field_into(out, false);
                if self.flag() {
                    let t = ["s", "d", "f", ">", ""][usize::from(self.pick(5))];
                    out.extend(t.chars().map(u32::from));
                }
            } else {
                let spec = self.spec();
                for &cp in spec.code_points() {
                    if cp != u32::from('{') && cp != u32::from('}') {
                        self.push_cp(out, cp);
                    }
                }
            }
        }
        if self.pick(10) != 0 {
            self.push_ascii(out, "}");
        }
    }

    /// A `%`-format template: literal text and `%[(key)][flags][width][.prec][len]type`.
    pub fn percent_template_cps(&mut self) -> Vec<u32> {
        if self.pick(8) == 0 {
            return self.text(30).into_code_points();
        }
        let mut out = Vec::new();
        let pieces = self.len(6);
        for _ in 0..pieces {
            match self.pick(8) {
                0 | 1 => {
                    let t = self.text(5);
                    for &cp in t.code_points() {
                        self.push_cp(&mut out, cp);
                    }
                }
                2 => self.push_ascii(&mut out, "%%"),
                _ => {
                    self.push_ascii(&mut out, "%");
                    if self.pick(3) == 0 {
                        self.push_ascii(&mut out, "(");
                        let k = ["a", "b", "", "x(y)", "0", "é"][usize::from(self.pick(6))];
                        self.push_ascii(&mut out, k);
                        if self.pick(8) != 0 {
                            self.push_ascii(&mut out, ")");
                        }
                    }
                    let nflags = self.len(3);
                    for _ in 0..nflags {
                        let f = ["-", "+", " ", "#", "0"][usize::from(self.pick(5))];
                        self.push_ascii(&mut out, f);
                    }
                    match self.pick(4) {
                        0 => self.push_ascii(&mut out, "*"),
                        1 => self.digits_into(&mut out),
                        _ => {}
                    }
                    if self.pick(3) == 0 {
                        self.push_ascii(&mut out, ".");
                        match self.pick(3) {
                            0 => self.push_ascii(&mut out, "*"),
                            1 => self.digits_into(&mut out),
                            _ => {}
                        }
                    }
                    if self.pick(6) == 0 {
                        let l = ["l", "h", "L"][usize::from(self.pick(3))];
                        self.push_ascii(&mut out, l);
                    }
                    if self.pick(12) != 0 {
                        let t = ["s", "r", "a", "c", "d", "i", "u", "o", "x", "X", "e", "E", "f", "F", "g", "G", "b", "%", "y", "\u{e9}"]
                            [usize::from(self.pick(20))];
                        self.push_ascii(&mut out, t);
                    }
                }
            }
        }
        out
    }

    /// Bytes from code points: values > 0xFF are folded into a byte.
    pub fn bytes_from_cps(&self, cps: &[u32]) -> Vec<u8> {
        let mut out = Vec::with_capacity(cps.len());
        let mut run = 0usize;
        for &cp in cps {
            let mut b = (cp & 0xFF) as u8;
            if b.is_ascii_digit() {
                if run >= self.max_digit_run {
                    b = b'x';
                    run = 0;
                } else {
                    run += 1;
                }
            } else {
                run = 0;
            }
            out.push(b);
        }
        out
    }

    pub fn raw_bytes(&mut self, max: usize) -> Vec<u8> {
        let t = self.text(max);
        self.bytes_from_cps(t.code_points())
    }

    /// Numeric text: sign, digits, prefixes, underscores, exponent,
    /// whitespace, inf/nan spellings, sometimes Unicode digits.
    pub fn numeric_text(&mut self) -> PyStr {
        if self.pick(5) == 0 {
            return self.text(40);
        }
        let mut out: Vec<u32> = Vec::new();
        let ws = [" ", "\t", "\n", "\u{a0}", "\u{3000}", "\u{2028}", "\u{1c}", ""];
        let lead = ws[usize::from(self.pick(8))];
        self.push_ascii(&mut out, lead);
        if self.flag() {
            let s = ["+", "-", "--", "\u{2212}"][usize::from(self.pick(4))];
            self.push_ascii(&mut out, s);
        }
        match self.pick(8) {
            0 => {
                let w = ["inf", "INF", "Infinity", "nan", "NaN", "infinit", "nann", "iNfInItY"][usize::from(self.pick(8))];
                self.push_ascii(&mut out, w);
            }
            1 => {
                let p = ["0x", "0X", "0o", "0b", "0B", "0_x", "0"][usize::from(self.pick(7))];
                self.push_ascii(&mut out, p);
                let n = self.len(40);
                for _ in 0..n {
                    let c = b"0123456789abcdefABCDEF_xz"[usize::from(self.pick(25))];
                    self.push_cp(&mut out, u32::from(c));
                }
            }
            _ => {
                let n = self.len(60);
                for _ in 0..n {
                    let cp = match self.pick(12) {
                        0 => u32::from('_'),
                        1 => u32::from('.'),
                        2 => u32::from(['e', 'E'][usize::from(self.pick(2))]),
                        3 => SPECIAL[usize::from(self.pick(30))],
                        4 => u32::from(b"abcdefzZ+-"[usize::from(self.pick(10))]),
                        _ => u32::from(b'0' + self.pick(10)),
                    };
                    self.push_cp(&mut out, cp);
                }
            }
        }
        let trail = ws[usize::from(self.pick(8))];
        self.push_ascii(&mut out, trail);
        if self.pick(10) == 0 {
            out.push(0); // embedded NUL
        }
        PyStr::from_code_points(out)
    }

    // ---- values -------------------------------------------------------

    /// A scalar number (bool, int or float).
    pub fn number(&mut self) -> Value {
        match self.pick(6) {
            0 => Value::Bool(self.flag()),
            1..=3 => Value::Int(self.bigint()),
            _ => Value::Float(self.float()),
        }
    }

    /// Any value, including containers (to `depth`) and a foreign type.
    pub fn value(&mut self, depth: u32) -> Value {
        let top = if depth == 0 { 7 } else { 13 };
        match self.pick(top) {
            0 => Value::None,
            1 => Value::Bool(self.flag()),
            2 | 3 => Value::Int(self.bigint()),
            4 => Value::Float(self.float()),
            5 => Value::Str(if self.flag() { self.text(8) } else { self.spec() }),
            6 => {
                let b = self.raw_bytes(8);
                if self.flag() {
                    Value::Bytes(b)
                } else {
                    Value::ByteArray(b)
                }
            }
            7 => Value::Tuple(self.values(depth - 1, 4)),
            8 => Value::List(self.values(depth - 1, 4)),
            9 => Value::Set(self.hashables(depth - 1)),
            10 => Value::FrozenSet(self.hashables(depth - 1)),
            11 => Value::Dict(self.pairs(depth - 1, 4)),
            _ => {
                if self.pick(4) == 0 {
                    Value::Foreign(["Decimal", "object", "Fraction", "complex"][usize::from(self.pick(4))].to_owned())
                } else {
                    Value::Str(self.text(4))
                }
            }
        }
    }

    pub fn values(&mut self, depth: u32, max: usize) -> Vec<Value> {
        let n = self.len(max);
        (0..n).map(|_| self.value(depth)).collect()
    }

    /// Set items: numbers and None mostly, sometimes a str (which the guard rejects).
    fn hashables(&mut self, depth: u32) -> Vec<Value> {
        let n = self.len(4);
        (0..n)
            .map(|_| match self.pick(8) {
                0 => Value::None,
                1 => Value::Str(self.text(3)),
                2 if depth > 0 => Value::Tuple(vec![self.number()]),
                _ => self.number(),
            })
            .collect()
    }

    /// A key used by format/percent templates: common names, ints, random.
    pub fn key(&mut self) -> Value {
        match self.pick(6) {
            0..=2 => Value::str(["a", "b", "name", "x", "0", "", "x(y)", "é", "a_b"][usize::from(self.pick(9))]),
            3 => Value::Int(BigInt::from(self.pick(3))),
            4 => Value::Bool(self.flag()),
            _ => self.value(0),
        }
    }

    pub fn pairs(&mut self, depth: u32, max: usize) -> Vec<(Value, Value)> {
        let n = self.len(max);
        (0..n)
            .map(|_| {
                let k = self.key();
                let v = self.value(depth);
                (k, v)
            })
            .collect()
    }

    /// A value that formatting code will actually format: scalars mostly.
    pub fn format_arg(&mut self) -> Value {
        match self.pick(8) {
            0..=4 => self.number(),
            5 => Value::Str(self.text(6)),
            _ => self.value(2),
        }
    }
}

/// Whether a `str.format` template can't build a huge width or precision
/// out of nested replacement fields: each spec holds at most one nested
/// field, which has no spec of its own and isn't next to a digit.
/// Conservative: it may reject some harmless templates.
#[must_use]
pub fn nested_fields_bounded(cps: &[u32]) -> bool {
    let is_digit = |cp: Option<&u32>| {
        cp.and_then(|&c| char::from_u32(c))
            .and_then(decimal_digit_value)
            .is_some()
    };
    let (lb, rb, colon) = (u32::from('{'), u32::from('}'), u32::from(':'));
    let mut depth = 0usize;
    let mut in_spec = false;
    let mut nested = 0usize;
    let mut i = 0usize;
    while i < cps.len() {
        let c = cps[i];
        if depth == 0 {
            if c == lb {
                if cps.get(i + 1) == Some(&lb) {
                    i += 2;
                    continue;
                }
                depth = 1;
                in_spec = false;
                nested = 0;
            }
        } else if c == lb {
            depth += 1;
            if in_spec || depth > 2 {
                nested += 1;
                if nested > 1 || depth > 2 || (i > 0 && is_digit(cps.get(i - 1))) {
                    return false;
                }
            }
        } else if c == rb {
            depth -= 1;
            if depth >= 1 && is_digit(cps.get(i + 1)) {
                return false;
            }
        } else if c == colon {
            if depth == 1 {
                in_spec = true;
            } else {
                return false; // spec inside a nested field
            }
        }
        i += 1;
    }
    true
}

/// Recursively replaces floats by short dyadic values below 1000, so their
/// `str()` can't spell a huge width or precision when substituted into a
/// nested format spec.
#[must_use]
pub fn tame_floats(v: Value) -> Value {
    let list = |items: Vec<Value>| items.into_iter().map(tame_floats).collect();
    match v {
        Value::Float(x) if x.is_finite() && x != 0.0 => {
            Value::Float(((x % 1000.0) * 4.0).trunc() / 4.0)
        }
        Value::Tuple(items) => Value::Tuple(list(items)),
        Value::List(items) => Value::List(list(items)),
        Value::Set(items) => Value::Set(list(items)),
        Value::FrozenSet(items) => Value::FrozenSet(list(items)),
        Value::Dict(pairs) => Value::Dict(
            pairs
                .into_iter()
                .map(|(k, v)| (tame_floats(k), tame_floats(v)))
                .collect(),
        ),
        other => other,
    }
}
