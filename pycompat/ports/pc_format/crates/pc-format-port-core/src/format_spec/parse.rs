//! Port of CPython 3.12 `parse_internal_render_format_spec`.

use crate::numbers::decimal_digit_value;
use crate::value::{PcResult, PyError, PyStr};

/// Which thousands separator a spec asked for (CPython's `LT_*` values).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Thousands {
    /// No separator.
    None,
    /// `,` every 3 digits.
    Comma,
    /// `_` every 3 digits.
    Underscore,
    /// `_` every 4 digits (b, o, x, X).
    UnderscoreFour,
}

/// A parsed format spec. Code points are `u32` because the fill may be a
/// lone surrogate and the type may be any code point (`0` = none given).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatSpec {
    /// Fill code point.
    pub fill: u32,
    /// One of `<`, `>`, `=`, `^`.
    pub align: u8,
    /// `#` given.
    pub alternate: bool,
    /// `z` given.
    pub no_neg_0: bool,
    /// `+`, `-`, ` ` or `0` for none.
    pub sign: u8,
    /// Minimum width.
    pub width: Option<usize>,
    /// Thousands separator.
    pub thousands: Thousands,
    /// Precision.
    pub precision: Option<usize>,
    /// Presentation type code point (`0` when absent and the default is `0`).
    pub kind: u32,
}

fn is_align(c: u32) -> bool {
    matches!(c, 0x3c | 0x3e | 0x3d | 0x5e)
}

fn ascii_byte(c: u32) -> u8 {
    u8::try_from(c).unwrap_or(0)
}

/// `get_integer`: parses Unicode decimal digits starting at `*pos`.
/// Returns `None` when no digit was consumed.
fn get_integer(spec: &[u32], pos: &mut usize) -> PcResult<Option<usize>> {
    let mut acc: usize = 0;
    let mut consumed = false;
    while let Some(&cp) = spec.get(*pos) {
        let Some(d) = char::from_u32(cp).and_then(decimal_digit_value) else {
            break;
        };
        let d = d as usize;
        if acc > (isize::MAX as usize - d) / 10 {
            return Err(PyError::value_error(
                "Too many decimal digits in format string",
            ));
        }
        acc = acc * 10 + d;
        consumed = true;
        *pos += 1;
    }
    Ok(consumed.then_some(acc))
}

/// The `'\x..'`-or-char rendering CPython uses for a presentation type in
/// error messages.
pub(crate) fn type_char_text(kind: u32) -> String {
    match char::from_u32(kind) {
        Some(c) if kind > 32 && kind < 128 => c.to_string(),
        _ => format!("\\x{kind:x}"),
    }
}

/// Parses `spec` for an object of type `type_name`.
///
/// # Errors
/// `ValueError` with CPython's message for every malformed spec.
pub fn parse_spec(
    spec: &PyStr,
    type_name: &str,
    default_type: u32,
    default_align: u8,
) -> PcResult<FormatSpec> {
    parse_spec_code_points(spec.code_points(), type_name, default_type, default_align)
}

/// [`parse_spec`] for a spec given as code points.
///
/// # Errors
/// As [`parse_spec`].
pub fn parse_spec_code_points(
    s: &[u32],
    type_name: &str,
    default_type: u32,
    default_align: u8,
) -> PcResult<FormatSpec> {
    let end = s.len();
    let at = |i: usize| s.get(i).copied().unwrap_or(u32::MAX);
    let mut f = FormatSpec {
        fill: u32::from(b' '),
        align: default_align,
        alternate: false,
        no_neg_0: false,
        sign: 0,
        width: None,
        thousands: Thousands::None,
        precision: None,
        kind: default_type,
    };
    let mut pos = 0;
    let mut fill_specified = false;
    let mut align_specified = false;
    if end >= 2 && is_align(at(1)) {
        f.align = ascii_byte(at(1));
        f.fill = at(0);
        fill_specified = true;
        align_specified = true;
        pos = 2;
    } else if end >= 1 && is_align(at(0)) {
        f.align = ascii_byte(at(0));
        align_specified = true;
        pos = 1;
    }
    if matches!(at(pos), 0x20 | 0x2b | 0x2d) {
        f.sign = ascii_byte(at(pos));
        pos += 1;
    }
    if at(pos) == u32::from(b'z') {
        f.no_neg_0 = true;
        pos += 1;
    }
    if at(pos) == u32::from(b'#') {
        f.alternate = true;
        pos += 1;
    }
    if !fill_specified && at(pos) == u32::from(b'0') {
        f.fill = u32::from(b'0');
        if !align_specified && default_align == b'>' {
            f.align = b'=';
        }
        pos += 1;
    }
    f.width = get_integer(s, &mut pos)?;
    if at(pos) == u32::from(b',') {
        f.thousands = Thousands::Comma;
        pos += 1;
    }
    if at(pos) == u32::from(b'_') {
        if f.thousands != Thousands::None {
            return Err(both_separators());
        }
        f.thousands = Thousands::Underscore;
        pos += 1;
    }
    if at(pos) == u32::from(b',') && f.thousands == Thousands::Underscore {
        return Err(both_separators());
    }
    if at(pos) == u32::from(b'.') {
        pos += 1;
        f.precision = get_integer(s, &mut pos)?;
        if f.precision.is_none() {
            return Err(PyError::value_error("Format specifier missing precision"));
        }
    }
    let rest = end.saturating_sub(pos);
    if rest > 1 {
        let mut msg = PyStr::from("Invalid format specifier '");
        msg.push_py_str(&PyStr::from_code_points(s.to_vec()));
        msg.push_str(&format!("' for object of type '{type_name}'"));
        return Err(PyError::value_error(msg));
    }
    if rest == 1 {
        f.kind = at(pos);
    }
    validate_thousands(&mut f)?;
    Ok(f)
}

/// Checks that the grouping separator is allowed with the presentation type,
/// switching `_` to every-four-digit grouping for `b`/`o`/`x`/`X`.
fn validate_thousands(f: &mut FormatSpec) -> PcResult<()> {
    let allowed = match char::from_u32(f.kind) {
        _ if f.thousands == Thousands::None => true,
        Some('d' | 'e' | 'f' | 'g' | 'E' | 'G' | '%' | 'F' | '\0') => true,
        Some('b' | 'o' | 'x' | 'X') if f.thousands == Thousands::Underscore => {
            f.thousands = Thousands::UnderscoreFour;
            true
        }
        _ => false,
    };
    if allowed {
        return Ok(());
    }
    let sep = if f.thousands == Thousands::Comma {
        ','
    } else {
        '_'
    };
    Err(PyError::value_error(format!(
        "Cannot specify '{sep}' with '{}'.",
        type_char_text(f.kind)
    )))
}

fn both_separators() -> PyError {
    PyError::value_error("Cannot specify both ',' and '_'.")
}
