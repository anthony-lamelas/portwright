#![no_main]
//! Fuzzes `date_fromordinal` with structured shim arguments plus near-misses.

use libfuzzer_sys::fuzz_target;
use pc_datetime_fuzz::{run, Kind};

const PARAMS: &[(&str, Kind)] = &[("n", Kind::Ordinal)];

fuzz_target!(|data: &[u8]| run(data, PARAMS, pc_datetime_port_core::date::date_fromordinal));
