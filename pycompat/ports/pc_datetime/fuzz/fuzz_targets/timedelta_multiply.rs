#![no_main]
//! Fuzzes `timedelta_multiply` with structured shim arguments plus near-misses.

use libfuzzer_sys::fuzz_target;
use pc_datetime_fuzz::{run, Kind};

const PARAMS: &[(&str, Kind)] = &[("td", Kind::Td), ("factor", Kind::Number)];

fuzz_target!(|data: &[u8]| run(data, PARAMS, pc_datetime_port_core::timedelta::timedelta_multiply));
