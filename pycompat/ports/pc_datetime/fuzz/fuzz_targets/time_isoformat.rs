#![no_main]
//! Fuzzes `time_isoformat` with structured shim arguments plus near-misses.

use libfuzzer_sys::fuzz_target;
use pc_datetime_fuzz::{run, Kind};

const PARAMS: &[(&str, Kind)] = &[("t", Kind::Time), ("timespec", Kind::Timespec)];

fuzz_target!(|data: &[u8]| run(data, PARAMS, pc_datetime_port_core::datetime::time_isoformat));
