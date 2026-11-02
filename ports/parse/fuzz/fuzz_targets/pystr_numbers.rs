//! `int(s, base)` / `float(s)` / `repr(s)` helpers that every converter
//! funnels untrusted text through. Input: base byte, text.
#![no_main]

use libfuzzer_sys::fuzz_target;
use parse_port_core::pystr;

fuzz_target!(|data: &[u8]| {
    let Some((&b, rest)) = data.split_first() else {
        return;
    };
    let s = String::from_utf8_lossy(rest);
    if s.len() > 4096 {
        return;
    }
    let _ = pystr::py_int(&s, u32::from(b % 40));
    let _ = pystr::py_float(&s);
    let _ = pystr::repr(&s);
    let _ = pystr::capitalize(&s);
    let _ = pystr::is_identifier(&s);
});
