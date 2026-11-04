//! Python `str` values as code points.
//!
//! Python strings may hold lone surrogates (`"x\ud800y"`), which Rust `String`
//! can't. Every pattern, subject, template, group name and `re.error` message
//! is therefore a [`PyStr`]: one `u32` per Python code point, so lengths,
//! positions and slices match Python's indices exactly.

/// A Python `str`: one element per code point (`0..=0x10FFFF`, surrogates allowed).
pub type PyStr = Vec<u32>;

/// Code points of a Rust string (never contains surrogates).
pub fn cps(s: &str) -> PyStr {
    s.chars().map(u32::from).collect()
}

/// Appends the code points of `s` to `out`.
pub fn push_str(out: &mut PyStr, s: &str) {
    out.extend(s.chars().map(u32::from));
}

/// A Rust string for code points known to be valid scalar values; lone
/// surrogates (and anything else invalid) become U+FFFD. For diagnostics and
/// ASCII-only text; never use it on data that is returned to Python.
pub fn to_string_lossy(s: &[u32]) -> String {
    s.iter()
        .map(|&c| char::from_u32(c).unwrap_or('\u{FFFD}'))
        .collect()
}

/// `s` as a Rust string, or `None` if it holds a lone surrogate.
pub fn to_string(s: &[u32]) -> Option<String> {
    s.iter().map(|&c| char::from_u32(c)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let v = cps("aé😀");
        assert_eq!(v, vec![0x61, 0xE9, 0x1F600]);
        assert_eq!(to_string(&v).as_deref(), Some("aé😀"));
        assert_eq!(to_string(&[0xD800]), None);
        assert_eq!(to_string_lossy(&[0x61, 0xD800]), "a\u{FFFD}");
        let mut out = cps("a");
        push_str(&mut out, "b");
        assert_eq!(out, cps("ab"));
    }
}
