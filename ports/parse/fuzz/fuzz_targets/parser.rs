//! `Parser::new(format, extra_types, case_sensitive)`, then compile the
//! search/match regexes and run them over text, applying the conversions
//! the parser chose (as `parse.parse` / `parse.search` would).
//! Input: flag byte, then `format \0 text \0 extra_name \0 extra_pattern`.
#![no_main]

use indexmap::IndexMap;
use libfuzzer_sys::fuzz_target;
use parse_port_core::convert::{date_convert, int_convert, percentage, DateArgs};
use parse_port_core::format::{Conversion, ExtraType, GroupKey, Parser};
use parse_port_core::pyre::Regex;
use parse_port_core::pystr;
use parse_port_core::strptime::strf_date_convert;

// Upstream itself backtracks exponentially on adjacent lazy fields
// (`{}{}...`, `{}{,}` -> `(.+?){,}`), so keep inputs small enough that only real hangs time out.
const MAX_FORMAT_CHARS: usize = 64;
const MAX_FIELDS: usize = 4;
const MAX_TEXT_CHARS: usize = 16;

fn apply(parser: &Parser, re: &Regex, text: &[char]) {
    let Some(caps) = re.search(text, 0, text.len()) else {
        return;
    };
    let groups: Vec<Option<String>> = (1..caps.len()).map(|i| caps.text(text, i)).collect();
    for (key, conv) in parser.conversions() {
        let idx = match key {
            GroupKey::Index(i) => usize::try_from(*i).ok(),
            GroupKey::Name(n) => re.groupindex().get(n).copied(),
        };
        let Some(s) = idx.and_then(|i| caps.text(text, i)) else {
            continue;
        };
        match conv {
            Conversion::Int(base) => drop(int_convert(&s, *base)),
            Conversion::Percentage => drop(percentage(&s)),
            Conversion::Float => drop(pystr::py_float(&s)),
            Conversion::Decimal | Conversion::Custom(_) => {}
            Conversion::Strf(t) => drop(strf_date_convert(&s, t)),
            Conversion::Date(kw) => {
                let args = DateArgs {
                    mm_dd: match (kw.mm, kw.dd) {
                        (Some(m), Some(d)) if m != 0 && d != 0 => Some((Ok(m), Ok(d))),
                        _ => None,
                    },
                    ymd: kw.ymd.map(Ok),
                    mdy: kw.mdy.map(Ok),
                    dmy: kw.dmy.map(Ok),
                    d_m_y: kw.d_m_y.map(|a| Ok(a.iter().copied().map(Ok).collect())),
                    hms: kw.hms.map(Ok),
                    am: kw.am.map(Ok),
                    tz: kw.tz.map(Ok),
                };
                drop(date_convert(&groups, &args));
            }
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&flags, rest)) = data.split_first() else {
        return;
    };
    let s = String::from_utf8_lossy(rest);
    let mut parts = s.split('\0');
    let format = parts.next().unwrap_or("");
    let text = parts.next().unwrap_or("");
    let extra_name = parts.next();
    let extra_pattern = parts.next().unwrap_or(".+?");
    if format.chars().count() > MAX_FORMAT_CHARS
        || format.matches('{').count() > 2 * MAX_FIELDS
        || text.chars().count() > MAX_TEXT_CHARS
        || extra_pattern.chars().count() > 32
    {
        return;
    }
    let mut extra: IndexMap<String, ExtraType> = IndexMap::new();
    if let Some(name) = extra_name {
        extra.insert(
            name.to_owned(),
            ExtraType {
                pattern: extra_pattern.to_owned(),
                group_count: i64::from(flags >> 4) - 2,
            },
        );
    }
    let Ok(parser) = Parser::new(format, &mut extra, flags & 1 == 0) else {
        return;
    };
    let _ = (parser.format(), parser.expression(), parser.fixed_fields(), parser.named_fields());
    for n in parser.named_fields() {
        let _ = parser.group_name(n);
    }
    let chars: Vec<char> = text.chars().collect();
    if let Ok(re) = parser.search_regex() {
        apply(&parser, &re, &chars);
    }
    if let Ok(re) = parser.match_regex() {
        apply(&parser, &re, &chars);
    }
});
