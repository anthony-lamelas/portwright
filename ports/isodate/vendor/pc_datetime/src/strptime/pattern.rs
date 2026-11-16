//! `_strptime.TimeRE` in the C locale: format string -> regex.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use regex::bytes::{Regex, RegexBuilder};

use super::text::{digit_class, is_space, push_wtf8, space_class};
use crate::error::{PyError, PyResult};
use crate::pystr::PyStr;

/// `TimeRE` for the C locale with TZ=UTC: each directive's pattern, as CPython's text.
fn directive_pattern(key: &str) -> Option<String> {
    const D: &str = r"(?P<d>3[0-1]|[1-2]\d|0[1-9]|[1-9]| [1-9])";
    const H: &str = r"(?P<H>2[0-3]|[0-1]\d|\d)";
    const M: &str = r"(?P<M>[0-5]\d|\d)";
    const S: &str = r"(?P<S>6[0-1]|[0-5]\d|\d)";
    const LOWER_M: &str = r"(?P<m>1[0-2]|0[1-9]|[1-9])";
    const LOWER_Y: &str = r"(?P<y>\d\d)";
    const UPPER_Y: &str = r"(?P<Y>\d\d\d\d)";
    const LOWER_A: &str = r"(?P<a>mon|tue|wed|thu|fri|sat|sun)";
    const LOWER_B: &str = r"(?P<b>jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec)";
    let text = match key {
        "%" => "%".to_owned(),
        "A" => r"(?P<A>wednesday|thursday|saturday|tuesday|monday|friday|sunday)".to_owned(),
        "B" => concat!(
            r"(?P<B>september|february|november|december|january|october|august|march|",
            r"april|june|july|may)"
        )
        .to_owned(),
        "G" => r"(?P<G>\d\d\d\d)".to_owned(),
        "H" => H.to_owned(),
        "I" => r"(?P<I>1[0-2]|0[1-9]|[1-9]| [1-9])".to_owned(),
        "M" => M.to_owned(),
        "S" => S.to_owned(),
        "U" => r"(?P<U>5[0-3]|[0-4]\d|\d)".to_owned(),
        "W" => r"(?P<W>5[0-3]|[0-4]\d|\d)".to_owned(),
        "V" => r"(?P<V>5[0-3]|0[1-9]|[1-4]\d|\d)".to_owned(),
        "Y" => UPPER_Y.to_owned(),
        "Z" => r"(?P<Z>gmt|utc)".to_owned(),
        "a" => LOWER_A.to_owned(),
        "b" => LOWER_B.to_owned(),
        "d" => D.to_owned(),
        "f" => r"(?P<f>[0-9]{1,6})".to_owned(),
        "j" => concat!(
            r"(?P<j>36[0-6]|3[0-5]\d|[1-2]\d\d|0[1-9]\d|00[1-9]|[1-9]\d|0[1-9]|",
            r"[1-9])"
        )
        .to_owned(),
        "m" => LOWER_M.to_owned(),
        "p" => r"(?P<p>am|pm)".to_owned(),
        "u" => r"(?P<u>[1-7])".to_owned(),
        "w" => r"(?P<w>[0-6])".to_owned(),
        "y" => LOWER_Y.to_owned(),
        "z" => r"(?P<z>[+-]\d\d:?[0-5]\d(:?[0-5]\d(\.\d{1,6})?)?|(?-i:Z))".to_owned(),
        "Ow" => r"(?P<w>\d)".to_owned(),
        "Od" | "Om" | "Oy" | "OH" | "OI" | "OM" | "OS" => {
            format!(r"(?P<{}>\d\d|\d| \d)", &key[1..])
        }
        "X" => format!("{H}:{M}:{S}"),
        "x" => format!("{LOWER_M}/{D}/{LOWER_Y}"),
        "c" => format!(r"{LOWER_A}\s+{LOWER_B}\s+{D}\s+{H}:{M}:{S}\s+{UPPER_Y}"),
        _ => return None,
    };
    Some(text)
}

fn cp(c: char) -> u32 {
    u32::from(c)
}

/// `TimeRE.pattern(format)`: the regex text CPython builds, as code points.
///
/// # Errors
/// ValueError `'%s' is a bad directive in format '%s'` for an unknown directive.
fn python_pattern(format: &PyStr) -> PyResult<Vec<u32>> {
    // re_sub(r"([\\.^$*+?\(\){}\[\]|])", r"\\\1", format)
    let mut escaped = Vec::with_capacity(format.len());
    for &c in format.code_points() {
        if "\\.^$*+?(){}[]|".chars().any(|s| cp(s) == c) {
            escaped.push(cp('\\'));
        }
        escaped.push(c);
    }
    // re_sub(r'\s+', r'\\s+', format), then re_sub(r"'", "['ʼ]", format)
    let mut spaced = Vec::with_capacity(escaped.len());
    let mut i = 0;
    while i < escaped.len() {
        let c = escaped[i];
        if is_space(c) {
            while i < escaped.len() && is_space(escaped[i]) {
                i += 1;
            }
            spaced.extend(r"\s+".chars().map(cp));
            continue;
        }
        if c == cp('\'') {
            spaced.extend("['\u{2bc}]".chars().map(cp));
        } else {
            spaced.push(c);
        }
        i += 1;
    }
    // re_sub(r'%(O?.)', repl, format)
    let mut out = Vec::with_capacity(spaced.len());
    let mut i = 0;
    while i < spaced.len() {
        let c = spaced[i];
        if c != cp('%') || i + 1 >= spaced.len() {
            out.push(c);
            i += 1;
            continue;
        }
        let key_len = if spaced[i + 1] == cp('O') && i + 2 < spaced.len() {
            2
        } else {
            1
        };
        let key = &spaced[i + 1..i + 1 + key_len];
        let text = PyStr::from_code_points(key.to_vec())
            .to_str()
            .and_then(|k| directive_pattern(&k));
        let Some(text) = text else {
            let shown: Vec<u32> = if key == [cp('\\')] {
                vec![cp('%')]
            } else {
                key.to_vec()
            };
            let mut message = PyStr::from("'");
            message.push_str(&PyStr::from_code_points(shown));
            message.push_str(&PyStr::from("' is a bad directive in format '"));
            message.push_str(format);
            message.push_str(&PyStr::from("'"));
            return Err(PyError::ValueError(message));
        };
        out.extend(text.chars().map(cp));
        i += 1 + key_len;
    }
    Ok(out)
}

/// A compiled format: the regex and its named groups in group order.
#[derive(Debug)]
pub struct Compiled {
    /// Anchored at the start only, like `re.match`.
    pub regex: Regex,
    /// `(group index, name)` of every named group, in group order.
    pub names: Vec<(usize, String)>,
    /// Length of the translated pattern: a proxy for the compiled size, for the cache.
    weight: usize,
}

/// Appends a literal code point, matched as Python's IGNORECASE would.
fn push_literal(out: &mut String, c: u32) {
    match char::from_u32(c) {
        // Python's IGNORECASE makes i, I, U+0130 and U+0131 equivalent; the regex
        // crate's simple case folding doesn't.
        Some('i' | 'I' | '\u{130}' | '\u{131}') => out.push_str("[iI\u{130}\u{131}]"),
        Some(ch) => out.push_str(&regex::escape(ch.encode_utf8(&mut [0u8; 4]))),
        None => {
            let mut bytes = Vec::new();
            push_wtf8(&mut bytes, c);
            out.push_str("(?-u:");
            for b in bytes {
                out.push_str(&format!("\\x{b:02X}"));
            }
            out.push(')');
        }
    }
}

/// Translates CPython's pattern text to the regex crate, checking group names as
/// `re.compile` does.
fn translate(p: &[u32]) -> PyResult<String> {
    let digits = digit_class();
    let spaces = space_class();
    let mut out = String::from(r"(?i)\A(?:");
    let mut groups = 0usize;
    let mut names: Vec<(Vec<u32>, usize)> = Vec::new();
    let at = |i: usize| p.get(i).copied();
    let copy_until = |out: &mut String, start: usize, end: u32| -> usize {
        let mut i = start;
        while let Some(c) = at(i) {
            out.extend(char::from_u32(c));
            i += 1;
            if c == end {
                break;
            }
        }
        i
    };
    let mut i = 0;
    while let Some(c) = at(i) {
        let ch = char::from_u32(c);
        match ch {
            Some('\\') => {
                match at(i + 1).and_then(char::from_u32) {
                    Some('d') => out.push_str(&digits),
                    Some('s') => out.push_str(&spaces),
                    _ => push_literal(&mut out, at(i + 1).unwrap_or(c)),
                }
                i += 2;
            }
            Some('[') => i = copy_until(&mut out, i, cp(']')),
            Some('{') => i = copy_until(&mut out, i, cp('}')),
            Some('(') if at(i + 1) == Some(cp('?')) => {
                if at(i + 2) == Some(cp('P')) && at(i + 3) == Some(cp('<')) {
                    groups += 1;
                    let name_start = i + 4;
                    let name_end = (name_start..p.len())
                        .find(|&j| p[j] == cp('>'))
                        .unwrap_or(p.len());
                    let name = p[name_start..name_end].to_vec();
                    if let Some((_, first)) = names.iter().find(|(n, _)| *n == name) {
                        return Err(PyError::ReError(PyStr::from(format!(
                            "redefinition of group name {} as group {groups}; was group \
                             {first} at position {}",
                            PyStr::from_code_points(name).repr(),
                            name_start
                        ))));
                    }
                    names.push((name, groups));
                    i = copy_until(&mut out, i, cp('>'));
                } else {
                    i = copy_until(&mut out, i, cp(':'));
                }
            }
            Some('(') => {
                groups += 1;
                out.push('(');
                i += 1;
            }
            Some(sym @ (')' | '|' | '?' | '+' | '*')) => {
                out.push(sym);
                i += 1;
            }
            _ => {
                push_literal(&mut out, c);
                i += 1;
            }
        }
    }
    out.push(')');
    Ok(out)
}

/// Builds the regex for a format.
fn build(format: &PyStr) -> PyResult<Compiled> {
    let pattern = translate(&python_pattern(format)?)?;
    // CPython's `re` has no compiled-size limit, so neither may this: a long literal
    // run in a format must compile (each case-folded literal costs several states).
    let regex = RegexBuilder::new(&pattern)
        .size_limit(usize::MAX)
        .build()
        .map_err(|e| PyError::value_error(format!("internal regex error: {e}")))?;
    let names = regex
        .capture_names()
        .enumerate()
        .filter_map(|(index, name)| Some((index, name?.to_owned())))
        .collect();
    Ok(Compiled {
        regex,
        names,
        weight: pattern.len(),
    })
}

/// Formats kept compiled, as `_strptime._regex_cache` does.
const CACHE_MAX: usize = 64;

/// Total translated-pattern bytes the cache may hold. The regex crate needs a few KiB
/// per format element where CPython's `re` needs bytes, so caching every long format
/// (as `_regex_cache` does) would exhaust memory. Caching never changes results.
const CACHE_WEIGHT_MAX: usize = 1 << 18;

/// The cached regexes and their total weight.
#[derive(Default)]
struct Cache {
    entries: HashMap<PyStr, Rc<Compiled>>,
    weight: usize,
}

thread_local! {
    static CACHE: RefCell<Cache> = RefCell::new(Cache::default());
}

/// The compiled regex for a format, cached.
///
/// # Errors
/// The bad-directive ValueError, or the `re.error` for a repeated group name.
pub fn compile(format: &PyStr) -> PyResult<Rc<Compiled>> {
    if let Some(hit) = CACHE.with(|c| c.borrow().entries.get(format).cloned()) {
        return Ok(hit);
    }
    let compiled = Rc::new(build(format)?);
    if compiled.weight <= CACHE_WEIGHT_MAX {
        CACHE.with(|c| {
            let mut cache = c.borrow_mut();
            if cache.entries.len() >= CACHE_MAX
                || cache.weight.saturating_add(compiled.weight) > CACHE_WEIGHT_MAX
            {
                cache.entries.clear();
                cache.weight = 0;
            }
            cache.weight = cache.weight.saturating_add(compiled.weight);
            cache.entries.insert(format.clone(), Rc::clone(&compiled));
        });
    }
    Ok(compiled)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn regex_for(pattern: &str) -> regex::Regex {
        let cps: Vec<u32> = pattern.chars().map(u32::from).collect();
        let translated = translate(&cps).expect("pattern translates");
        regex::Regex::new(&translated).expect("translation compiles")
    }

    #[test]
    fn translation_keeps_python_classes_and_anchoring() {
        let re = regex_for(r"(?P<d>3[01]|[12]\d|0[1-9]|[1-9]| [1-9])\s(?P<b>jan|feb)");
        let caps = re
            .captures("31\u{2003}FEB")
            .expect("unicode space, ignore-case");
        assert_eq!(&caps["d"], "31");
        assert_eq!(&caps["b"], "FEB");
        assert!(re.is_match("1\u{0663} jan"), "\\d is Unicode");
        assert!(!re.is_match("x31 jan"), "anchored at the start");
        assert!(regex_for(r"a\.b").is_match("a.b"));
        assert!(!regex_for(r"a\.b").is_match("axb"));
    }

    #[test]
    fn repeated_group_name_is_re_error() {
        let cps: Vec<u32> = "(?P<d>1)-(?P<d>2)".chars().map(u32::from).collect();
        let err = translate(&cps).expect_err("duplicate name");
        assert_eq!(
            err.to_string(),
            "error: redefinition of group name 'd' as group 2; was group 1 at position 13"
        );
    }
}
