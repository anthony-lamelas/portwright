#![no_main]
//! Fuzzes `strptime` with structured shim arguments plus near-misses.

use libfuzzer_sys::fuzz_target;
use pc_datetime_fuzz::{run, Kind};

const PARAMS: &[(&str, Kind)] = &[("text", Kind::StrptimeText), ("fmt", Kind::Format)];

fuzz_target!(|data: &[u8]| run(data, PARAMS, pc_datetime_port_core::strptime::strptime));
