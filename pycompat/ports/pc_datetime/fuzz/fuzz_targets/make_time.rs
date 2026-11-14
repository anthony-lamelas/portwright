#![no_main]
//! Fuzzes `make_time` with structured shim arguments plus near-misses.

use libfuzzer_sys::fuzz_target;
use pc_datetime_fuzz::{run, Kind};

const PARAMS: &[(&str, Kind)] = &[("hour", Kind::Hour), ("minute", Kind::Minsec), ("second", Kind::Minsec), ("microsecond", Kind::Us), ("utcoffset", Kind::Offset), ("fold", Kind::Fold)];

fuzz_target!(|data: &[u8]| run(data, PARAMS, pc_datetime_port_core::datetime::make_time));
