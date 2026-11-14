#![no_main]
//! Fuzzes `make_timedelta` with structured shim arguments plus near-misses.

use libfuzzer_sys::fuzz_target;
use pc_datetime_fuzz::{run, Kind};

const PARAMS: &[(&str, Kind)] = &[("days", Kind::Number), ("seconds", Kind::Number), ("microseconds", Kind::Number), ("milliseconds", Kind::Number), ("minutes", Kind::Number), ("hours", Kind::Number), ("weeks", Kind::Number)];

fuzz_target!(|data: &[u8]| run(data, PARAMS, pc_datetime_port_core::timedelta::make_timedelta));
