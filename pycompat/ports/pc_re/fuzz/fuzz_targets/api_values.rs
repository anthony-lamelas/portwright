//! Every API with arbitrary positional/keyword Value trees: `_check_builtin`,
//! argument binding, `_text`, int coercion, attribute errors.
#![no_main]
#[path = "common/mod.rs"]
mod common;

use arbitrary::Unstructured;
use libfuzzer_sys::fuzz_target;
use pc_re_port_core::{api, pyrepr, Value};

type Api = fn(&[Value], &[(String, Value)]) -> pc_re_port_core::Result<Value>;

const APIS: &[Api] = &[
    api::compile, api::escape, api::search, api::r#match, api::fullmatch, api::findall,
    api::finditer, api::split, api::sub, api::subn, api::pattern_text, api::pattern_flags,
    api::pattern_groups, api::pattern_groupindex, api::pattern_search, api::pattern_match,
    api::pattern_fullmatch, api::pattern_findall, api::pattern_finditer, api::pattern_split,
    api::pattern_sub, api::pattern_subn, api::match_group, api::match_groups,
    api::match_groupdict, api::match_start, api::match_end, api::match_span,
    api::match_lastindex, api::match_lastgroup, api::match_pos, api::match_endpos,
    api::match_string, api::match_expand,
];

const KW: &[&str] = &[
    "pattern", "string", "flags", "flag", "repl", "count", "maxsplit", "pos", "endpos", "p", "m",
    "template", "default", "msg",
];

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let Ok(f) = u.choose(APIS) else { return };
    let nargs = u.int_in_range(0..=5).unwrap_or(0);
    let mut args = Vec::new();
    for i in 0..nargs {
        // Keep the first argument usually a Pattern/Match/str so calls get past binding.
        let v = if i == 0 && u.ratio(2, 3).unwrap_or(false) {
            match u.int_in_range(0..=2).unwrap_or(0) {
                0 => common::compiled(&mut u).ok().flatten(),
                1 => common::a_match(&mut u).ok().flatten(),
                _ => common::pattern(&mut u).ok().map(Value::Str),
            }
            .unwrap_or(Value::None)
        } else if u.ratio(1, 2).unwrap_or(false) {
            Value::Str(common::text(&mut u, 16).unwrap_or_default())
        } else {
            common::value(&mut u, 2).unwrap_or(Value::None)
        };
        args.push(v);
    }
    let nkw = u.int_in_range(0..=2).unwrap_or(0);
    let mut kwargs = Vec::new();
    for _ in 0..nkw {
        let k = u.choose(KW).map(|s| s.to_string()).unwrap_or_default();
        kwargs.push((k, common::value(&mut u, 2).unwrap_or(Value::None)));
    }
    if let Ok(v) = f(&args, &kwargs) {
        let _ = pyrepr::repr(&v);
        let _ = pyrepr::str(&v);
    }
    common::drain();
});
