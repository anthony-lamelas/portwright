#![no_main]

use libfuzzer_sys::fuzz_target;
use pc_unicode_port_core::{call};

#[path = "../common.rs"]
mod common;

fuzz_target!(|data: &[u8]| {
    let mut u = arbitrary::Unstructured::new(data);
    let run = |u: &mut arbitrary::Unstructured| -> arbitrary::Result<()> {
        // Argument binding + built-in guard, as every API runs them first.
        const PARAMS: &[call::Param] = &[
            call::Param::required("s"),
            call::Param::optional("sep"),
            call::Param::optional("maxsplit"),
        ];
        const NAMES: &[&str] = &["s", "sep", "maxsplit", "width", "", "s\u{e9}"];
        let n_params = u.int_in_range(0usize..=PARAMS.len())?;
        let n_args = u.int_in_range(0usize..=5)?;
        let args = (0..n_args)
            .map(|_| common::value(u, 3))
            .collect::<arbitrary::Result<Vec<_>>>()?;
        let n_kwargs = u.int_in_range(0usize..=4)?;
        let mut kwargs = Vec::with_capacity(n_kwargs);
        for _ in 0..n_kwargs {
            let key = if u.ratio(1u8, 4)? {
                u.arbitrary::<String>()?
            } else {
                (*u.choose(NAMES)?).to_string()
            };
            kwargs.push((key, common::value(u, 3)?));
        }
        let _ = call::guard(&args, kwargs.iter().map(|(_, v)| v));
        let _ = call::bind("split", &PARAMS[..n_params], args, kwargs);
        Ok(())
    };
    // Running out of fuzz bytes is not a bug; neither is any returned Python error.
    let _ = run(&mut u);
});
