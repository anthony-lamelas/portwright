#![no_main]
//! Fuzzes `date_add` with structured shim arguments plus near-misses.

use libfuzzer_sys::fuzz_target;
use pc_datetime_fuzz::{run, Kind};

const PARAMS: &[(&str, Kind)] = &[("d", Kind::Date), ("td", Kind::Td)];

fuzz_target!(|data: &[u8]| run(data, PARAMS, pc_datetime_port_core::date::date_add));
