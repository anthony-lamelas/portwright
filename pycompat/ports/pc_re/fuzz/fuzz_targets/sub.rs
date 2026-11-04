//! sub / subn (module and Pattern methods): template parsing and expansion.
#![no_main]
#[path = "common/mod.rs"]
mod common;

use arbitrary::Unstructured;
use libfuzzer_sys::fuzz_target;
use num_bigint::BigInt;
use pc_re_port_core::{api, PyStr, Value};

fn repl(u: &mut Unstructured<'_>) -> arbitrary::Result<PyStr> {
    const PIECES: &[&str] = &[
        "\\g<0>", "\\g<1>", "\\g<n>", "\\g<m1>", "\\g<", "\\g<>", "\\g<01>", "\\g<1_0>",
        "\\g< 1>", "\\g<+1>", "\\g<\u{663}>", "\\g<99>", "\\1", "\\2", "\\10", "\\01", "\\0",
        "\\177", "\\400", "\\n", "\\t", "\\\\", "\\q", "\\", "\\x41", "\\u00e9", "-", "x",
    ];
    let n = u.int_in_range(0..=6)?;
    let mut out = PyStr::new();
    for _ in 0..n {
        if u.ratio(1, 3)? {
            out.extend(common::text(u, 4)?);
        } else {
            out.extend(u.choose(PIECES)?.chars().map(u32::from));
        }
    }
    Ok(out)
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let Ok(p) = common::any_pattern(&mut u) else { return };
    let Ok(r) = repl(&mut u) else { return };
    let Ok(s) = common::text(&mut u, 40) else { return };
    let Ok(flags) = common::flags(&mut u) else { return };
    let count = if u.ratio(1, 4).unwrap_or(false) {
        Value::Int(common::int(&mut u).unwrap_or_default())
    } else {
        Value::Int(BigInt::from(u.int_in_range(-1i64..=4).unwrap_or(0)))
    };
    let (ps, rs, ss) = (Value::Str(p), Value::Str(r.clone()), Value::Str(s));
    let kw = [("count".to_string(), count.clone()), ("flags".to_string(), flags.clone())];
    let _ = api::sub(&[ps.clone(), rs.clone(), ss.clone()], &kw);
    let _ = api::subn(&[ps.clone(), rs.clone(), ss.clone()], &kw);
    let _ = api::sub(&[ps.clone(), rs.clone(), ss.clone()], &[]);
    common::drain();
    if let Ok(pat) = api::compile(&[ps.clone(), flags], &[]) {
        let _ = api::pattern_sub(&[pat.clone(), rs.clone(), ss.clone(), count.clone()], &[]);
        let _ = api::pattern_subn(&[pat.clone(), rs.clone(), ss.clone()], &[("count".into(), count)]);
        let _ = api::pattern_subn(&[pat, rs.clone(), ss.clone()], &[]);
    }
    if let Ok(m @ Value::Match(_)) = api::search(&[ps, ss], &[]) {
        let _ = api::match_expand(&[m, rs], &[]);
    }
    common::drain();
});
