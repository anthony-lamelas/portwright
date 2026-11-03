# Fuzz targets for pc-format-port-core

One target per public API (all 13 take untrusted `Value`s): float_repr, int_str,
int_to_float, parse_int, parse_float, floor_div, div_mod, true_div, round_number,
format_value, percent_format, modulo, str_format. `common/mod.rs` builds
binding-shaped `Value`s from fuzz bytes (lone surrogates, Unicode digits/spaces,
format/percent syntax, huge ints past f64 range and the 4300-digit limit,
NaN/-0.0/subnormals, nested containers, `Foreign`). Python errors are ignored;
only panics/aborts/timeouts/OOM fail.

Build layout: this dir must sit next to `crates/` inside the port workspace
(core inherits `workspace.package`), e.g. `<port>/fuzz`, then
`cargo +nightly fuzz build --fuzz-dir <port>/fuzz` (set CARGO_TARGET_DIR if the
default is unwritable).

Found on first 20s run: `round_number(int, ndigits)` with a huge negative
ndigits panics ("memory overflow" in num-bigint pow, arith.rs `round_int`)
instead of returning MemoryError.
