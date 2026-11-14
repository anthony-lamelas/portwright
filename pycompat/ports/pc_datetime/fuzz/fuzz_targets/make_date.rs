#![no_main]
//! Fuzzes `make_date` with structured shim arguments plus near-misses.

use libfuzzer_sys::fuzz_target;
use pc_datetime_fuzz::{run, Kind};

const PARAMS: &[(&str, Kind)] = &[("year", Kind::Year), ("month", Kind::Month), ("day", Kind::Day)];

fuzz_target!(|data: &[u8]| run(data, PARAMS, pc_datetime_port_core::date::make_date));
