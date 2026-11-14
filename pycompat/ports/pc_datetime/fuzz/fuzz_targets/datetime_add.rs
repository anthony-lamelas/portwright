#![no_main]
//! Fuzzes `datetime_add` with structured shim arguments plus near-misses.

use libfuzzer_sys::fuzz_target;
use pc_datetime_fuzz::{run, Kind};

const PARAMS: &[(&str, Kind)] = &[("d", Kind::Datetime), ("td", Kind::Td)];

fuzz_target!(|data: &[u8]| run(data, PARAMS, pc_datetime_port_core::datetime::datetime_add));
