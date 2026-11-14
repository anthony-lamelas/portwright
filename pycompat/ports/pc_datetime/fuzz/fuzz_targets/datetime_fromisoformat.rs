#![no_main]
//! Fuzzes `datetime_fromisoformat` with structured shim arguments plus near-misses.

use libfuzzer_sys::fuzz_target;
use pc_datetime_fuzz::{run, Kind};

const PARAMS: &[(&str, Kind)] = &[("text", Kind::Isotext)];

fuzz_target!(|data: &[u8]| run(data, PARAMS, pc_datetime_port_core::isoparse::datetime_fromisoformat));
