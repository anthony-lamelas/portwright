//! `extract_format`: pull apart `[[fill]align][sign][0][width][grouping][.precision][type]`.

use crate::error::Error;
use crate::pyre::dt_format::DT_FORMAT_TO_REGEX;
use crate::pyre::unicode15 as pystr;

/// Upstream `ALLOWED_TYPES`.
pub const ALLOWED_TYPES: &[&str] = &[
    "n", "b", "o", "x", "X", "%", "f", "F", "e", "E", "g", "G", "w", "W", "d", "D", "s", "S", "l",
    "ti", "te", "ta", "th", "tg", "tc", "tt", "ts",
];

/// The parts of a format spec, as upstream's `locals()` holds them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FormatSpec {
    /// `fill` (`None` unless an alignment follows it).
    pub fill: Option<char>,
    /// `align`: one of `<>=^`.
    pub align: Option<char>,
    /// `zero`: a `0` before the width.
    pub zero: bool,
    /// `width`, as the digits seen (may be empty).
    pub width: String,
    /// `grouping`, only when set (`,` or `_`).
    pub grouping: Option<char>,
    /// `precision`, only when a `.` was seen (digits may be empty).
    pub precision: Option<String>,
    /// The remaining type (also upstream's final `format` local).
    pub type_: String,
}

/// `any(k in type for k in dt_format_to_regex)`.
pub fn has_datetime_directive(type_: &str) -> bool {
    DT_FORMAT_TO_REGEX.iter().any(|(k, _)| type_.contains(k))
}

/// Upstream `extract_format(format, extra_types)`. `is_extra` answers
/// `type in extra_types`; it is consulted only when upstream would.
///
/// # Errors
/// `IndexError` on an empty spec, `ValueError` for an unknown type, or
/// whatever `is_extra` reports.
pub fn extract_format<E: From<Error>>(
    spec: &str,
    mut is_extra: impl FnMut(&str) -> Result<bool, E>,
) -> Result<FormatSpec, E> {
    let chars: Vec<char> = spec.chars().collect();
    let Some(&first) = chars.first() else {
        return Err(Error::Index("string index out of range".into()).into());
    };
    let is_align = |c: char| matches!(c, '<' | '>' | '=' | '^');
    let mut out = FormatSpec::default();
    let mut rest: &[char] = &chars;
    if is_align(first) {
        out.align = Some(first);
        rest = &chars[1..];
    } else if let Some(&second) = chars.get(1).filter(|&&c| is_align(c)) {
        out.fill = Some(first);
        out.align = Some(second);
        rest = &chars[2..];
    }
    if let Some(('+' | '-' | ' ', tail)) = rest.split_first() {
        rest = tail;
    }
    if let Some(('0', tail)) = rest.split_first() {
        out.zero = true;
        rest = tail;
    }
    while let Some((&c, tail)) = rest.split_first() {
        if !pystr::is_digit(c) {
            break;
        }
        out.width.push(c);
        rest = tail;
    }
    if let Some((&g @ (',' | '_'), tail)) = rest.split_first() {
        out.grouping = Some(g);
        rest = tail;
    }
    if let Some(('.', tail)) = rest.split_first() {
        rest = tail;
        let mut precision = String::new();
        while let Some((&c, tail)) = rest.split_first() {
            if !pystr::is_digit(c) {
                break;
            }
            precision.push(c);
            rest = tail;
        }
        out.precision = Some(precision);
    }
    out.type_ = rest.iter().collect();
    let t = out.type_.as_str();
    if !t.is_empty() && !ALLOWED_TYPES.contains(&t) && !is_extra(t)? && !has_datetime_directive(t) {
        return Err(Error::Value(format!("format spec {} not recognised", pystr::repr(t))).into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ex(spec: &str) -> Result<FormatSpec, Error> {
        extract_format(spec, |_| Ok::<bool, Error>(false))
    }

    #[test]
    fn splits_spec() -> Result<(), Error> {
        let f = ex("x>10,.3f")?;
        assert_eq!(f.fill, Some('x'));
        assert_eq!(f.align, Some('>'));
        assert_eq!(f.width, "10");
        assert_eq!(f.grouping, Some(','));
        assert_eq!(f.precision.as_deref(), Some("3"));
        assert_eq!(f.type_, "f");
        assert!(ex("q").is_err());
        assert!(ex("").is_err());
        assert_eq!(ex("abc%dxyz")?.type_, "abc%dxyz");
        Ok(())
    }
}
