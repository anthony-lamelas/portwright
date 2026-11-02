//! The sre-compatible engine: compile a user pattern (`with_pattern`) and
//! run search/match/fullmatch with fuzzed pos/endpos. Text is capped: sre
//! backtracking is exponential on nested quantifiers (upstream too).
//! Input: flags byte, pos byte, endpos byte, then `pattern \0 text`.
#![no_main]

use libfuzzer_sys::fuzz_target;
use parse_port_core::pyre::{clamp_pos, Flags, Regex};

fuzz_target!(|data: &[u8]| {
    let [flags, pos, endpos, rest @ ..] = data else {
        return;
    };
    let s = String::from_utf8_lossy(rest);
    let (pattern, text) = s.split_once('\0').unwrap_or((&s, ""));
    if pattern.chars().count() > 64 || text.chars().count() > 16 {
        return;
    }
    let f = Flags { ignorecase: flags & 1 != 0, dotall: flags & 2 != 0 };
    match Regex::new(pattern, f) {
        Ok(re) => {
            let _ = (re.pattern(), re.flags_value(), re.groups(), re.groupindex());
            let chars: Vec<char> = text.chars().collect();
            let (p, e) = clamp_pos(isize::from(*pos as i8), isize::from(*endpos as i8), chars.len());
            for caps in [
                re.search(&chars, p, e),
                re.match_at(&chars, p, e),
                re.fullmatch(&chars, p, e),
                re.search_from(&chars, p, e, true),
            ]
            .into_iter()
            .flatten()
            {
                for i in 0..=caps.len() {
                    let _ = caps.span(i);
                    let _ = caps.text(&chars, i);
                }
            }
        }
        Err(err) => {
            let _ = (err.message(), err.lineno(), err.colno(), err.to_string());
        }
    }
    let _ = Regex::compile(pattern, f);
});
