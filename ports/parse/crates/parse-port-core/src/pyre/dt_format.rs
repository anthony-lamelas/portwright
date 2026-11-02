//! `dt_format_to_regex` and `get_regex_for_datetime_format` from upstream.

/// Upstream `dt_format_to_regex`, in dict (insertion) order.
pub const DT_FORMAT_TO_REGEX: &[(&str, &str)] = &[
    ("%a", "(?:Sun|Mon|Tue|Wed|Thu|Fri|Sat)"),
    (
        "%A",
        "(?:Sunday|Monday|Tuesday|Wednesday|Thursday|Friday|Saturday)",
    ),
    ("%w", "[0-6]"),
    ("%d", "[0-9]{1,2}"),
    ("%b", "(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)"),
    (
        "%B",
        "(?:January|February|March|April|May|June|July|August|September|October|November|December)",
    ),
    ("%m", "[0-9]{1,2}"),
    ("%y", "[0-9]{2}"),
    ("%Y", "[0-9]{4}"),
    ("%H", "[0-9]{1,2}"),
    ("%I", "[0-9]{1,2}"),
    ("%p", "(?:AM|PM)"),
    ("%M", "[0-9]{2}"),
    ("%S", "[0-9]{2}"),
    ("%f", "[0-9]{1,6}"),
    ("%z", "(?:[+-][0-9]{2}:?[0-9]{2}(?::?[0-9]{2})?|(?-i:Z))"),
    ("%j", "[0-9]{1,3}"),
    ("%U", "[0-9]{1,2}"),
    ("%W", "[0-9]{1,2}"),
];

/// Regex for a directive such as `"%Y"`, if it is a key of the table.
pub fn directive_regex(directive: &str) -> Option<&'static str> {
    DT_FORMAT_TO_REGEX
        .iter()
        .find(|(k, _)| *k == directive)
        .map(|(_, v)| *v)
}

/// Same as [`get_regex_for_datetime_format`] on raw code points, so callers
/// can pass Python strings that hold lone surrogates.
pub fn get_regex_for_datetime_format_codepoints(format: &[u32]) -> Vec<u32> {
    let mut out = Vec::with_capacity(format.len());
    let mut i = 0;
    while let Some(&c) = format.get(i) {
        if c == u32::from(b'%') {
            let key = format
                .get(i + 1)
                .and_then(|&n| char::from_u32(n))
                .map(|n| format!("%{n}"));
            if let Some(rx) = key.as_deref().and_then(directive_regex) {
                out.extend(rx.chars().map(u32::from));
                i += 2;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// `get_regex_for_datetime_format(format_)`: replace every directive with its
/// regex, scanning left to right without overlap (like `re.sub` with an
/// alternation of the keys, which are all two characters long).
pub fn get_regex_for_datetime_format(format: &str) -> String {
    let chars: Vec<char> = format.chars().collect();
    let mut out = String::with_capacity(format.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '%' {
            if let Some(&next) = chars.get(i + 1) {
                let key: String = ['%', next].iter().collect();
                if let Some(rx) = directive_regex(&key) {
                    out.push_str(rx);
                    i += 2;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitutes_leftmost() {
        assert_eq!(get_regex_for_datetime_format("%%d"), "%[0-9]{1,2}");
        assert_eq!(get_regex_for_datetime_format("%Z %"), "%Z %");
        assert_eq!(
            get_regex_for_datetime_format("%Y-%m"),
            "[0-9]{4}-[0-9]{1,2}"
        );
    }
}
