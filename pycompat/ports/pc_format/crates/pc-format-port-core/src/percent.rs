//! The `%` operator: printf-style formatting of str/bytes/bytearray, else numeric modulo.
//!
//! Ported from CPython 3.12's `PyUnicode_Format` (`Objects/unicodeobject.c`)
//! and `_PyBytes_FormatEx` (`Objects/bytesobject.c`). Both share one parser
//! and one padding routine; [`Mode`] selects the handful of differences.

use std::borrow::Cow;

use num_bigint::BigInt;
use num_traits::{FromPrimitive, ToPrimitive};

use crate::arith::numeric_mod;
use crate::format_spec::{check_size, memory_error, try_vec};
use crate::format_spec::{
    int_magnitude_digits, try_double_to_string, u64_digits, wrap_e_precision, DtoaFlags,
};
use crate::numbers::int_to_float;
use crate::repr::{ascii_repr, value_repr, value_str};
use crate::value::{check_builtins, ExcKind, PcResult, PyError, PyStr, Value};

/// The `-`, `+`, ` `, `#` and `0` conversion flags.
#[derive(Debug, Clone, Copy, Default)]
struct Flags {
    ljust: bool,
    sign: bool,
    blank: bool,
    alt: bool,
    zero: bool,
}

/// `pc_format.percent_format(template, values)`: `template % values`.
///
/// # Errors
/// As the Python built-in it wraps.
pub fn percent_format_api(template: &Value, values: &Value) -> PcResult<Value> {
    check_builtins(&[template, values])?;
    binary_mod(template, values)
}

/// `pc_format.modulo(a, b)`: `a % b`.
///
/// # Errors
/// As the Python built-in it wraps.
pub fn modulo_api(a: &Value, b: &Value) -> PcResult<Value> {
    check_builtins(&[a, b])?;
    binary_mod(a, b)
}

/// Python's binary `%` for any pair of values.
fn binary_mod(a: &Value, b: &Value) -> PcResult<Value> {
    match a {
        Value::Str(s) => format(Mode::Text, s.code_points(), b)
            .and_then(Units::into_wide)
            .map(|out| Value::Str(PyStr::from_code_points(out))),
        Value::Bytes(t) => format_bytes(t, b).map(Value::Bytes),
        Value::ByteArray(t) => format_bytes(t, b).map(Value::ByteArray),
        _ => numeric_mod(a, b).unwrap_or_else(|| {
            Err(PyError::type_error(format!(
                "unsupported operand type(s) for %: '{}' and '{}'",
                a.type_name(),
                b.type_name()
            )))
        }),
    }
}

fn format_bytes(template: &[u8], args: &Value) -> PcResult<Vec<u8>> {
    let mut units: Vec<u32> = try_vec(template.len())?;
    units.extend(template.iter().map(|&b| u32::from(b)));
    format(Mode::Bytes, &units, args)?.into_bytes()
}

/// Code points stored in the narrowest of CPython's str kinds (1, 2 or 4
/// bytes each) that holds them, so huge paddings cost what they cost
/// CPython (the worker runs under a 1 GiB address-space limit).
#[derive(Debug)]
enum Units {
    Narrow(Vec<u8>),
    Ucs2(Vec<u16>),
    Wide(Vec<u32>),
}

/// Runs `$body` with `$v` bound to whichever vector `$units` holds.
macro_rules! each {
    ($units:expr, $v:ident => $body:expr) => {
        match $units {
            Units::Narrow($v) => $body,
            Units::Ucs2($v) => $body,
            Units::Wide($v) => $body,
        }
    };
}

/// `src` converted unit by unit to a fresh vector, or `MemoryError`.
fn convert_units<S: Copy, D: TryFrom<S> + Default>(src: &[S]) -> PcResult<Vec<D>> {
    let mut out = try_vec(src.len())?;
    out.extend(src.iter().map(|&u| D::try_from(u).unwrap_or_default()));
    Ok(out)
}

/// Appends `part` to `dst`; callers widen `dst` first, so nothing truncates.
fn append<S: Copy, D: TryFrom<S> + Default>(dst: &mut Vec<D>, part: &[S]) -> PcResult<()> {
    dst.try_reserve(part.len()).map_err(|_| memory_error())?;
    dst.extend(part.iter().map(|&u| D::try_from(u).unwrap_or_default()));
    Ok(())
}

impl Units {
    fn len(&self) -> usize {
        each!(self, v => v.len())
    }

    fn maxchar(&self) -> u32 {
        match self {
            Units::Narrow(v) => v.iter().copied().max().map_or(0, u32::from),
            Units::Ucs2(v) => v.iter().copied().max().map_or(0, u32::from),
            Units::Wide(v) => v.iter().copied().max().unwrap_or(0),
        }
    }

    fn get(&self, i: usize) -> Option<u32> {
        match self {
            Units::Narrow(v) => v.get(i).copied().map(u32::from),
            Units::Ucs2(v) => v.get(i).copied().map(u32::from),
            Units::Wide(v) => v.get(i).copied(),
        }
    }

    fn truncate(&mut self, n: usize) {
        each!(self, v => v.truncate(n));
    }

    /// Grows the buffer by `extra` units, or fails with `MemoryError`.
    fn reserve(&mut self, extra: usize) -> PcResult<()> {
        each!(self, v => v.try_reserve(extra)).map_err(|_| memory_error())
    }

    /// Switches to a wider kind if `maxchar` doesn't fit the current one.
    fn widen_for(&mut self, maxchar: u32) -> PcResult<()> {
        let widened = match self {
            Units::Narrow(v) if maxchar > 0xFFFF => Units::Wide(convert_units(v)?),
            Units::Narrow(v) if maxchar > 0xFF => Units::Ucs2(convert_units(v)?),
            Units::Ucs2(v) if maxchar > 0xFFFF => Units::Wide(convert_units(v)?),
            _ => return Ok(()),
        };
        *self = widened;
        Ok(())
    }

    fn push(&mut self, u: u32) -> PcResult<()> {
        self.push_fill(u, 1)
    }

    fn push_fill(&mut self, u: u32, n: usize) -> PcResult<()> {
        self.widen_for(u)?;
        self.reserve(n)?;
        match self {
            Units::Narrow(v) => v.extend(std::iter::repeat_n(u8::try_from(u).unwrap_or(0), n)),
            Units::Ucs2(v) => v.extend(std::iter::repeat_n(u16::try_from(u).unwrap_or(0), n)),
            Units::Wide(v) => v.extend(std::iter::repeat_n(u, n)),
        }
        Ok(())
    }

    /// Appends `src[start..end]` (clamped to `src`).
    fn extend_from(&mut self, src: &Units, start: usize, end: usize) -> PcResult<()> {
        let end = end.min(src.len());
        let start = start.min(end);
        let part_max = match src {
            Units::Narrow(s) => s
                .get(start..end)
                .unwrap_or(&[])
                .iter()
                .copied()
                .max()
                .map_or(0, u32::from),
            Units::Ucs2(s) => s
                .get(start..end)
                .unwrap_or(&[])
                .iter()
                .copied()
                .max()
                .map_or(0, u32::from),
            Units::Wide(s) => s
                .get(start..end)
                .unwrap_or(&[])
                .iter()
                .copied()
                .max()
                .unwrap_or(0),
        };
        self.widen_for(part_max)?;
        each!(self, d => each!(src, s => append(d, s.get(start..end).unwrap_or(&[]))))
    }

    /// Appends code points, widening once for the widest of them.
    fn extend_code_points(&mut self, cps: &[u32]) -> PcResult<()> {
        self.widen_for(cps.iter().copied().max().unwrap_or(0))?;
        each!(self, d => append(d, cps))
    }

    fn into_wide(self) -> PcResult<Vec<u32>> {
        match self {
            Units::Wide(v) => Ok(v),
            Units::Ucs2(v) => convert_units(&v),
            Units::Narrow(v) => convert_units(&v),
        }
    }

    /// The bytes formatter only ever stores bytes and ASCII text.
    fn into_bytes(self) -> PcResult<Vec<u8>> {
        match self {
            Units::Narrow(v) => Ok(v),
            Units::Ucs2(v) => convert_units(&v),
            Units::Wide(v) => convert_units(&v),
        }
    }
}

/// [`units_of`] for borrowed code points.
fn units_from(cps: &[u32]) -> PcResult<Units> {
    let maxchar = cps.iter().copied().max().unwrap_or(0);
    Ok(if maxchar > 0xFFFF {
        Units::Wide(convert_units(cps)?)
    } else if maxchar > 0xFF {
        Units::Ucs2(convert_units(cps)?)
    } else {
        Units::Narrow(convert_units(cps)?)
    })
}

/// `cps` stored in the narrowest kind that holds them.
fn units_of(cps: Vec<u32>) -> PcResult<Units> {
    let mut units = Units::Narrow(Vec::new());
    units.widen_for(cps.iter().copied().max().unwrap_or(0))?;
    if let Units::Wide(_) = units {
        return Ok(Units::Wide(cps));
    }
    each!(&mut units, d => append(d, &cps))?;
    Ok(units)
}

/// Bytes `_Py_dg_dtoa`'s `rv_alloc(i)` takes: a `Bigint` whose digit
/// buffer `j` is the first power of two (times 4) with `24 + j > i`.
fn dtoa_alloc(i: usize) -> usize {
    let mut j: usize = 4;
    while j.saturating_add(24) <= i {
        j = j.saturating_mul(2);
    }
    j.saturating_add(28)
}

/// Precisions below this keep [`float_memory`] far under the budget.
const SMALL_PRECISION: usize = 1 << 20;

/// CPython's peak allocation (bytes) for `PyOS_double_to_string(x, ch,
/// prec)`: the dtoa digits are alive while the output buffer is built,
/// then the buffer and its str copy are alive together.
fn float_memory(x: f64, ch: char, prec: usize, alt: bool) -> usize {
    if !x.is_finite() {
        return 0;
    }
    // Every estimate below is a few MiB at most for these precisions (a
    // finite f64 has at most 309 integer digits): skip the `log10`.
    if prec < SMALL_PRECISION {
        return 0;
    }
    // Digits before the point; at most 309 for a finite f64.
    let int_digits = if x.abs() >= 1.0 {
        usize::try_from(x.abs().log10().floor() as i64 + 1).unwrap_or(0)
    } else {
        0
    };
    let (ndigits, len) = match ch {
        'e' | 'E' => (prec.saturating_add(1), prec.saturating_add(8)),
        'f' | 'F' => (
            prec.saturating_add(int_digits).saturating_add(1),
            prec.saturating_add(int_digits).saturating_add(3),
        ),
        _ => (prec, if alt { prec.saturating_add(8) } else { 32 }),
    };
    // `_Py_dg_dtoa` returns "0" for zero before allocating `ndigits` bytes.
    let ndigits = if x == 0.0 { 1 } else { ndigits };
    dtoa_alloc(ndigits)
        .saturating_add(len)
        .max(len.saturating_mul(2))
}

/// Which `%` implementation runs: `str % x` or `bytes % x`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Text,
    Bytes,
}

impl Mode {
    fn what(self) -> &'static str {
        match self {
            Mode::Text => "string",
            Mode::Bytes => "bytes",
        }
    }

    fn not_all_converted(self) -> PyError {
        PyError::type_error(format!(
            "not all arguments converted during {} formatting",
            self.what()
        ))
    }
}

/// The argument list: a tuple's items, or one value (CPython's
/// `arglen == -1` / `argidx == -2` state).
enum Args<'a> {
    Tuple { items: &'a [Value], idx: usize },
    Single { value: &'a Value, used: bool },
}

impl<'a> Args<'a> {
    fn new(v: &'a Value) -> Self {
        match v {
            Value::Tuple(items) => Args::Tuple { items, idx: 0 },
            other => Args::Single {
                value: other,
                used: false,
            },
        }
    }

    fn next(&mut self) -> PcResult<&'a Value> {
        let got = match self {
            Args::Tuple { items, idx } => {
                let v = items.get(*idx);
                if v.is_some() {
                    *idx += 1;
                }
                v
            }
            Args::Single { value, used } => {
                if *used {
                    None
                } else {
                    *used = true;
                    Some(*value)
                }
            }
        };
        got.ok_or_else(|| PyError::type_error("not enough arguments for format string"))
    }

    fn has_unused(&self) -> bool {
        match self {
            Args::Tuple { items, idx } => *idx < items.len(),
            Args::Single { used, .. } => !*used,
        }
    }
}

/// Reads the template like CPython's `fmtpos`/`fmtcnt` pair: once a read
/// past the end is attempted, `exhausted` stays set (`fmtcnt < 0`).
struct Cursor<'t> {
    units: &'t [u32],
    pos: usize,
    exhausted: bool,
}

impl Cursor<'_> {
    fn next(&mut self) -> Option<u32> {
        if let Some(&u) = self.units.get(self.pos) {
            self.pos += 1;
            Some(u)
        } else {
            self.exhausted = true;
            None
        }
    }

    /// The unit at the current position (the NUL terminator at the end).
    fn peek(&self) -> u32 {
        self.units.get(self.pos).copied().unwrap_or(0)
    }
}

/// One parsed conversion specifier.
struct Spec {
    flags: Flags,
    width: Option<usize>,
    prec: Option<usize>,
    ch: u32,
    /// Bytes mode: `ch` was read through a signed `char` (bytes >= 0x80
    /// become negative in C, which changes the unsupported-character error).
    signed_read: bool,
}

fn is_ascii_digit(u: u32) -> bool {
    (u32::from(b'0')..=u32::from(b'9')).contains(&u)
}

fn mapping_of(mode: Mode, args: &Value) -> Option<&Value> {
    let is_mapping = match args {
        Value::Dict(_) | Value::List(_) => true,
        Value::Bytes(_) | Value::ByteArray(_) => mode == Mode::Text,
        _ => false,
    };
    is_mapping.then_some(args)
}

/// `PyObject_GetItem(mapping, key)` for a str (text mode) or bytes key.
fn lookup<'a>(mode: Mode, mapping: &'a Value, key: &[u32]) -> PcResult<&'a Value> {
    // Hit in a dict with a str key: compare in place, without building the
    // key value. Misses and other cases take the general path below.
    if let (Mode::Text, Value::Dict(pairs)) = (mode, mapping) {
        let found = pairs.iter().find_map(|(k, v)| match k {
            Value::Str(s) if s.code_points() == key => Some(v),
            _ => None,
        });
        if let Some(v) = found {
            return Ok(v);
        }
    }
    let key_value = match mode {
        Mode::Text => Value::Str(PyStr::from_code_points(key.to_vec())),
        Mode::Bytes => Value::Bytes(
            key.iter()
                .map(|&u| u8::try_from(u).unwrap_or(b'?'))
                .collect(),
        ),
    };
    let key_type = key_value.type_name().to_owned();
    let indices = |container: &str| {
        PyError::type_error(format!(
            "{container} indices must be integers or slices, not {key_type}"
        ))
    };
    match mapping {
        Value::Dict(pairs) => pairs
            .iter()
            .find(|(k, _)| *k == key_value)
            .map(|(_, v)| v)
            .ok_or_else(|| PyError::key_error(key_value.clone())),
        Value::List(_) => Err(indices("list")),
        Value::Bytes(_) => Err(indices("byte")),
        Value::ByteArray(_) => Err(indices("bytearray")),
        other => Err(PyError::type_error(format!(
            "'{}' object is not subscriptable",
            other.type_name()
        ))),
    }
}

fn star_width(v: &Value) -> PcResult<(usize, bool)> {
    let n = star_int(v)?;
    let w = n
        .to_i64()
        .ok_or_else(|| PyError::overflow_error("Python int too large to convert to C ssize_t"))?;
    let mag = usize::try_from(w.unsigned_abs()).unwrap_or(usize::MAX);
    Ok((mag, w < 0))
}

fn star_prec(v: &Value) -> PcResult<usize> {
    let n = star_int(v)?;
    let p = n
        .to_i32()
        .ok_or_else(|| PyError::overflow_error("Python int too large to convert to C int"))?;
    Ok(usize::try_from(p).unwrap_or(0))
}

fn star_int(v: &Value) -> PcResult<BigInt> {
    match v {
        Value::Int(n) => Ok(n.clone()),
        Value::Bool(b) => Ok(BigInt::from(u8::from(*b))),
        _ => Err(PyError::type_error("* wants int")),
    }
}

/// Parses digits after `first`, like CPython's width/precision loops
/// (these reads go through `Py_CHARMASK`, so they're unsigned).
fn parse_number(
    cur: &mut Cursor<'_>,
    first: u32,
    limit: usize,
    msg: &str,
) -> PcResult<(usize, u32)> {
    let mut value = usize::try_from(first - u32::from(b'0')).unwrap_or(0);
    let mut ch = first;
    while let Some(c) = cur.next() {
        ch = c;
        if !is_ascii_digit(c) {
            break;
        }
        let d = usize::try_from(c - u32::from(b'0')).unwrap_or(0);
        if value > (limit - d) / 10 {
            return Err(PyError::value_error(msg));
        }
        value = value * 10 + d;
    }
    Ok((value, ch))
}

/// Port of `unicode_format_arg_parse` / the bytes equivalent. Starts just
/// after the `%`, with `%%` already handled.
fn parse_spec<'a>(
    mode: Mode,
    cur: &mut Cursor<'_>,
    args: &mut Args<'a>,
    dict: Option<&'a Value>,
) -> PcResult<Spec> {
    let mut spec = Spec {
        flags: Flags::default(),
        width: None,
        prec: None,
        ch: 0,
        signed_read: true,
    };
    if cur.peek() == u32::from(b'(') {
        let Some(mapping) = dict else {
            return Err(PyError::type_error("format requires a mapping"));
        };
        cur.pos += 1;
        let keystart = cur.pos;
        let mut pcount = 1usize;
        while pcount > 0 {
            let Some(c) = cur.next() else { break };
            if c == u32::from(b')') {
                pcount -= 1;
            } else if c == u32::from(b'(') {
                pcount += 1;
            }
        }
        if cur.exhausted || pcount > 0 {
            return Err(PyError::value_error("incomplete format key"));
        }
        let key = cur.units.get(keystart..cur.pos - 1).unwrap_or(&[]);
        *args = Args::Single {
            value: lookup(mode, mapping, key)?,
            used: false,
        };
    }

    // Flags.
    while let Some(c) = cur.next() {
        spec.ch = c;
        let flag = match char::from_u32(c) {
            Some('-') => &mut spec.flags.ljust,
            Some('+') => &mut spec.flags.sign,
            Some(' ') => &mut spec.flags.blank,
            Some('#') => &mut spec.flags.alt,
            Some('0') => &mut spec.flags.zero,
            _ => break,
        };
        *flag = true;
    }

    // Width.
    if spec.ch == u32::from(b'*') {
        let (w, negative) = star_width(args.next()?)?;
        if negative {
            spec.flags.ljust = true;
        }
        spec.width = Some(w);
        if let Some(c) = cur.next() {
            spec.ch = c;
        }
    } else if is_ascii_digit(spec.ch) {
        let limit = usize::try_from(i64::MAX).unwrap_or(usize::MAX);
        let (w, ch) = parse_number(cur, spec.ch, limit, "width too big")?;
        spec.width = Some(w);
        spec.ch = ch;
        spec.signed_read = false;
    }

    // Precision.
    if spec.ch == u32::from(b'.') {
        spec.prec = Some(0);
        spec.signed_read = true;
        if let Some(c) = cur.next() {
            spec.ch = c;
        }
        if spec.ch == u32::from(b'*') {
            spec.prec = Some(star_prec(args.next()?)?);
            if let Some(c) = cur.next() {
                spec.ch = c;
            }
        } else if is_ascii_digit(spec.ch) {
            let limit = usize::try_from(i32::MAX).unwrap_or(usize::MAX);
            // `_PyBytes_FormatEx` words this differently from the str formatter.
            let msg = match mode {
                Mode::Bytes => "prec too big",
                Mode::Text => "precision too big",
            };
            let (p, ch) = parse_number(cur, spec.ch, limit, msg)?;
            spec.prec = Some(p);
            spec.ch = ch;
            spec.signed_read = false;
        }
    }

    // Length modifiers h, l, L are ignored.
    if !cur.exhausted && matches!(char::from_u32(spec.ch), Some('h' | 'l' | 'L')) {
        if let Some(c) = cur.next() {
            spec.ch = c;
            spec.signed_read = true;
        }
    }
    if cur.exhausted {
        return Err(PyError::value_error("incomplete format"));
    }
    Ok(spec)
}

/// A converted argument, before padding.
struct Converted {
    units: Units,
    /// Numeric conversions take a sign and zero padding.
    numeric: bool,
}

fn text_units(s: String) -> PcResult<Units> {
    if s.is_ascii() {
        Ok(Units::Narrow(s.into_bytes()))
    } else {
        units_of(s.chars().map(u32::from).collect())
    }
}

/// `int(v)` for `%d %i %u`, `operator.index(v)` for `%o %x %X`.
fn integer_arg(v: &Value, ch: char) -> PcResult<Cow<'_, BigInt>> {
    let wrong = || {
        let what = if matches!(ch, 'o' | 'x' | 'X') {
            "an integer"
        } else {
            "a real number"
        };
        PyError::type_error(format!(
            "%{ch} format: {what} is required, not {}",
            v.type_name()
        ))
    };
    match v {
        Value::Int(n) => Ok(Cow::Borrowed(n)),
        Value::Bool(b) => Ok(Cow::Owned(BigInt::from(u8::from(*b)))),
        Value::Float(_) if matches!(ch, 'o' | 'x' | 'X') => Err(wrong()),
        Value::Float(f) => {
            if f.is_nan() {
                Err(PyError::value_error("cannot convert float NaN to integer"))
            } else if f.is_infinite() {
                Err(PyError::overflow_error(
                    "cannot convert float infinity to integer",
                ))
            } else {
                BigInt::from_f64(f.trunc())
                    .map(Cow::Owned)
                    .ok_or_else(wrong)
            }
        }
        _ => Err(wrong()),
    }
}

/// Port of `_PyUnicode_FormatLong`: sign, `0o`/`0x` when alternate,
/// zero-extended to `prec` digits, uppercased for `X`.
fn format_long(n: &BigInt, ch: char, spec: &Spec) -> PcResult<Units> {
    let (base, prefix) = match ch {
        'o' => (8, "0o"),
        'x' | 'X' => (16, "0x"),
        _ => (10, ""),
    };
    let prec = spec.prec.unwrap_or(0);
    // Checked before the digits are computed, so it wins over the
    // str-digit limit.
    if prec > i32::MAX as usize - 3 {
        return Err(PyError::overflow_error("precision too large"));
    }
    let mut small = [0_u8; 64];
    let owned;
    let digits: &[u8] = match n.magnitude().to_u64() {
        Some(m) => u64_digits(m, base, &mut small),
        None => {
            owned = int_magnitude_digits(n, base)?;
            owned.as_bytes()
        }
    };
    let len = digits.len().max(prec).saturating_add(3);
    // The zero-filled copy is made as bytes, then decoded to a str.
    if prec > digits.len() {
        check_size(len.saturating_mul(2), 0)?;
    }
    // Only ASCII is written, so the bytes are the narrow str kind.
    let mut out = Vec::new();
    out.try_reserve_exact(len).map_err(|_| memory_error())?;
    if n.sign() == num_bigint::Sign::Minus {
        out.push(b'-');
    }
    if spec.flags.alt {
        out.extend_from_slice(prefix.as_bytes());
    }
    out.extend(std::iter::repeat_n(b'0', prec.saturating_sub(digits.len())));
    out.extend_from_slice(digits);
    if ch == 'X' {
        out.make_ascii_uppercase();
    }
    Ok(Units::Narrow(out))
}

/// `PyFloat_AsDouble(v)`, with the bytes formatter's error rewording.
fn float_arg(mode: Mode, v: &Value) -> PcResult<f64> {
    let result = match v {
        Value::Float(f) => Ok(*f),
        Value::Bool(b) => Ok(f64::from(u8::from(*b))),
        Value::Int(n) => int_to_float(n),
        other => Err(PyError::type_error(format!(
            "must be real number, not {}",
            other.type_name()
        ))),
    };
    match mode {
        Mode::Text => result,
        Mode::Bytes => result.map_err(|_| {
            PyError::type_error(format!("float argument required, not {}", v.type_name()))
        }),
    }
}

fn char_arg(mode: Mode, v: &Value) -> PcResult<u32> {
    let (limit, range_msg, type_msg) = match mode {
        Mode::Text => (
            0x10_FFFF,
            "%c arg not in range(0x110000)",
            "%c requires int or char",
        ),
        Mode::Bytes => (
            0xFF,
            "%c arg not in range(256)",
            "%c requires an integer in range(256) or a single byte",
        ),
    };
    let n = match (mode, v) {
        (Mode::Text, Value::Str(s)) => {
            return match s.code_points() {
                [c] => Ok(*c),
                _ => Err(PyError::type_error(type_msg)),
            };
        }
        (Mode::Bytes, Value::Bytes(b) | Value::ByteArray(b)) => {
            return match b.as_slice() {
                [c] => Ok(u32::from(*c)),
                _ => Err(PyError::type_error(type_msg)),
            };
        }
        (_, Value::Int(n)) => n.clone(),
        (_, Value::Bool(b)) => BigInt::from(u8::from(*b)),
        _ => return Err(PyError::type_error(type_msg)),
    };
    n.to_u32()
        .filter(|&c| c <= limit)
        .ok_or_else(|| PyError::overflow_error(range_msg))
}

fn bytes_like_required(v: &Value) -> PyError {
    let name = v.type_name();
    PyError::type_error(format!(
        "%b requires a bytes-like object, or an object that implements __bytes__, not '{name}'"
    ))
}

fn unsupported(mode: Mode, spec: &Spec, index: usize) -> PyError {
    let ch = spec.ch;
    let shown = match mode {
        Mode::Text if (31..=126).contains(&ch) => ch,
        Mode::Text => u32::from(b'?'),
        Mode::Bytes if spec.signed_read && ch >= 0x80 => {
            return PyError::overflow_error("character argument not in range(0x110000)");
        }
        Mode::Bytes => ch,
    };
    let mut msg = PyStr::from("unsupported format character '");
    msg.push_code_point(shown);
    msg.push_str(&format!("' ({ch:#x}) at index {index}"));
    PyError::new(ExcKind::ValueError, msg)
}

/// Port of `unicode_format_arg_format` and the bytes `switch (c)`.
fn convert(mode: Mode, spec: &Spec, v: &Value, index: usize) -> PcResult<Converted> {
    let ch = char::from_u32(spec.ch).unwrap_or('\0');
    let truncate = |mut units: Units| {
        if let Some(p) = spec.prec {
            units.truncate(p);
        }
        units
    };
    let units = match (mode, ch) {
        (Mode::Text, 's') => truncate(match v {
            // Copied once, straight into the narrowest kind.
            Value::Str(s) => units_from(s.code_points())?,
            other => units_of(value_str(other)?.into_code_points())?,
        }),
        (Mode::Text, 'r') => truncate(text_units(value_repr(v)?)?),
        (Mode::Text, 'a') | (Mode::Bytes, 'r' | 'a') => truncate(text_units(ascii_repr(v)?)?),
        (Mode::Bytes, 's' | 'b') => match v {
            Value::Bytes(b) | Value::ByteArray(b) => truncate(Units::Narrow(b.clone())),
            other => return Err(bytes_like_required(other)),
        },
        (_, 'd' | 'i' | 'u' | 'o' | 'x' | 'X') => {
            // `_PyBytes_FormatEx` rewrites `%i` to `%d` before converting.
            let ch = if mode == Mode::Bytes && ch == 'i' {
                'd'
            } else {
                ch
            };
            let n = integer_arg(v, ch)?;
            return Ok(Converted {
                units: format_long(&n, ch, spec)?,
                numeric: true,
            });
        }
        (_, 'e' | 'E' | 'f' | 'F' | 'g' | 'G') => {
            let x = float_arg(mode, v)?;
            let prec = wrap_e_precision(ch, spec.prec.unwrap_or(6));
            let alt = spec.flags.alt;
            check_size(float_memory(x, ch, prec, alt), 0)?;
            let flags = DtoaFlags {
                alt,
                ..DtoaFlags::default()
            };
            return Ok(Converted {
                units: text_units(try_double_to_string(x, ch, prec, flags)?)?,
                numeric: true,
            });
        }
        (_, 'c') => units_of(vec![char_arg(mode, v)?])?,
        _ => return Err(unsupported(mode, spec, index)),
    };
    Ok(Converted {
        units,
        numeric: false,
    })
}

/// Fails like CPython's `_PyUnicodeWriter_Prepare(writer, width, maxchar)`:
/// the writer grows to its current length plus `width`, over-allocated by a
/// quarter unless this is the template's last directive.
fn check_pad_memory(
    out: &Units,
    width: usize,
    fill: u32,
    conv: &Converted,
    last: bool,
) -> PcResult<()> {
    let mut total = out.len().saturating_add(width);
    if !last {
        total = total.saturating_add(total / 4);
    }
    // Only scan for the widest code point when the size could matter.
    if total.saturating_mul(4) < (1 << 28) {
        return Ok(());
    }
    let maxchar = out.maxchar().max(conv.units.maxchar()).max(fill);
    check_size(total, maxchar)
}

/// Port of `unicode_format_arg_output` (the bytes formatter pads the same way).
fn pad(out: &mut Units, spec: &Spec, conv: &Converted, last: bool) -> PcResult<()> {
    let buf = &conv.units;
    let mut sign = conv.numeric;
    let fill = if sign && spec.flags.zero {
        u32::from(b'0')
    } else {
        u32::from(b' ')
    };
    let space = u32::from(b' ');
    let mut len = buf.len();
    let mut pindex = 0usize;
    let mut signchar = 0u32;
    if sign {
        match buf.get(0) {
            Some(c) if c == u32::from(b'-') || c == u32::from(b'+') => {
                signchar = c;
                len -= 1;
                pindex += 1;
            }
            _ if spec.flags.sign => signchar = u32::from(b'+'),
            _ if spec.flags.blank => signchar = space,
            _ => sign = false,
        }
    }
    let mut width = spec.width.unwrap_or(0).max(len);
    if width > len {
        check_pad_memory(out, width, fill, conv, last)?;
        // Like `_PyUnicodeWriter_Prepare`: widen to the final kind before
        // the padding is written, so it is never copied to a wider kind.
        out.widen_for(conv.units.maxchar().max(fill))?;
        out.reserve(width)?;
    }
    let alt_prefix = spec.flags.alt && matches!(char::from_u32(spec.ch), Some('o' | 'x' | 'X'));

    if sign {
        if fill != space {
            out.push(signchar)?;
        }
        if width > len {
            width -= 1;
        }
    }
    if alt_prefix {
        if fill != space {
            out.extend_from(buf, pindex, pindex + 2)?;
            pindex += 2;
        }
        width = width.saturating_sub(2);
        len = len.saturating_sub(2);
    }
    if width > len && !spec.flags.ljust {
        out.push_fill(fill, width - len)?;
        width = len;
    }
    if fill == space {
        if sign {
            out.push(signchar)?;
        }
        if alt_prefix {
            out.extend_from(buf, pindex, pindex + 2)?;
            pindex += 2;
        }
    }
    out.extend_from(buf, pindex, pindex + len)?;
    if width > len {
        out.push_fill(space, width - len)?;
    }
    Ok(())
}

/// The shared formatting loop (`PyUnicode_Format` / `_PyBytes_FormatEx`).
fn format(mode: Mode, template: &[u32], values: &Value) -> PcResult<Units> {
    let percent = u32::from(b'%');
    let dict = mapping_of(mode, values);
    let mut args = Args::new(values);
    let mut out = Units::Narrow(try_vec(template.len())?);
    let mut cur = Cursor {
        units: template,
        pos: 0,
        exhausted: false,
    };
    while let Some(u) = cur.next() {
        if u != percent {
            // Copy the whole literal run up to the next '%' at once.
            let start = cur.pos - 1;
            let rest = cur.units.get(cur.pos..).unwrap_or_default();
            let end = rest
                .iter()
                .position(|&c| c == percent)
                .map_or(cur.units.len(), |i| cur.pos + i);
            out.extend_code_points(cur.units.get(start..end).unwrap_or_default())?;
            cur.pos = end;
            continue;
        }
        if cur.peek() == percent {
            cur.pos += 1;
            out.push(percent)?;
            continue;
        }
        let spec = parse_spec(mode, &mut cur, &mut args, dict)?;
        let v = args.next()?;
        let conv = convert(mode, &spec, v, cur.pos - 1)?;
        let last = cur.pos >= cur.units.len();
        pad(&mut out, &spec, &conv, last)?;
        if dict.is_some() && args.has_unused() {
            return Err(mode.not_all_converted());
        }
    }
    if dict.is_none() && args.has_unused() {
        return Err(mode.not_all_converted());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(t: &str, v: Value) -> PcResult<Value> {
        percent_format_api(&Value::str(t), &v)
    }

    fn ok(t: &str, v: Value) -> String {
        match fmt(t, v) {
            Ok(Value::Str(s)) => s.to_string_lossy(),
            other => format!("{other:?}"),
        }
    }

    #[test]
    fn basics() {
        assert_eq!(ok("%#06o", Value::int(8)), "0o0010");
        assert_eq!(ok("%#06x", Value::int(-8)), "-0x008");
        assert_eq!(ok("%-06d", Value::int(-5)), "-5    ");
        assert_eq!(ok("%#5.3X", Value::int(-5)), "-0X005");
        assert_eq!(ok("%.2s", Value::str("abc")), "ab");
        assert_eq!(ok("a", Value::List(vec![])), "a");
        assert_eq!(
            ok("%(a)s", Value::Dict(vec![(Value::str("a"), Value::int(1))])),
            "1"
        );
    }

    #[test]
    fn errors() {
        let e = fmt("%", Value::Tuple(vec![])).err();
        assert_eq!(e, Some(PyError::value_error("incomplete format")));
        let e = fmt("%z", Value::int(1)).err();
        assert_eq!(
            e,
            Some(PyError::value_error(
                "unsupported format character 'z' (0x7a) at index 1"
            ))
        );
        let e = fmt("%5%", Value::Tuple(vec![])).err();
        assert_eq!(
            e,
            Some(PyError::type_error(
                "not enough arguments for format string"
            ))
        );
    }

    fn bfmt(t: &[u8], v: Value) -> PcResult<Value> {
        percent_format_api(&Value::Bytes(t.to_vec()), &v)
    }

    #[test]
    fn bytes_formatting() {
        assert_eq!(
            bfmt(
                b"%06.2f|%#6o|%5c",
                Value::Tuple(vec![Value::Float(-1.5), Value::int(8), Value::int(65)])
            ),
            Ok(Value::Bytes(b"-01.50|  0o10|    A".to_vec()))
        );
        assert_eq!(
            bfmt(b"%.2a", Value::str("abc")),
            Ok(Value::Bytes(b"'a".to_vec()))
        );
        assert_eq!(
            bfmt(b"a", Value::Bytes(b"x".to_vec())),
            Err(PyError::type_error(
                "not all arguments converted during bytes formatting"
            ))
        );
        assert_eq!(
            bfmt(b"%f", Value::int(BigInt::from(10).pow(400))),
            Err(PyError::type_error("float argument required, not int"))
        );
        assert_eq!(
            bfmt(b"%\xe9", Value::int(1)),
            Err(PyError::overflow_error(
                "character argument not in range(0x110000)"
            ))
        );
        let mut msg = PyStr::from("unsupported format character '");
        msg.push_code_point(0xe9);
        msg.push_str("' (0xe9) at index 2");
        assert_eq!(
            bfmt(b"%5\xe9", Value::int(1)),
            Err(PyError::new(ExcKind::ValueError, msg))
        );
        assert_eq!(
            bfmt(
                b"%(a)s",
                Value::Dict(vec![(Value::str("a"), Value::Bytes(b"x".to_vec()))])
            ),
            Err(PyError::key_error(Value::Bytes(b"a".to_vec())))
        );
        assert_eq!(
            percent_format_api(&Value::ByteArray(b"%d".to_vec()), &Value::int(5)),
            Ok(Value::ByteArray(b"5".to_vec()))
        );
    }

    #[test]
    fn huge_precision_and_width() {
        let one = || Value::int(1);
        assert_eq!(
            fmt("%.2147483647d", Value::int(BigInt::from(10).pow(5000))).err(),
            Some(PyError::overflow_error("precision too large"))
        );
        assert_eq!(
            fmt("%.2147483644x", one()).err(),
            Some(PyError::new(ExcKind::MemoryError, ""))
        );
        assert_eq!(ok("%.2147483647e", Value::Float(1.5)), "2e+00");
        assert_eq!(ok("%#.2147483647E", Value::Float(-1234.5)), "-1.E+03");
        assert_eq!(ok("%.536870000g", Value::Float(1.5)), "1.5");
        for t in [
            "%.2147483646e",
            "%.600000000f",
            "%.537000000g",
            "%2147483647d",
        ] {
            assert_eq!(
                fmt(t, Value::Float(1.0)).err(),
                Some(PyError::new(ExcKind::MemoryError, "")),
                "{t}"
            );
        }
        assert_eq!(
            bfmt(b"%9223372036854775807s", Value::Bytes(vec![])).err(),
            Some(PyError::new(ExcKind::MemoryError, ""))
        );
    }

    #[test]
    fn mixed_kinds_widen() {
        let t = Value::Tuple(vec![
            Value::str("\u{e9}"),
            Value::str("\u{1234}"),
            Value::str("\u{1f600}"),
            Value::int(255),
        ]);
        assert_eq!(
            ok("a%-3s|%3s|%2s|%#06x", t),
            "a\u{e9}  |  \u{1234}| \u{1f600}|0x00ff"
        );
        assert_eq!(
            bfmt(
                b"%*r",
                Value::Tuple(vec![Value::int(3), Value::Bytes(vec![])])
            ),
            Ok(Value::Bytes(b"b''".to_vec()))
        );
    }

    #[test]
    fn numeric_and_unsupported_operands() {
        assert_eq!(
            modulo_api(&Value::int(7), &Value::int(-2)),
            Ok(Value::int(-1))
        );
        assert_eq!(
            modulo_api(&Value::int(1), &Value::str("a")),
            Err(PyError::type_error(
                "unsupported operand type(s) for %: 'int' and 'str'"
            ))
        );
    }
}
