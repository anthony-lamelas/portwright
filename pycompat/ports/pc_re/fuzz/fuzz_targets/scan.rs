//! findall / finditer / split (module and Pattern methods).
#![no_main]
#[path = "common/mod.rs"]
mod common;

use arbitrary::Unstructured;
use libfuzzer_sys::fuzz_target;
use num_bigint::BigInt;
use pc_re_port_core::{api, Value};

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let Ok(p) = common::any_pattern(&mut u) else { return };
    let Ok(s) = common::text(&mut u, 40) else { return };
    let Ok(flags) = common::flags(&mut u) else { return };
    let Ok(n) = (|| -> arbitrary::Result<Value> {
        Ok(if u.ratio(1, 4)? {
            Value::Int(common::int(&mut u)?)
        } else {
            Value::Int(BigInt::from(u.int_in_range(-2i64..=5)?))
        })
    })() else { return };
    let ps = Value::Str(p.clone());
    let ss = Value::Str(s.clone());
    let fk = || ("flags".to_string(), flags.clone());
    let _ = api::findall(&[ps.clone(), ss.clone()], &[fk()]);
    if let Ok(Value::List(ms)) = api::finditer(&[ps.clone(), ss.clone()], &[fk()]) {
        if let Some(m) = ms.last() {
            common::poke_match(&mut u, m);
        }
    }
    let _ = api::split(&[ps.clone(), ss.clone(), n.clone()], &[fk()]);
    let _ = api::split(&[ps.clone(), ss.clone()], &[]);
    common::drain();
    if let Ok(pat) = api::compile(&[ps, flags], &[]) {
        let a = Value::Int(BigInt::from(u.int_in_range(-3i64..=45).unwrap_or(0)));
        let b = if u.arbitrary().unwrap_or(false) {
            Value::None
        } else {
            n.clone()
        };
        let _ = api::pattern_findall(&[pat.clone(), ss.clone(), a.clone(), b.clone()], &[]);
        if let Ok(Value::List(ms)) = api::pattern_finditer(&[pat.clone(), ss.clone(), a, b], &[]) {
            if let Some(m) = ms.first() {
                common::poke_match(&mut u, m);
            }
        }
        let _ = api::pattern_split(&[pat.clone(), ss.clone()], &[("maxsplit".into(), n)]);
        let _ = api::pattern_split(&[pat, ss], &[]);
    }
    common::drain();
});
