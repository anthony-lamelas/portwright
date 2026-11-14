#![no_main]
//! Fuzzes `timezone_name` with structured shim arguments plus near-misses.

use libfuzzer_sys::fuzz_target;
use pc_datetime_fuzz::{run, Kind};

const PARAMS: &[(&str, Kind)] = &[("utcoffset", Kind::Offset)];

fuzz_target!(|data: &[u8]| run(data, PARAMS, pc_datetime_port_core::timedelta::timezone_name));
