//! Exact int <-> decimal text conversions, faster than num-bigint's generic
//! radix code for the sizes `str(int)` and `int(text)` allow (at most 4300
//! digits). Both work in base `10**9` chunks over `u32` limbs, so every
//! step is a `u64` operation with a constant divisor or multiplier.

use num_bigint::BigUint;

/// `10**9`, the chunk size of both conversions.
const CHUNK: u32 = 1_000_000_000;
const CHUNK_DIGITS: usize = 9;

/// Where decimal text is written: a `String` or `PyStr` code points.
pub(crate) trait AsciiSink {
    /// Makes room for `n` more bytes.
    fn reserve_ascii(&mut self, n: usize);
    /// Appends ASCII bytes.
    fn push_ascii(&mut self, bytes: &[u8]);
}

impl AsciiSink for String {
    fn reserve_ascii(&mut self, n: usize) {
        self.reserve(n);
    }

    fn push_ascii(&mut self, bytes: &[u8]) {
        self.push_str(std::str::from_utf8(bytes).unwrap_or(""));
    }
}

impl AsciiSink for Vec<u32> {
    fn reserve_ascii(&mut self, n: usize) {
        self.reserve(n);
    }

    fn push_ascii(&mut self, bytes: &[u8]) {
        self.extend(bytes.iter().map(|&b| u32::from(b)));
    }
}

/// Magnitudes up to this many `u32` limbs (1024 bits) convert without
/// heap scratch space.
const STACK_LIMBS: usize = 32;

/// Divisions by 10**9 per sweep over the limbs.
const SWEEP: usize = 4;

/// Base-10**9 chunks needed for `limbs` `u32` limbs (each chunk holds more
/// than 29 bits), plus the zero chunks a last sweep may add.
const fn chunk_capacity(limbs: usize) -> usize {
    limbs * 32 / 29 + SWEEP
}

/// Appends the decimal digits of `n` (no sign, no leading zeros; `"0"` for
/// zero) to `out`.
pub(crate) fn magnitude_to_decimal(n: &BigUint, out: &mut impl AsciiSink) {
    let n_limbs = usize::try_from(n.bits().div_ceil(32)).unwrap_or(usize::MAX);
    if n_limbs <= STACK_LIMBS {
        let mut limbs = [0_u32; STACK_LIMBS];
        for (slot, digit) in limbs.iter_mut().zip(n.iter_u32_digits()) {
            *slot = digit;
        }
        let mut chunks = [0_u32; chunk_capacity(STACK_LIMBS)];
        let used = to_chunks(limbs.get_mut(..n_limbs).unwrap_or_default(), &mut chunks);
        emit_chunks(chunks.get(..used).unwrap_or_default(), out);
    } else {
        let mut limbs = n.to_u32_digits();
        let mut chunks = vec![0_u32; chunk_capacity(limbs.len())];
        let used = to_chunks(&mut limbs, &mut chunks);
        emit_chunks(chunks.get(..used).unwrap_or_default(), out);
    }
}

/// Divides `limbs` (least significant first, consumed) down to base-10**9
/// `chunks`, least significant first. Returns how many chunks were written
/// (no zero top chunks; zero itself gives none).
fn to_chunks(limbs: &mut [u32], chunks: &mut [u32]) -> usize {
    let div = |rem: u64, limb: u32| -> (u32, u64) {
        let cur = (rem << 32) | u64::from(limb);
        // rem < 10**9, so cur < 10**9 * 2**32 and the quotient fits a u32.
        (
            u32::try_from(cur / u64::from(CHUNK)).unwrap_or(u32::MAX),
            cur % u64::from(CHUNK),
        )
    };
    let mut len = limbs.len();
    while len > 0 && limbs.get(len - 1) == Some(&0) {
        len -= 1;
    }
    let mut used = 0;
    while len > 0 {
        // `SWEEP` divisions by 10**9 per sweep: each one consumes the
        // quotient limb of the previous as soon as it exists, so the
        // remainder chains run side by side.
        let mut rems = [0_u64; SWEEP];
        for limb in limbs.iter_mut().take(len).rev() {
            let mut q = *limb;
            for rem in &mut rems {
                let (next_q, r) = div(*rem, q);
                *rem = r;
                q = next_q;
            }
            *limb = q;
        }
        for rem in rems {
            if let Some(slot) = chunks.get_mut(used) {
                *slot = u32::try_from(rem).unwrap_or(0);
                used += 1;
            }
        }
        while len > 0 && limbs.get(len - 1) == Some(&0) {
            len -= 1;
        }
    }
    // The last sweep may leave a zero top chunk (never the only one).
    while used > 1 && chunks.get(used - 1) == Some(&0) {
        used -= 1;
    }
    used
}

fn emit_chunks(chunks: &[u32], out: &mut impl AsciiSink) {
    let Some((&top, rest)) = chunks.split_last() else {
        out.push_ascii(b"0");
        return;
    };
    out.reserve_ascii(CHUNK_DIGITS * (rest.len() + 1));
    // The leading chunk has no padding.
    let mut nine = [0u8; CHUNK_DIGITS];
    write_nine(top, &mut nine);
    let lead = nine
        .iter()
        .take(CHUNK_DIGITS - 1)
        .take_while(|&&b| b == b'0')
        .count();
    out.push_ascii(nine.get(lead..).unwrap_or_default());
    // The rest are zero-padded, written a batch at a time.
    const BATCH: usize = 8;
    let mut buf = [0u8; CHUNK_DIGITS * BATCH];
    let mut chunks = rest.iter().rev().copied().peekable();
    while chunks.peek().is_some() {
        let mut used = 0;
        for (slot, chunk) in buf
            .as_chunks_mut::<CHUNK_DIGITS>()
            .0
            .iter_mut()
            .zip(chunks.by_ref().take(BATCH))
        {
            write_nine(chunk, slot);
            used += CHUNK_DIGITS;
        }
        out.push_ascii(buf.get(..used).unwrap_or_default());
    }
}

/// `"00".."99"`.
const PAIRS: &[u8; 200] = b"0001020304050607080910111213141516171819\
2021222324252627282930313233343536373839\
4041424344454647484950515253545556575859\
6061626364656667686970717273747576777879\
8081828384858687888990919293949596979899";

/// The decimal digits of `v` (no leading zeros; `"0"` for zero), written
/// to the end of `buf`, two at a time.
pub(crate) fn u64_to_ascii(mut v: u64, buf: &mut [u8; 20]) -> &[u8] {
    let mut start = buf.len();
    while v >= 100 {
        let i = usize::try_from(v % 100).unwrap_or(0) * 2;
        v /= 100;
        start -= 2;
        buf[start] = PAIRS[i];
        buf[start + 1] = PAIRS[i + 1];
    }
    if v >= 10 {
        let i = usize::try_from(v).unwrap_or(0) * 2;
        start -= 2;
        buf[start] = PAIRS[i];
        buf[start + 1] = PAIRS[i + 1];
    } else {
        start -= 1;
        buf[start] = b'0' + u8::try_from(v).unwrap_or(0);
    }
    &buf[start..]
}

/// Writes `v < 10**9` as exactly nine ASCII digits.
fn write_nine(v: u32, buf: &mut [u8; CHUNK_DIGITS]) {
    let pair = |n: u32| -> [u8; 2] {
        let i = usize::try_from(n * 2).unwrap_or(0);
        [PAIRS[i % 200], PAIRS[(i + 1) % 200]]
    };
    let top = v / 100_000_000;
    let low = v % 100_000_000;
    let (hi4, lo4) = (low / 10_000, low % 10_000);
    let digits = [
        pair(hi4 / 100),
        pair(hi4 % 100),
        pair(lo4 / 100),
        pair(lo4 % 100),
    ];
    buf[0] = b'0' + u8::try_from(top % 10).unwrap_or(0);
    for (dst, src) in buf[1..].as_chunks_mut::<2>().0.iter_mut().zip(digits) {
        *dst = src;
    }
}

/// The value of ASCII decimal `digits`, skipping `_` separators. Every
/// other byte must be an ASCII digit (callers validate first).
pub(crate) fn decimal_to_magnitude(digits: &[u8]) -> BigUint {
    let owned: Vec<u8>;
    let digits = if digits.contains(&b'_') {
        owned = digits.iter().copied().filter(|&b| b != b'_').collect();
        &owned[..]
    } else {
        digits
    };
    // `u64` limbs, least significant first: one per 16-digit chunk is
    // enough room (10**16 < 2**64).
    let n_limbs = digits.len() / WIDE_DIGITS + 1;
    let mut stack = [0_u64; STACK_WIDE_LIMBS];
    let mut heap = Vec::new();
    let limbs: &mut [u64] = if n_limbs <= STACK_WIDE_LIMBS {
        stack.get_mut(..n_limbs).unwrap_or_default()
    } else {
        heap.resize(n_limbs, 0);
        &mut heap
    };
    let mut len = 0;
    // The first chunk takes the leftover digits so the rest are full.
    let (head, rest) = digits.split_at(digits.len() % WIDE_DIGITS);
    if !head.is_empty() {
        let width = u32::try_from(head.len()).unwrap_or(0);
        wide_mul_add(limbs, &mut len, 10_u64.pow(width), wide_chunk_value(head));
    }
    for chunk in rest.as_chunks::<WIDE_DIGITS>().0 {
        wide_mul_add(limbs, &mut len, WIDE_CHUNK, wide_chunk_value(chunk));
    }
    let limbs = limbs.get(..len).unwrap_or_default();
    if let [low] = limbs {
        return BigUint::from(*low);
    }
    let split = |limb: u64| {
        [
            u32::try_from(limb & 0xffff_ffff).unwrap_or(0),
            u32::try_from(limb >> 32).unwrap_or(0),
        ]
    };
    if limbs.len() <= STACK_WIDE_LIMBS {
        let mut halves = [0_u32; 2 * STACK_WIDE_LIMBS];
        for (pair, &limb) in halves.as_chunks_mut::<2>().0.iter_mut().zip(limbs) {
            *pair = split(limb);
        }
        return BigUint::from_slice(halves.get(..2 * limbs.len()).unwrap_or_default());
    }
    BigUint::new(limbs.iter().flat_map(|&limb| split(limb)).collect())
}

/// Digits per chunk of [`decimal_to_magnitude`], and its scale.
const WIDE_DIGITS: usize = 16;
const WIDE_CHUNK: u64 = 10_000_000_000_000_000;
/// Texts up to `16 * 40 = 640` digits (the most CPython parses without its
/// digit-limit check) use stack scratch space.
const STACK_WIDE_LIMBS: usize = 41;

/// The value of at most 16 ASCII digits.
fn wide_chunk_value(digits: &[u8]) -> u64 {
    let (high, low) = digits.split_at(digits.len().saturating_sub(8));
    let low_value = match <&[u8; 8]>::try_from(low) {
        Ok(eight) => u64::from(eight_digits_value(eight)),
        Err(_) => u64::from(chunk_value(low)),
    };
    if high.is_empty() {
        return low_value;
    }
    let high_value = match <&[u8; 8]>::try_from(high) {
        Ok(eight) => u64::from(eight_digits_value(eight)),
        Err(_) => u64::from(chunk_value(high)),
    };
    high_value * 100_000_000 + low_value
}

/// `limbs[..len] = limbs[..len] * scale + add` (growing `len`), for
/// `scale <= 10**16` and `add < scale`. `limbs` has room for the result.
fn wide_mul_add(limbs: &mut [u64], len: &mut usize, scale: u64, add: u64) {
    let mut carry = add;
    for limb in limbs.iter_mut().take(*len) {
        let t = u128::from(*limb) * u128::from(scale) + u128::from(carry);
        *limb = u64::try_from(t & u128::from(u64::MAX)).unwrap_or(0);
        carry = u64::try_from(t >> 64).unwrap_or(0);
    }
    if carry != 0 {
        if let Some(slot) = limbs.get_mut(*len) {
            *slot = carry;
            *len += 1;
        }
    }
}

/// The value of eight ASCII digits, combined pairwise in one `u64`
/// (independent lanes instead of a chain of eight multiply-adds).
fn eight_digits_value(digits: &[u8; 8]) -> u32 {
    let mut v = u64::from_le_bytes(*digits).wrapping_sub(0x3030_3030_3030_3030);
    // Bytes hold digits, most significant first (lowest address = low byte).
    v = (v.wrapping_mul(10) + (v >> 8)) & 0x00FF_00FF_00FF_00FF;
    v = (v.wrapping_mul(100) + (v >> 16)) & 0x0000_FFFF_0000_FFFF;
    v = (v.wrapping_mul(10_000) + (v >> 32)) & 0xFFFF_FFFF;
    u32::try_from(v).unwrap_or(0)
}

/// The value of at most nine ASCII digits (so it can't overflow a `u32`).
fn chunk_value(digits: &[u8]) -> u32 {
    if let Some((&first, rest)) = digits.split_first() {
        if let Ok(eight) = <&[u8; 8]>::try_from(rest) {
            return u32::from(first.wrapping_sub(b'0')) * 100_000_000 + eight_digits_value(eight);
        }
    }
    digits.iter().fold(0_u32, |acc, &b| {
        acc.wrapping_mul(10)
            .wrapping_add(u32::from(b.wrapping_sub(b'0')))
    })
}

/// `round(abs * 10**nd)`, half to even, computed on the exact binary value
/// of `abs` (finite, `>= 0`), when that fits plain `u128` arithmetic: `nd`
/// at most 22 and `abs` normal or zero. `None` otherwise; callers keep their
/// general path for that.
pub(crate) fn scaled_round_half_even(abs: f64, nd: u32) -> Option<u128> {
    if abs == 0.0 {
        return Some(0);
    }
    if nd > 22 || !abs.is_finite() || abs.is_sign_negative() {
        return None;
    }
    let bits = abs.to_bits();
    let biased = (bits >> 52) & 0x7ff;
    if biased == 0 {
        return None;
    }
    let m = (bits & ((1_u64 << 52) - 1)) | (1_u64 << 52);
    // m < 2**53 and 10**22 < 2**74, so the product fits.
    let scaled = u128::from(m) * 10_u128.pow(nd);
    let e = i64::try_from(biased).ok()? - 1075;
    if e >= 0 {
        let e = u32::try_from(e).ok()?;
        return (e < scaled.leading_zeros()).then(|| scaled << e);
    }
    let shift = u32::try_from(-e).ok()?;
    if shift >= 128 {
        // scaled < 2**127 <= half a unit: rounds to 0.
        return Some(0);
    }
    let q = scaled >> shift;
    let r = scaled & ((1_u128 << shift) - 1);
    let half = 1_u128 << (shift - 1);
    Some(if r > half || (r == half && q & 1 == 1) {
        q + 1
    } else {
        q
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_against_num_bigint() {
        let mut n = BigUint::from(0u8);
        for i in 0..400u32 {
            let mut text = String::new();
            magnitude_to_decimal(&n, &mut text);
            assert_eq!(text, n.to_string());
            assert_eq!(decimal_to_magnitude(text.as_bytes()), n);
            let long = format!("{text}{text}{text}{text}{text}{text}{text}");
            let want = BigUint::parse_bytes(long.as_bytes(), 10).unwrap_or_default();
            assert_eq!(decimal_to_magnitude(long.as_bytes()), want);
            n = n * 7u8 + BigUint::from(i % 10) + (BigUint::from(10u8).pow(i % 23));
        }
        assert_eq!(
            decimal_to_magnitude(b"1_000_000_000_0"),
            BigUint::from(10_000_000_000u64)
        );
        assert_eq!(decimal_to_magnitude(b"000"), BigUint::from(0u8));
    }

    #[test]
    fn chunk_values() {
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        for i in 0..200_000_u32 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let v = u32::try_from(state % 1_000_000_000).unwrap_or(0);
            let v = if i < 1000 {
                i
            } else if i < 2000 {
                999_999_999 - i
            } else {
                v
            };
            let text = format!("{v:09}");
            assert_eq!(chunk_value(text.as_bytes()), v);
            assert_eq!(chunk_value(&text.as_bytes()[1..]), v % 100_000_000);
        }
    }

    #[test]
    fn every_size_matches_num_bigint() {
        for bits in 0..3000_u32 {
            let p = BigUint::from(1u8) << bits;
            for n in [p.clone() - 1u8, p.clone(), p * 3u8 / 2u8] {
                let mut text = String::new();
                magnitude_to_decimal(&n, &mut text);
                assert_eq!(text, n.to_string(), "{bits}");
                let mut cps: Vec<u32> = Vec::new();
                magnitude_to_decimal(&n, &mut cps);
                assert!(cps
                    .iter()
                    .map(|&c| char::from_u32(c).unwrap())
                    .eq(text.chars()));
            }
        }
        let mut buf = [0_u8; 20];
        for v in [0, 1, 9, 10, 99, 100, 101, 12345, u64::MAX, 10_u64.pow(19)] {
            assert_eq!(u64_to_ascii(v, &mut buf), v.to_string().as_bytes());
        }
    }

    #[test]
    fn scaled_rounding_is_exact() {
        assert_eq!(scaled_round_half_even(2.675, 2), Some(267));
        assert_eq!(scaled_round_half_even(0.125, 2), Some(12));
        assert_eq!(scaled_round_half_even(0.375, 2), Some(38));
        assert_eq!(scaled_round_half_even(0.5, 0), Some(0));
        assert_eq!(scaled_round_half_even(1.5, 0), Some(2));
        assert_eq!(scaled_round_half_even(1e-300, 5), Some(0));
        assert_eq!(scaled_round_half_even(1e37, 2), None);
        assert_eq!(
            scaled_round_half_even(1e30, 0),
            Some(1_000_000_000_000_000_019_884_624_838_656)
        );
        assert_eq!(scaled_round_half_even(1.0, 23), None);
    }
}
