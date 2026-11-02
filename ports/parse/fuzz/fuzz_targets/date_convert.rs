//! `date_convert(string, match, **kw)` on fuzzed `match.groups()` and
//! keyword indexes (including negative / out-of-range ones).
//! Input: 10 control bytes, then groups separated by `\0` (`\x01` alone = None).
#![no_main]

use libfuzzer_sys::fuzz_target;
use parse_port_core::convert::{date_convert, DateArgs, Index};

fn idx(b: u8) -> Option<Index> {
    // 0xff => None; otherwise a small signed index.
    (b != 0xff).then(|| Ok(i64::from(b as i8 % 12)))
}

fuzz_target!(|data: &[u8]| {
    let [mode, a, b, c, d, e, f, g, h, i, rest @ ..] = data else {
        return;
    };
    let s = String::from_utf8_lossy(rest);
    if s.len() > 1024 {
        return;
    }
    let groups: Vec<Option<String>> = s
        .split('\0')
        .map(|g| (g != "\u{1}").then(|| g.to_owned()))
        .collect();
    let mut args = DateArgs::default();
    match mode % 5 {
        0 => args.ymd = idx(*a),
        1 => args.mdy = idx(*a),
        2 => args.dmy = idx(*a),
        3 => {
            let n = usize::from(*b % 5);
            args.d_m_y = Some(Ok([*a, *c, *d, *e][..n.min(4)].iter().filter_map(|&x| idx(x)).collect()));
        }
        _ => {
            if let (Some(m), Some(dd)) = (idx(*a), idx(*c)) {
                args.mm_dd = Some((m, dd));
            }
        }
    }
    args.hms = idx(*f);
    args.am = idx(*g);
    args.tz = idx(*h);
    if mode & 0x80 != 0 {
        args.ymd = idx(*i);
    }
    let _ = date_convert(&groups, &args);
});
