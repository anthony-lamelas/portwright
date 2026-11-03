//! Number layout (`calc_number_widths` / `fill_number`), int and float
//! formatting, and the `PyOS_double_to_string` port.

use num_bigint::{BigInt, Sign};
use num_traits::ToPrimitive;

use super::parse::{FormatSpec, Thousands};
use crate::numbers::{int_str, int_to_float};
use crate::value::{ExcKind, PcResult, PyError};

/// The allocation budget (bytes) that huge outputs are measured against.
///
/// This is deliberate policy: it models CPython 3.12 running under a 1 GiB
/// address-space limit (less what the interpreter itself has mapped), so an
/// output CPython could not allocate there raises `MemoryError` here too,
/// even when this machine could build it.
const CPYTHON_BUDGET: usize = (1 << 30) - (64 << 20);

/// The `MemoryError` CPython raises (with no arguments).
pub(crate) fn memory_error() -> PyError {
    PyError::new(ExcKind::MemoryError, "")
}

/// Bytes per code point of a CPython str whose largest code point is `maxchar`.
fn kind_bytes(maxchar: u32) -> usize {
    match maxchar {
        0..=0xFF => 1,
        0x100..=0xFFFF => 2,
        _ => 4,
    }
}

/// Fails like CPython's allocator when it would need `bytes` at once.
fn check_bytes(bytes: usize) -> PcResult<()> {
    if bytes < CPYTHON_BUDGET {
        Ok(())
    } else {
        Err(memory_error())
    }
}

/// Fails like CPython's allocator when its output str would need `total`
/// code points no wider than `maxchar`.
pub(crate) fn check_size(total: usize, maxchar: u32) -> PcResult<()> {
    check_bytes(total.saturating_mul(kind_bytes(maxchar)))
}

/// A vector with room for `n` items, or `MemoryError`.
pub(crate) fn try_vec<T>(n: usize) -> PcResult<Vec<T>> {
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| memory_error())?;
    Ok(v)
}

/// Digit grouping: group size (0 = none) and separator.
#[derive(Debug, Clone, Copy)]
struct Grouping {
    size: usize,
    sep: u32,
}

fn grouping_for(spec: &FormatSpec) -> Grouping {
    if spec.kind == u32::from(b'n') {
        // The C locale: no grouping.
        return Grouping { size: 0, sep: 0 };
    }
    match spec.thousands {
        Thousands::None => Grouping { size: 0, sep: 0 },
        Thousands::Comma => Grouping {
            size: 3,
            sep: u32::from(b','),
        },
        Thousands::Underscore => Grouping {
            size: 3,
            sep: u32::from(b'_'),
        },
        Thousands::UnderscoreFour => Grouping {
            size: 4,
            sep: u32::from(b'_'),
        },
    }
}

/// How `_PyUnicode_InsertThousandsGrouping` lays out `digits_len` digits
/// padded to `min_width`: the number of digit positions (digits plus `'0'`
/// padding) and of groups.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GroupShape {
    positions: usize,
    groups: usize,
}

impl GroupShape {
    fn new(digits_len: usize, min_width: usize, g: Grouping) -> Self {
        let Some(block) = g.size.checked_add(1).filter(|_| g.size > 0) else {
            // No grouping: one group, zero-padded to `min_width`.
            return GroupShape {
                positions: digits_len.max(min_width).max(1),
                groups: 1,
            };
        };
        let natural = digits_len.saturating_add(digits_len.saturating_sub(1) / g.size);
        // `group_layout` stops once the digits and `min_width` are used up,
        // but never right after a separator: a total length that is a
        // multiple of `size + 1` gets one more '0'.
        let total = if min_width <= natural {
            natural
        } else if min_width.is_multiple_of(block) {
            min_width.saturating_add(1)
        } else {
            min_width
        };
        let separators = total / block;
        GroupShape {
            positions: total - separators,
            groups: separators + 1,
        }
    }

    /// The grouped length, separators included.
    fn len(self) -> usize {
        self.positions.saturating_add(self.groups.saturating_sub(1))
    }
}

/// Appends `digits`, left-padded with `'0'` and grouped as `shape` says,
/// to `out`, uppercased when `toupper`.
#[cfg(test)]
fn push_grouped(out: &mut Vec<u32>, digits: &[u8], shape: GroupShape, g: Grouping, toupper: bool) {
    let start = out.len();
    out.resize(start + shape.len(), 0);
    if let Some(dest) = out.get_mut(start..) {
        fill_grouped(dest, digits, shape, g, toupper);
    }
}

/// Writes `digits`, left-padded with `'0'` and grouped as `shape` says,
/// over all of `dest` (`shape.len()` long), uppercased when `toupper`.
fn fill_grouped(dest: &mut [u32], digits: &[u8], shape: GroupShape, g: Grouping, toupper: bool) {
    let zeros = shape.positions.saturating_sub(digits.len());
    // Only the first `positions` padded digits are laid out.
    let digits = digits.get(..shape.positions - zeros).unwrap_or(digits);
    let conv = |b: u8| u32::from(if toupper { b.to_ascii_uppercase() } else { b });
    // Filled from the right: every group but the first (leftmost) is full.
    let mut slots = dest.iter_mut().rev();
    let mut seps = shape.groups.saturating_sub(1);
    let mut in_group = 0;
    for &c in digits.iter().rev() {
        if in_group == g.size && seps > 0 {
            if let Some(slot) = slots.next() {
                *slot = g.sep;
            }
            seps -= 1;
            in_group = 0;
        }
        if let Some(slot) = slots.next() {
            *slot = conv(c);
        }
        in_group += 1;
    }
    // The rest is '0' padding, with separators between its groups.
    for slot in slots {
        if in_group == g.size && seps > 0 {
            *slot = g.sep;
            seps -= 1;
            in_group = 0;
        } else {
            *slot = u32::from(b'0');
            in_group += 1;
        }
    }
}

/// What follows the digits and decimal point of a formatted number.
#[derive(Clone, Copy)]
enum Remainder<'a> {
    /// ASCII text (a float's fraction, exponent, `%`).
    Ascii(&'a [u8]),
    /// Two ASCII texts, one after the other (a float's tail, then `%`).
    AsciiThen(&'a [u8], &'a [u8]),
    /// Code points (the character of `'c'`).
    CodePoints(&'a [u32]),
}

impl Remainder<'_> {
    fn len(self) -> usize {
        match self {
            Remainder::Ascii(b) => b.len(),
            Remainder::AsciiThen(a, b) => a.len() + b.len(),
            Remainder::CodePoints(c) => c.len(),
        }
    }

    fn max_char(self) -> u32 {
        let ascii_max = |b: &[u8]| b.iter().copied().max().map_or(0, u32::from);
        match self {
            Remainder::Ascii(b) => ascii_max(b),
            Remainder::AsciiThen(a, b) => ascii_max(a).max(ascii_max(b)),
            Remainder::CodePoints(c) => c.iter().copied().max().unwrap_or(0),
        }
    }
}

/// The parts of a formatted number before padding.
struct NumberParts<'a> {
    /// `'-'` if the number is negative.
    negative: bool,
    /// `0x` etc.
    prefix: &'a str,
    /// The ASCII digits to group.
    digits: &'a [u8],
    /// Whether a decimal point follows the digits.
    has_decimal: bool,
    /// Everything after the digits/decimal point.
    remainder: Remainder<'a>,
}

/// Port of `calc_number_widths` + `fill_number`.
fn fill_number(
    spec: &FormatSpec,
    parts: &NumberParts<'_>,
    toupper: bool,
    out: &mut Vec<u32>,
) -> PcResult<()> {
    let sign: Option<u8> = match spec.sign {
        b'+' => Some(if parts.negative { b'-' } else { b'+' }),
        b' ' => Some(if parts.negative { b'-' } else { b' ' }),
        _ => parts.negative.then_some(b'-'),
    };
    let n_non_digit = usize::from(sign.is_some())
        + parts.prefix.len()
        + usize::from(parts.has_decimal)
        + parts.remainder.len();
    let width = spec.width.unwrap_or(0);
    let min_width = if spec.fill == u32::from(b'0') && spec.align == b'=' {
        width.saturating_sub(n_non_digit)
    } else {
        0
    };
    let g = grouping_for(spec);
    let digit_room = min_width.max(parts.digits.len());
    let est_digits = digit_room.saturating_add(digit_room.checked_div(g.size).unwrap_or(0));
    let est_used = n_non_digit.saturating_add(est_digits);
    let mut maxchar = parts.remainder.max_char();
    if width > est_used {
        maxchar = maxchar.max(spec.fill);
    }
    check_size(width.max(est_used), maxchar)?;
    let shape =
        (!parts.digits.is_empty()).then(|| GroupShape::new(parts.digits.len(), min_width, g));
    let n_grouped = shape.map_or(0, GroupShape::len);
    let used = n_non_digit + n_grouped;
    let padding = width.saturating_sub(used);
    let (mut lpad, mut spad) = (0, 0);
    match spec.align {
        // Right padding is what is left at the end.
        b'<' => {}
        b'^' => lpad = padding / 2,
        b'=' => spad = padding,
        _ => lpad = padding,
    }
    // Pre-filled with the fill character, so padding is just skipped over.
    let total = used + padding;
    out.try_reserve(total).map_err(|_| memory_error())?;
    let start = out.len();
    out.resize(start + total, spec.fill);
    let mut cur = Cursor {
        out: out.get_mut(start..).unwrap_or_default(),
        at: lpad,
    };
    if let Some(s) = sign {
        cur.put(u32::from(s));
    }
    if toupper {
        cur.put_ascii_upper(parts.prefix.as_bytes());
    } else {
        cur.put_ascii(parts.prefix.as_bytes());
    }
    cur.at += spad;
    if let Some(shape) = shape {
        let end = cur.at + shape.len();
        if let Some(dest) = cur.out.get_mut(cur.at..end) {
            fill_grouped(dest, parts.digits, shape, g, toupper);
        }
        cur.at = end;
    }
    if parts.has_decimal {
        cur.put(u32::from(b'.'));
    }
    match parts.remainder {
        Remainder::Ascii(b) => cur.put_ascii(b),
        Remainder::AsciiThen(a, b) => {
            cur.put_ascii(a);
            cur.put_ascii(b);
        }
        Remainder::CodePoints(c) => cur.put_code_points(c),
    }
    Ok(())
}

/// Writes into a pre-sized buffer at a moving position (writes past its
/// end are dropped; callers size it exactly).
struct Cursor<'a> {
    out: &'a mut [u32],
    at: usize,
}

impl Cursor<'_> {
    fn put(&mut self, cp: u32) {
        if let Some(slot) = self.out.get_mut(self.at) {
            *slot = cp;
        }
        self.at += 1;
    }

    fn put_ascii(&mut self, bytes: &[u8]) {
        if let Some(dest) = self.out.get_mut(self.at..) {
            for (slot, &b) in dest.iter_mut().zip(bytes) {
                *slot = u32::from(b);
            }
        }
        self.at += bytes.len();
    }

    fn put_ascii_upper(&mut self, bytes: &[u8]) {
        if let Some(dest) = self.out.get_mut(self.at..) {
            for (slot, &b) in dest.iter_mut().zip(bytes) {
                *slot = u32::from(b.to_ascii_uppercase());
            }
        }
        self.at += bytes.len();
    }

    fn put_code_points(&mut self, cps: &[u32]) {
        if let Some(dest) = self.out.get_mut(self.at..) {
            for (slot, &cp) in dest.iter_mut().zip(cps) {
                *slot = cp;
            }
        }
        self.at += cps.len();
    }
}

/// The magnitude of `n` in `base` (2, 8, 10 or 16), lowercase, no prefix.
/// Base 10 enforces the 4300-digit limit like `str(int)`.
///
/// # Errors
/// `ValueError` for decimal output over the digit limit.
pub fn int_magnitude_digits(n: &BigInt, base: u32) -> PcResult<String> {
    if base == 10 {
        let s = int_str(n)?;
        return Ok(s.trim_start_matches('-').to_owned());
    }
    Ok(n.magnitude().to_str_radix(base))
}

/// The lowercase digits of `m` in `base` (2, 8, 10 or 16), written to the
/// end of `buf` (64 bytes hold any `u64` in base 2).
fn u64_digits(mut m: u64, base: u32, buf: &mut [u8; 64]) -> &[u8] {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut start = buf.len();
    // Shifts for the power-of-two bases; constant divisors for base 10.
    let (shift, mask) = match base {
        2 => (1, 1),
        8 => (3, 7),
        16 => (4, 15),
        _ => (0, 0),
    };
    loop {
        start -= 1;
        let d = if shift == 0 {
            let d = m % 10;
            m /= 10;
            d
        } else {
            let d = m & mask;
            m >>= shift;
            d
        };
        buf[start] = DIGITS[usize::try_from(d).unwrap_or(0)];
        if m == 0 {
            return &buf[start..];
        }
    }
}

/// Port of `format_long_internal`: `n` with an int presentation type.
pub(crate) fn format_int(n: &BigInt, spec: &FormatSpec, out: &mut Vec<u32>) -> PcResult<()> {
    if spec.precision.is_some() {
        return Err(PyError::value_error(
            "Precision not allowed in integer format specifier",
        ));
    }
    if spec.no_neg_0 {
        return Err(PyError::value_error(
            "Negative zero coercion (z) not allowed in integer format specifier",
        ));
    }
    let kind = char::from_u32(spec.kind).unwrap_or('d');
    if kind == 'c' {
        if spec.sign != 0 {
            return Err(PyError::value_error(
                "Sign not allowed with integer format specifier 'c'",
            ));
        }
        if spec.alternate {
            return Err(PyError::value_error(
                "Alternate form (#) not allowed with integer format specifier 'c'",
            ));
        }
        let x = n
            .to_i64()
            .ok_or_else(|| PyError::overflow_error("Python int too large to convert to C long"))?;
        let cp = u32::try_from(x)
            .ok()
            .filter(|&c| c <= 0x10_FFFF)
            .ok_or_else(|| PyError::overflow_error("%c arg not in range(0x110000)"))?;
        let rem = [cp];
        return fill_number(
            spec,
            &NumberParts {
                negative: false,
                prefix: "",
                digits: &[],
                has_decimal: false,
                remainder: Remainder::CodePoints(&rem),
            },
            false,
            out,
        );
    }
    let (base, prefix) = match kind {
        'b' => (2, "0b"),
        'o' => (8, "0o"),
        'x' | 'X' => (16, "0x"),
        _ => (10, ""),
    };
    let mut small = [0_u8; 64];
    let owned;
    let digits: &[u8] = match n.magnitude().to_u64() {
        Some(m) => u64_digits(m, base, &mut small),
        None => {
            owned = int_magnitude_digits(n, base)?;
            owned.as_bytes()
        }
    };
    fill_number(
        spec,
        &NumberParts {
            negative: n.sign() == Sign::Minus,
            prefix: if spec.alternate { prefix } else { "" },
            digits,
            has_decimal: false,
            remainder: Remainder::Ascii(&[]),
        },
        kind == 'X',
        out,
    )
}

/// Flags for [`double_to_string`] (CPython's `Py_DTSF_*`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DtoaFlags {
    /// `Py_DTSF_SIGN`: always add a sign.
    pub sign: bool,
    /// `Py_DTSF_ADD_DOT_0`: integers get `.0`.
    pub add_dot_0: bool,
    /// `Py_DTSF_ALT`: alternate form (`#`).
    pub alt: bool,
    /// `Py_DTSF_NO_NEG_0`: negative zero becomes zero.
    pub no_neg_0: bool,
}

/// Significant decimal digits: inline for the fast paths, else on the heap.
enum Digits {
    /// The last `len` bytes of `buf` (39 bytes hold any `u128`).
    Inline {
        buf: [u8; 39],
        start: usize,
    },
    Heap(Vec<u8>),
}

impl std::ops::Deref for Digits {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        match self {
            Digits::Inline { buf, start } => buf.get(*start..).unwrap_or_default(),
            Digits::Heap(v) => v,
        }
    }
}

/// `_Py_dg_dtoa` digits for a finite `abs >= 0`: significant digits with
/// no trailing zeros (zero is `"0"`) and the decimal-point position.
fn dtoa_digits(abs: f64, mode: u8, ndigits: usize) -> (Digits, i64) {
    if mode == 3 {
        if let Some(found) = fixed_digits_fast(abs, ndigits) {
            return found;
        }
    }
    let (digits, decpt) = dtoa_digits_vec(abs, mode, ndigits);
    (Digits::Heap(digits), decpt)
}

fn dtoa_digits_vec(abs: f64, mode: u8, ndigits: usize) -> (Vec<u8>, i64) {
    let from_sci = |sci: &str| -> (Vec<u8>, i64) {
        let (mant, exp) = sci.split_once('e').unwrap_or((sci, "0"));
        let mut digits: Vec<u8> = mant.bytes().filter(u8::is_ascii_digit).collect();
        while digits.len() > 1 && digits.last() == Some(&b'0') {
            digits.pop();
        }
        let exp: i64 = exp.parse().unwrap_or(0);
        if digits == b"0" {
            return (digits, 1);
        }
        (digits, exp + 1)
    };
    // Rust's precision formatting is exact (correctly rounded, ties to
    // even), like `_Py_dg_dtoa`. A finite f64 has at most 767 significant
    // digits and 1074 fractional ones, so larger requests only add zeros.
    match mode {
        0 => {
            if abs == 0.0 {
                return (b"0".to_vec(), 1);
            }
            let (digits, exp) = crate::numbers::shortest_digits(abs);
            (digits.to_vec(), exp + 1)
        }
        2 => from_sci(&format!("{:.*e}", ndigits.clamp(1, 800) - 1, abs)),
        _ => {
            let ndigits = ndigits.min(1100);
            let fixed = format!("{abs:.ndigits$}");
            let (int_part, frac) = fixed.split_once('.').unwrap_or((&fixed, ""));
            let mut decpt = i64::try_from(int_part.len()).unwrap_or(0);
            let mut all: Vec<u8> = int_part.bytes().chain(frac.bytes()).collect();
            let lead = all.iter().take_while(|&&b| b == b'0').count();
            all.drain(..lead);
            decpt -= i64::try_from(lead).unwrap_or(0);
            while all.last() == Some(&b'0') {
                all.pop();
            }
            if all.is_empty() {
                (b"0".to_vec(), 1)
            } else {
                (all, decpt)
            }
        }
    }
}

/// Mode 3 digits from an exact `u128` rounding of `abs * 10**ndigits`,
/// skipping Rust's `{:.N}` formatting. `None` when that doesn't apply.
fn fixed_digits_fast(abs: f64, ndigits: usize) -> Option<(Digits, i64)> {
    let nd = u32::try_from(ndigits).ok()?;
    let mut q = crate::numbers::decimal::scaled_round_half_even(abs, nd)?;
    let mut buf = [0_u8; 39];
    if q == 0 {
        buf[38] = b'0';
        return Some((Digits::Inline { buf, start: 38 }, 1));
    }
    let mut start = buf.len();
    // Trailing zeros aren't significant digits.
    let mut zeros = 0_i64;
    match u64::try_from(q) {
        // Constant `u64` divisions are much cheaper than `u128` ones.
        Ok(mut v) => {
            while v % 10 == 0 {
                v /= 10;
                zeros += 1;
            }
            loop {
                start -= 1;
                buf[start] = b'0' + u8::try_from(v % 10).unwrap_or(0);
                v /= 10;
                if v == 0 {
                    break;
                }
            }
        }
        Err(_) => {
            while q % 10 == 0 {
                q /= 10;
                zeros += 1;
            }
            loop {
                start -= 1;
                buf[start] = b'0' + u8::try_from(q % 10).unwrap_or(0);
                q /= 10;
                if q == 0 {
                    break;
                }
            }
        }
    }
    let n_digits = i64::try_from(buf.len() - start).ok()?;
    let decpt = n_digits + zeros - i64::from(nd);
    Some((Digits::Inline { buf, start }, decpt))
}

/// Port of CPython's `PyOS_double_to_string` (`format_float_short`).
/// `code` is one of `e E f F g G r`; `r` requires `precision == 0`.
///
/// Returns an empty string if the output can't be allocated; use
/// [`try_double_to_string`] to see that as `MemoryError`.
#[must_use]
pub fn double_to_string(val: f64, code: char, precision: usize, flags: DtoaFlags) -> String {
    try_double_to_string(val, code, precision, flags).unwrap_or_default()
}

/// [`double_to_string`], failing with `MemoryError` when the output can't
/// be allocated.
///
/// # Errors
/// `MemoryError` for an output too large to allocate.
pub fn try_double_to_string(
    val: f64,
    code: char,
    precision: usize,
    flags: DtoaFlags,
) -> PcResult<String> {
    let mut out = Vec::new();
    write_double(val, code, precision, flags, &mut out)?;
    // Only ASCII is ever written.
    Ok(String::from_utf8(out).unwrap_or_default())
}

/// Appends `n` ASCII `'0'`s (none for `n <= 0`).
fn push_zeros(out: &mut impl ByteSink, n: i64) {
    if let Ok(n) = usize::try_from(n) {
        out.push_zero_bytes(n);
    }
}

/// Where [`write_double`] writes its ASCII text.
trait ByteSink {
    /// Makes room for `n` more bytes, or fails with `MemoryError`.
    fn try_room(&mut self, n: usize) -> PcResult<()>;
    fn push_byte(&mut self, b: u8);
    fn push_bytes(&mut self, bytes: &[u8]);
    fn push_zero_bytes(&mut self, n: usize);
    fn byte_len(&self) -> usize;
    fn last_byte(&self) -> Option<u8>;
    fn pop_byte(&mut self);
}

impl ByteSink for Vec<u8> {
    fn try_room(&mut self, n: usize) -> PcResult<()> {
        self.try_reserve_exact(n).map_err(|_| memory_error())
    }

    fn push_byte(&mut self, b: u8) {
        Vec::push(self, b);
    }

    fn push_bytes(&mut self, bytes: &[u8]) {
        Vec::extend_from_slice(self, bytes);
    }

    fn push_zero_bytes(&mut self, n: usize) {
        self.resize(self.len() + n, b'0');
    }

    fn byte_len(&self) -> usize {
        Vec::len(self)
    }

    fn last_byte(&self) -> Option<u8> {
        self.as_slice().last().copied()
    }

    fn pop_byte(&mut self) {
        Vec::pop(self);
    }
}

/// A fixed stack buffer for short float text. It never grows: asking for
/// more room than it has, or writing past its end, marks it as overflowed,
/// and callers then redo the work with a `Vec`.
struct StackBytes {
    buf: [u8; STACK_BYTES],
    len: usize,
    overflowed: bool,
}

const STACK_BYTES: usize = 64;

impl StackBytes {
    fn new() -> Self {
        StackBytes {
            buf: [0; STACK_BYTES],
            len: 0,
            overflowed: false,
        }
    }

    /// The text, or `None` if it didn't fit.
    fn bytes(&self) -> Option<&[u8]> {
        if self.overflowed {
            return None;
        }
        self.buf.get(..self.len)
    }
}

impl ByteSink for StackBytes {
    fn try_room(&mut self, n: usize) -> PcResult<()> {
        if self.len.saturating_add(n) > STACK_BYTES {
            self.overflowed = true;
            // Any error makes the caller retry with a `Vec`.
            return Err(memory_error());
        }
        Ok(())
    }

    fn push_byte(&mut self, b: u8) {
        self.push_bytes(&[b]);
    }

    fn push_bytes(&mut self, bytes: &[u8]) {
        let end = self.len.saturating_add(bytes.len());
        match self.buf.get_mut(self.len..end) {
            Some(dst) => {
                dst.copy_from_slice(bytes);
                self.len = end;
            }
            None => self.overflowed = true,
        }
    }

    fn push_zero_bytes(&mut self, n: usize) {
        let end = self.len.saturating_add(n);
        match self.buf.get_mut(self.len..end) {
            Some(dst) => {
                dst.fill(b'0');
                self.len = end;
            }
            None => self.overflowed = true,
        }
    }

    fn byte_len(&self) -> usize {
        self.len
    }

    fn last_byte(&self) -> Option<u8> {
        self.len
            .checked_sub(1)
            .and_then(|i| self.buf.get(i))
            .copied()
    }

    fn pop_byte(&mut self) {
        self.len = self.len.saturating_sub(1);
    }
}

/// [`try_double_to_string`], appending its ASCII text to `out`.
fn write_double(
    val: f64,
    code: char,
    precision: usize,
    flags: DtoaFlags,
    out: &mut impl ByteSink,
) -> PcResult<()> {
    let upper = code.is_ascii_uppercase();
    let code = code.to_ascii_lowercase();
    let (mode, mut precision) = match code {
        'e' => (2, precision.saturating_add(1)),
        'f' => (3, precision),
        'g' => (2, precision.max(1)),
        _ => (0, 0),
    };
    if !val.is_finite() {
        out.push_bytes(non_finite_to_string(val, upper, flags.sign).as_bytes());
        return Ok(());
    }
    let mut negative = val.is_sign_negative();
    if code == 'r' {
        precision = 17;
    }
    let (digits, mut decpt) = dtoa_digits(val.abs(), mode, precision);
    if flags.no_neg_0 && negative && *digits == *b"0" {
        negative = false;
    }
    let digits_len = i64::try_from(digits.len()).unwrap_or(i64::MAX);
    let prec = i64::try_from(precision).unwrap_or(i64::MAX);
    let mut vdigits_end = digits_len;
    let mut use_exp = false;
    match code {
        'e' => {
            use_exp = true;
            vdigits_end = prec;
        }
        'f' => vdigits_end = decpt.saturating_add(prec),
        'g' => {
            let limit = if flags.add_dot_0 { prec - 1 } else { prec };
            if decpt <= -4 || decpt > limit {
                use_exp = true;
            }
            if flags.alt {
                vdigits_end = prec;
            }
        }
        _ => {
            if decpt <= -4 || decpt > 16 {
                use_exp = true;
            }
        }
    }
    let mut exp = 0;
    if use_exp {
        exp = decpt - 1;
        decpt = 1;
    }
    let vdigits_start = if decpt <= 0 { decpt - 1 } else { 0 };
    vdigits_end = if !use_exp && flags.add_dot_0 {
        vdigits_end.max(decpt + 1)
    } else {
        vdigits_end.max(decpt)
    };
    let span = usize::try_from(vdigits_end.saturating_sub(vdigits_start)).unwrap_or(usize::MAX);
    out.try_room(span.saturating_add(16))?;
    let begin = out.byte_len();
    if negative {
        out.push_byte(b'-');
    } else if flags.sign {
        out.push_byte(b'+');
    }
    if decpt <= 0 {
        push_zeros(out, decpt - vdigits_start);
        out.push_byte(b'.');
        push_zeros(out, -decpt);
    } else {
        push_zeros(out, -vdigits_start);
    }
    if 0 < decpt && decpt <= digits_len {
        let d = usize::try_from(decpt).unwrap_or(0);
        let (int_part, frac) = digits.split_at(d.min(digits.len()));
        out.push_bytes(int_part);
        out.push_byte(b'.');
        out.push_bytes(frac);
    } else {
        out.push_bytes(&digits);
    }
    if digits_len < decpt {
        push_zeros(out, decpt - digits_len);
        out.push_byte(b'.');
        push_zeros(out, vdigits_end - decpt);
    } else {
        push_zeros(out, vdigits_end - digits_len);
    }
    if out.byte_len() > begin && out.last_byte() == Some(b'.') && !flags.alt {
        out.pop_byte();
    }
    if use_exp {
        out.push_byte(if upper { b'E' } else { b'e' });
        // `%+.02d`: a sign and at least two digits.
        out.push_byte(if exp < 0 { b'-' } else { b'+' });
        let mut buf = [0_u8; 20];
        let mut at = buf.len();
        let mut e = exp.unsigned_abs();
        loop {
            at -= 1;
            buf[at] = b'0' + u8::try_from(e % 10).unwrap_or(0);
            e /= 10;
            if e == 0 {
                break;
            }
        }
        if buf.len() - at < 2 {
            out.push_byte(b'0');
        }
        out.push_bytes(&buf[at..]);
    }
    Ok(())
}

/// Writes `-` for a negative value, or `+` when `always_sign` asks for it.
fn push_sign(out: &mut String, negative: bool, always_sign: bool) {
    if negative {
        out.push('-');
    } else if always_sign {
        out.push('+');
    }
}

/// `inf`/`nan` text; NaN never shows its sign bit.
fn non_finite_to_string(val: f64, upper: bool, always_sign: bool) -> String {
    let mut out = String::new();
    push_sign(
        &mut out,
        val.is_sign_negative() && !val.is_nan(),
        always_sign,
    );
    out.push_str(match (val.is_nan(), upper) {
        (true, false) => "nan",
        (true, true) => "NAN",
        (false, false) => "inf",
        (false, true) => "INF",
    });
    out
}

/// `format_float_short` does `precision++` for 'e' in a C int: INT_MAX
/// wraps to INT_MIN, which dtoa treats like 1 digit, the same output as
/// precision 0. Returns the precision that gives CPython's output.
pub(crate) fn wrap_e_precision(code: char, precision: usize) -> usize {
    if matches!(code, 'e' | 'E') && precision == i32::MAX as usize {
        0
    } else {
        precision
    }
}

/// Port of `format_float_internal`.
pub(crate) fn format_float(val: f64, spec: &FormatSpec, out: &mut Vec<u32>) -> PcResult<()> {
    let precision = spec.precision;
    if precision.is_some_and(|p| p > i32::MAX as usize) {
        return Err(PyError::value_error("precision too big"));
    }
    let mut flags = DtoaFlags {
        alt: spec.alternate,
        no_neg_0: spec.no_neg_0,
        ..DtoaFlags::default()
    };
    let mut kind = char::from_u32(spec.kind).unwrap_or('\0');
    let mut default_precision = 6;
    if kind == '\0' {
        flags.add_dot_0 = true;
        kind = 'r';
        default_precision = 0;
    }
    if kind == 'n' {
        kind = 'g';
    }
    let mut val = val;
    let mut add_pct = false;
    if kind == '%' {
        kind = 'f';
        val *= 100.0;
        add_pct = true;
    }
    let precision = match precision {
        None => default_precision,
        Some(p) => {
            if kind == 'r' {
                kind = 'g';
            }
            p
        }
    };
    let precision = wrap_e_precision(kind, precision);
    check_float_memory(val, kind, precision, flags.alt, spec.width.unwrap_or(0))?;
    // Short text (the usual case) stays on the stack.
    let mut small = StackBytes::new();
    let mut heap = Vec::new();
    let small_ok = write_double(val, kind, precision, flags, &mut small).is_ok();
    let text: &[u8] = match small.bytes().filter(|_| small_ok) {
        Some(text) => text,
        None => {
            write_double(val, kind, precision, flags, &mut heap)?;
            &heap
        }
    };
    let pct: &[u8] = if add_pct { b"%" } else { b"" };
    let (negative, body) = match text.split_first() {
        Some((b'-', rest)) => (true, rest),
        _ => (false, text),
    };
    let n_digits = body.iter().take_while(|b| b.is_ascii_digit()).count();
    let (digits, after) = body.split_at(n_digits);
    let (has_decimal, remainder) = match after.split_first() {
        Some((b'.', r)) => (true, r),
        _ => (false, after),
    };
    fill_number(
        spec,
        &NumberParts {
            negative,
            prefix: "",
            digits,
            has_decimal,
            remainder: Remainder::AsciiThen(remainder, pct),
        },
        false,
        out,
    )
}

/// Precisions and widths below this can't come near [`CPYTHON_BUDGET`].
const SURELY_FITS: usize = 1 << 20;

/// Models the peak memory of CPython's `format_float_internal`: `_Py_dg_dtoa`
/// allocates about `precision` bytes for its digits (`rv_alloc`), the
/// formatted buffer is alive with it, and then the buffer, its str copy and
/// the output writer are alive together.
fn check_float_memory(
    val: f64,
    kind: char,
    precision: usize,
    alt: bool,
    width: usize,
) -> PcResult<()> {
    if !val.is_finite() || kind == 'r' {
        return check_size(width, 0);
    }
    // Below these sizes every estimate is a few MiB at most (a finite f64
    // has at most 309 integer digits), far under the budget, so skip the
    // `log10`.
    if precision < SURELY_FITS && width < SURELY_FITS {
        return Ok(());
    }
    // Decimal exponent, roughly: digits before the point.
    let int_digits = if val.abs() >= 1.0 {
        // Bounded by 309 for a finite f64.
        usize::try_from(val.abs().log10().floor() as i64 + 1).unwrap_or(0)
    } else {
        0
    };
    let (dtoa, buf) = match kind {
        'e' | 'E' => (precision.saturating_add(1), precision.saturating_add(1)),
        'f' | 'F' => (
            precision.saturating_add(int_digits).saturating_add(1),
            precision.saturating_add(int_digits),
        ),
        _ => (precision.max(1), if alt { precision } else { 0 }),
    };
    // `_Py_dg_dtoa` returns "0" for zero before allocating `precision` bytes.
    let dtoa = if val == 0.0 { 1 } else { dtoa };
    let buf = buf.saturating_add(8);
    let first = dtoa.saturating_add(buf);
    let second = buf.saturating_mul(2).saturating_add(width.max(buf));
    check_bytes(first.max(second))
}

/// Converts an int for a float presentation type (`PyNumber_Float`).
pub(crate) fn int_as_float(n: &BigInt) -> PcResult<f64> {
    int_to_float(n)
}

#[cfg(test)]
mod grouping_tests {
    use super::*;

    /// CPython's `_PyUnicode_InsertThousandsGrouping` loop, literally.
    fn old_group_digits(digits: &[u8], min_width: usize, g: Grouping) -> Vec<u32> {
        let mut rev: Vec<u32> = Vec::new();
        let mut min_width = i128::try_from(min_width).unwrap_or(i128::MAX);
        let mut remaining = i128::try_from(digits.len()).unwrap_or(i128::MAX);
        let mut pos = digits.len();
        let mut use_sep = false;
        let mut push = |rev: &mut Vec<u32>, n_zeros: i128, n_chars: i128, use_sep: bool| {
            let n_chars = usize::try_from(n_chars).unwrap_or(0);
            let n_zeros = usize::try_from(n_zeros).unwrap_or(0);
            if use_sep {
                rev.push(g.sep);
            }
            let start = pos.saturating_sub(n_chars);
            rev.extend(digits[start..pos].iter().rev().map(|&b| u32::from(b)));
            rev.extend(std::iter::repeat_n(u32::from(b'0'), n_zeros));
            pos = start;
        };
        if g.size > 0 {
            let size = i128::try_from(g.size).unwrap_or(3);
            loop {
                let len = size.min(remaining.max(min_width).max(1));
                let n_zeros = (len - remaining).max(0);
                let n_chars = remaining.min(len).max(0);
                push(&mut rev, n_zeros, n_chars, use_sep);
                use_sep = true;
                remaining -= n_chars;
                min_width -= len;
                if remaining <= 0 && min_width <= 0 {
                    rev.reverse();
                    return rev;
                }
                min_width -= 1;
            }
        }
        let len = remaining.max(min_width).max(1);
        let n_zeros = (len - remaining).max(0);
        let n_chars = remaining.min(len).max(0);
        push(&mut rev, n_zeros, n_chars, use_sep);
        rev.reverse();
        rev
    }
    #[test]
    fn shape_matches_cpython_loop() {
        let all = b"123456789abcdef0123456789";
        for size in [0usize, 1, 2, 3, 4] {
            let g = Grouping {
                size,
                sep: u32::from(b','),
            };
            for len in 1..=all.len() {
                for mw in 0..80 {
                    let d = &all[..len];
                    let old = old_group_digits(d, mw, g);
                    let sh = GroupShape::new(len, mw, g);
                    let mut new = Vec::new();
                    push_grouped(&mut new, d, sh, g, false);
                    assert_eq!(old, new, "size {size} len {len} mw {mw}");
                    assert_eq!(sh.len(), new.len());
                }
            }
        }
    }
}
