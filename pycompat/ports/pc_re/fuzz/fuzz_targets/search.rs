//! search / match / fullmatch (module and Pattern methods), then every Match accessor.
#![no_main]
#[path = "common/mod.rs"]
mod common;

use arbitrary::Unstructured;
use libfuzzer_sys::fuzz_target;
use num_bigint::BigInt;
use pc_re_port_core::{api, Value};

type Api = fn(&[Value], &[(String, Value)]) -> pc_re_port_core::Result<Value>;

fn pos(u: &mut Unstructured<'_>) -> arbitrary::Result<Value> {
    Ok(match u.int_in_range(0..=5)? {
        0 => Value::None,
        1 => Value::Int(common::int(u)?),
        _ => Value::Int(BigInt::from(u.int_in_range(-5i64..=40)?)),
    })
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let Ok(p) = common::any_pattern(&mut u) else { return };
    let Ok(s) = common::text(&mut u, 40) else { return };
    let Ok(flags) = common::flags(&mut u) else { return };
    let Ok(which) = u.int_in_range(0..=2u8) else { return };
    let (module, method): (Api, Api) = match which {
        0 => (api::search, api::pattern_search),
        1 => (api::r#match, api::pattern_match),
        _ => (api::fullmatch, api::pattern_fullmatch),
    };
    let args = [Value::Str(p.clone()), Value::Str(s.clone())];
    if let Ok(m) = module(&args, &[("flags".to_string(), flags.clone())]) {
        if matches!(m, Value::Match(_)) {
            common::poke_match(&mut u, &m);
        }
    }
    common::drain();
    if let Ok(pat) = api::compile(&[Value::Str(p), flags], &[]) {
        let (Ok(a), Ok(b)) = (pos(&mut u), pos(&mut u)) else { return };
        let r = match u.int_in_range(0..=2u8).unwrap_or(0) {
            0 => method(&[pat, Value::Str(s)], &[]),
            1 => method(&[pat, Value::Str(s), a], &[]),
            _ => method(&[pat, Value::Str(s)], &[("pos".into(), a), ("endpos".into(), b)]),
        };
        if let Ok(m) = r {
            if matches!(m, Value::Match(_)) {
                common::poke_match(&mut u, &m);
            }
        }
    }
    common::drain();
});
