#![no_main]
//! Fuzzes `timedelta_divide` with structured shim arguments plus near-misses.

use libfuzzer_sys::fuzz_target;
use pc_datetime_fuzz::{run, Kind};

const PARAMS: &[(&str, Kind)] = &[("td", Kind::Td), ("divisor", Kind::Divisor)];

fuzz_target!(|data: &[u8]| run(data, PARAMS, pc_datetime_port_core::timedelta::timedelta_divide));
