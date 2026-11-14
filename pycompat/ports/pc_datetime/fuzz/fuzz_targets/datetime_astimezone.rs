#![no_main]
//! Fuzzes `datetime_astimezone` with structured shim arguments plus near-misses.

use libfuzzer_sys::fuzz_target;
use pc_datetime_fuzz::{run, Kind};

const PARAMS: &[(&str, Kind)] = &[("d", Kind::Datetime), ("utcoffset", Kind::Offset)];

fuzz_target!(|data: &[u8]| run(data, PARAMS, pc_datetime_port_core::timestamp::datetime_astimezone));
