#![no_main]
//! Fuzzes `datetime_isoformat` with structured shim arguments plus near-misses.

use libfuzzer_sys::fuzz_target;
use pc_datetime_fuzz::{run, Kind};

const PARAMS: &[(&str, Kind)] = &[("d", Kind::Datetime), ("sep", Kind::Sep), ("timespec", Kind::Timespec)];

fuzz_target!(|data: &[u8]| run(data, PARAMS, pc_datetime_port_core::datetime::datetime_isoformat));
