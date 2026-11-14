#![no_main]
//! Fuzzes `datetime_fromtimestamp` with structured shim arguments plus near-misses.

use libfuzzer_sys::fuzz_target;
use pc_datetime_fuzz::{run, Kind};

const PARAMS: &[(&str, Kind)] = &[("timestamp", Kind::Timestamp), ("utcoffset", Kind::Offset)];

fuzz_target!(|data: &[u8]| run(data, PARAMS, pc_datetime_port_core::timestamp::datetime_fromtimestamp));
