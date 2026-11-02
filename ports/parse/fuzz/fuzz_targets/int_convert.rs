//! `int_convert(base)(string, match)`. Input: 8-byte base selector, string.
#![no_main]

use libfuzzer_sys::fuzz_target;
use parse_port_core::convert::int_convert;

fuzz_target!(|data: &[u8]| {
    let Some((&sel, rest)) = data.split_first() else {
        return;
    };
    let (base, rest) = if sel & 1 == 0 {
        (None, rest)
    } else if rest.len() >= 8 {
        let mut b = [0u8; 8];
        b.copy_from_slice(&rest[..8]);
        let raw = i64::from_le_bytes(b);
        // Mostly small bases (incl. 0, 1, negatives), sometimes extreme.
        let base = if sel & 2 == 0 { raw % 40 } else { raw };
        (Some(base), &rest[8..])
    } else {
        (Some(i64::from(sel >> 2) - 8), rest)
    };
    let s = String::from_utf8_lossy(rest);
    if s.len() > 4096 {
        return;
    }
    let _ = int_convert(&s, base);
});
