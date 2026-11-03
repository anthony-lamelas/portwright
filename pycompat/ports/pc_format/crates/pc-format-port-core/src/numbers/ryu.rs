//! Shortest round-trip digits of an `f64` (Ulf Adams' Ryu, "Ryū: fast
//! float-to-string conversion", PLDI 2018), using only `u64`/`u128`
//! arithmetic. The power-of-5 tables are built once with num-bigint.
//!
//! The result is the shortest decimal in the rounding interval of `x`
//! (bounds included when the mantissa is even, as for round-half-even
//! parsing), and among those the one nearest to `x`.

use std::sync::OnceLock;

use num_bigint::BigUint;

const POW5_INV_BITCOUNT: u32 = 125;
const POW5_BITCOUNT: u32 = 125;
const POW5_INV_TABLE_LEN: usize = 342;
const POW5_TABLE_LEN: usize = 326;

struct Tables {
    /// `floor(2**(bits(5**i) - 1 + 125) / 5**i) + 1`, as (low, high) words.
    inv_split: Vec<(u64, u64)>,
    /// `5**i` scaled to exactly 125 bits (truncated), as (low, high) words.
    split: Vec<(u64, u64)>,
}

fn words(v: &BigUint) -> (u64, u64) {
    let digits = v.to_u64_digits();
    (
        digits.first().copied().unwrap_or(0),
        digits.get(1).copied().unwrap_or(0),
    )
}

fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(|| {
        let one = BigUint::from(1_u8);
        let mut pow = one.clone();
        let mut inv_split = Vec::with_capacity(POW5_INV_TABLE_LEN);
        let mut split = Vec::with_capacity(POW5_TABLE_LEN);
        for i in 0..POW5_INV_TABLE_LEN {
            let len = pow.bits();
            let j = len - 1 + u64::from(POW5_INV_BITCOUNT);
            inv_split.push(words(&((&one << j) / &pow + &one)));
            if i < POW5_TABLE_LEN {
                let bits = u64::from(POW5_BITCOUNT);
                let scaled = if len >= bits {
                    &pow >> (len - bits)
                } else {
                    &pow << (bits - len)
                };
                split.push(words(&scaled));
            }
            pow *= 5_u8;
        }
        Tables { inv_split, split }
    })
}

/// `ceil(log2(5**e))` for `0 <= e <= 3528` (plus one, as Ryu defines it).
fn pow5bits(e: i32) -> i32 {
    // e * log2(5) fits easily: e <= 3528.
    i32::try_from((i64::from(e) * 1_217_359) >> 19).unwrap_or(0) + 1
}

/// `floor(log10(2**e))` for `0 <= e <= 1650`.
fn log10_pow2(e: i32) -> u32 {
    u32::try_from((i64::from(e) * 78_913) >> 18).unwrap_or(0)
}

/// `floor(log10(5**e))` for `0 <= e <= 2620`.
fn log10_pow5(e: i32) -> u32 {
    u32::try_from((i64::from(e) * 732_923) >> 20).unwrap_or(0)
}

fn pow5_factor(mut v: u64) -> u32 {
    let mut count = 0;
    while v > 0 && v.is_multiple_of(5) {
        v /= 5;
        count += 1;
    }
    count
}

fn multiple_of_power_of_5(v: u64, p: u32) -> bool {
    pow5_factor(v) >= p
}

fn multiple_of_power_of_2(v: u64, p: u32) -> bool {
    p < 64 && v & ((1_u64 << p) - 1) == 0
}

/// `(m * mul) >> j` for a 125-bit `mul` and `j >= 64`.
fn mul_shift(m: u64, mul: (u64, u64), j: u32) -> u64 {
    let b0 = u128::from(m) * u128::from(mul.0);
    let b2 = u128::from(m) * u128::from(mul.1);
    let sum = (b0 >> 64) + b2;
    let shifted = sum.checked_shr(j.saturating_sub(64)).unwrap_or(0);
    u64::try_from(shifted).unwrap_or(u64::MAX)
}

/// The shortest digits of a finite, nonzero `x >= 0`: `(digits, e10)` with
/// `x ~= digits * 10**e10`. `digits` may end in zeros.
pub(crate) fn shortest(x: f64) -> (u64, i32) {
    const MANTISSA_BITS: u32 = 52;
    const BIAS: i32 = 1023;
    let bits = x.to_bits();
    let ieee_mantissa = bits & ((1_u64 << MANTISSA_BITS) - 1);
    let ieee_exponent = u32::try_from((bits >> MANTISSA_BITS) & 0x7ff).unwrap_or(0);
    let (e2, m2) = if ieee_exponent == 0 {
        (1 - BIAS - 52 - 2, ieee_mantissa)
    } else {
        (
            i32::try_from(ieee_exponent).unwrap_or(0) - BIAS - 52 - 2,
            ieee_mantissa | (1_u64 << MANTISSA_BITS),
        )
    };
    let accept_bounds = m2 & 1 == 0;
    let mv = 4 * m2;
    let mm_shift = u64::from(ieee_mantissa != 0 || ieee_exponent <= 1);
    let t = tables();

    let mut vm_trailing_zeros = false;
    let mut vr_trailing_zeros = false;
    let (e10, mut vr, mut vp, mut vm);
    if e2 >= 0 {
        let q = log10_pow2(e2) - u32::from(e2 > 3);
        e10 = i32::try_from(q).unwrap_or(0);
        let k = i32::try_from(POW5_INV_BITCOUNT).unwrap_or(0) + pow5bits(e10) - 1;
        let i = u32::try_from(-e2 + e10 + k).unwrap_or(64);
        let mul = t
            .inv_split
            .get(usize::try_from(q).unwrap_or(0))
            .copied()
            .unwrap_or_default();
        vr = mul_shift(mv, mul, i);
        vp = mul_shift(mv + 2, mul, i);
        vm = mul_shift(mv - 1 - mm_shift, mul, i);
        if q <= 21 {
            if mv % 5 == 0 {
                vr_trailing_zeros = multiple_of_power_of_5(mv, q);
            } else if accept_bounds {
                vm_trailing_zeros = multiple_of_power_of_5(mv - 1 - mm_shift, q);
            } else {
                vp -= u64::from(multiple_of_power_of_5(mv + 2, q));
            }
        }
    } else {
        let q = log10_pow5(-e2) - u32::from(-e2 > 1);
        let q_i = i32::try_from(q).unwrap_or(0);
        e10 = q_i + e2;
        let i = -e2 - q_i;
        let k = pow5bits(i) - i32::try_from(POW5_BITCOUNT).unwrap_or(0);
        let j = u32::try_from(q_i - k).unwrap_or(64);
        let mul = t
            .split
            .get(usize::try_from(i).unwrap_or(0))
            .copied()
            .unwrap_or_default();
        vr = mul_shift(mv, mul, j);
        vp = mul_shift(mv + 2, mul, j);
        vm = mul_shift(mv - 1 - mm_shift, mul, j);
        if q <= 1 {
            // mv = 4 * m2 has at least two trailing zero bits.
            vr_trailing_zeros = true;
            if accept_bounds {
                vm_trailing_zeros = mm_shift == 1;
            } else {
                vp -= 1;
            }
        } else if q < 63 {
            vr_trailing_zeros = multiple_of_power_of_2(mv, q);
        }
    }

    let mut removed = 0_i32;
    let output = if vm_trailing_zeros || vr_trailing_zeros {
        let mut last_removed = 0_u64;
        while vp / 10 > vm / 10 {
            vm_trailing_zeros &= vm % 10 == 0;
            vr_trailing_zeros &= last_removed == 0;
            last_removed = vr % 10;
            vr /= 10;
            vp /= 10;
            vm /= 10;
            removed += 1;
        }
        if vm_trailing_zeros {
            while vm % 10 == 0 {
                vr_trailing_zeros &= last_removed == 0;
                last_removed = vr % 10;
                vr /= 10;
                // `vp` is no longer needed.
                vm /= 10;
                removed += 1;
            }
        }
        if vr_trailing_zeros && last_removed == 5 && vr % 2 == 0 {
            // An exact tie: round to even.
            last_removed = 4;
        }
        vr + u64::from((vr == vm && (!accept_bounds || !vm_trailing_zeros)) || last_removed >= 5)
    } else {
        let mut round_up = false;
        while vp / 10 > vm / 10 {
            round_up = vr % 10 >= 5;
            vr /= 10;
            vp /= 10;
            vm /= 10;
            removed += 1;
        }
        vr + u64::from(vr == vm || round_up)
    };
    (output, e10 + removed)
}
