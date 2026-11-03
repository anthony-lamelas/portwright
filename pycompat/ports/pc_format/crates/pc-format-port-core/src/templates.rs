//! `str.format`: template parsing, field lookup, conversions and nested specs,
//! ported from CPython 3.12's `Objects/stringlib/unicode_format.h`.

mod attrs;
mod object;

use std::borrow::Cow;
use std::ops::Range;

use num_traits::Zero;

use crate::numbers::decimal_digit_value;
use crate::value::{check_builtins, PcResult, PyError, PyStr, Value};

use object::{kwarg_lookup, Key, Obj};

/// `build_string`'s starting recursion budget: a spec may nest one level.
const MAX_RECURSION: u32 = 2;

const LBRACE: u32 = '{' as u32;
const RBRACE: u32 = '}' as u32;

/// `pc_format.str_format(template, args, kwargs)`:
/// `template.format(*args, **(kwargs or {}))`.
///
/// # Errors
/// As the Python built-in it wraps.
pub fn str_format_api(template: &Value, args: &Value, kwargs: &Value) -> PcResult<Value> {
    check_builtins(&[template, args, kwargs])?;
    let Value::Str(template) = template else {
        return Err(PyError::attribute_error(format!(
            "'{}' object has no attribute 'format'",
            template.type_name()
        )));
    };
    refuse_attribute_fields(template.code_points(), 0)?;
    // `**` is merged (and checked) before `*` is turned into a tuple.
    let kwargs: &[(Value, Value)] = match kwargs {
        Value::Dict(pairs) => pairs,
        other if !is_truthy(other) => &[],
        other => {
            return Err(PyError::type_error(format!(
                "str.format() argument after ** must be a mapping, not {}",
                other.type_name()
            )))
        }
    };
    let args = iterate(args)?;
    let mut ctx = Context {
        args: &args,
        kwargs,
        auto: AutoNumber::default(),
    };
    ctx.build_string(template.code_points(), MAX_RECURSION)
        .map(Value::Str)
}

fn is_truthy(v: &Value) -> bool {
    match v {
        Value::None => false,
        Value::Bool(b) => *b,
        Value::Int(n) => !n.is_zero(),
        Value::Float(f) => *f != 0.0,
        Value::Str(s) => !s.is_empty(),
        Value::Bytes(b) | Value::ByteArray(b) => !b.is_empty(),
        Value::Tuple(items) | Value::List(items) | Value::Set(items) | Value::FrozenSet(items) => {
            !items.is_empty()
        }
        Value::Dict(pairs) => !pairs.is_empty(),
        Value::Foreign(_) => true,
    }
}

/// `tuple(args)` for the `*args` of a call.
fn iterate(args: &Value) -> PcResult<Cow<'_, [Value]>> {
    let owned: Vec<Value> = match args {
        Value::Tuple(items) | Value::List(items) | Value::Set(items) | Value::FrozenSet(items) => {
            return Ok(Cow::Borrowed(items))
        }
        Value::Str(s) => s
            .code_points()
            .iter()
            .map(|&cp| Value::Str(PyStr::from_code_points(vec![cp])))
            .collect(),
        Value::Bytes(b) | Value::ByteArray(b) => b.iter().map(|&x| Value::int(x)).collect(),
        Value::Dict(pairs) => pairs.iter().map(|(k, _)| k.clone()).collect(),
        other => {
            return Err(PyError::type_error(format!(
                "str.format() argument after * must be an iterable, not {}",
                other.type_name()
            )))
        }
    };
    Ok(Cow::Owned(owned))
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum AutoState {
    #[default]
    Init,
    Auto,
    Manual,
}

#[derive(Debug, Default)]
struct AutoNumber {
    state: AutoState,
    next: usize,
}

/// One parsed replacement field (ranges index the template being built).
struct Field {
    name: Range<usize>,
    /// The conversion character; 0 means none (as in C, `!\0` is none too).
    conversion: u32,
    spec: Range<usize>,
    spec_needs_expanding: bool,
}

fn value_error(msg: &str) -> PyError {
    PyError::value_error(msg)
}

/// CPython's `get_integer`: `Ok(None)` unless every char is a decimal digit.
fn get_integer(s: &[u32]) -> PcResult<Option<usize>> {
    if s.is_empty() {
        return Ok(None);
    }
    let mut acc: usize = 0;
    for &cp in s {
        let Some(digit) = char::from_u32(cp).and_then(decimal_digit_value) else {
            return Ok(None);
        };
        // Py_ssize_t limit.
        acc = acc
            .checked_mul(10)
            .and_then(|a| a.checked_add(digit as usize))
            .filter(|&a| i64::try_from(a).is_ok())
            .ok_or_else(|| value_error("Too many decimal digits in format string"))?;
    }
    Ok(Some(acc))
}

/// CPython's `parse_field`, starting just after the opening `{`.
fn parse_field(s: &[u32], pos: &mut usize) -> PcResult<Field> {
    let name_start = *pos;
    let mut c = 0;
    while let Some(&ch) = s.get(*pos) {
        *pos += 1;
        c = ch;
        match char::from_u32(ch) {
            Some('{') => return Err(value_error("unexpected '{' in field name")),
            Some('[') => {
                while s.get(*pos).is_some_and(|&x| x != u32::from(']')) {
                    *pos += 1;
                }
            }
            Some('}' | ':' | '!') => break,
            _ => {}
        }
    }
    let name = name_start..pos.saturating_sub(1).max(name_start);
    let mut field = Field {
        name,
        conversion: 0,
        spec: 0..0,
        spec_needs_expanding: false,
    };
    if c == u32::from('!') || c == u32::from(':') {
        if c == u32::from('!') {
            let Some(&conv) = s.get(*pos) else {
                return Err(value_error(
                    "end of string while looking for conversion specifier",
                ));
            };
            *pos += 1;
            field.conversion = conv;
            if let Some(&next) = s.get(*pos) {
                *pos += 1;
                if next == RBRACE {
                    return Ok(field);
                }
                if next != u32::from(':') {
                    return Err(value_error("expected ':' after conversion specifier"));
                }
            }
        }
        let spec_start = *pos;
        let mut count = 1;
        while let Some(&ch) = s.get(*pos) {
            *pos += 1;
            if ch == LBRACE {
                field.spec_needs_expanding = true;
                count += 1;
            } else if ch == RBRACE {
                count -= 1;
                if count == 0 {
                    field.spec = spec_start..*pos - 1;
                    return Ok(field);
                }
            }
        }
        return Err(value_error("unmatched '{' in format spec"));
    }
    if c != RBRACE {
        return Err(value_error("expected '}' before end of string"));
    }
    Ok(field)
}

/// CPython's `MarkupIterator_next`: the next literal run and the field
/// after it, or `None` at the end of `s`.
fn next_markup(s: &[u32], pos: &mut usize) -> PcResult<Option<(Range<usize>, Option<Field>)>> {
    if *pos >= s.len() {
        return Ok(None);
    }
    let start = *pos;
    let rest = s.get(start..).unwrap_or_default();
    // `c` is the brace that ended the literal (0 when the text ran out).
    let (c, mut markup) = match rest.iter().position(|&ch| ch == LBRACE || ch == RBRACE) {
        Some(i) => {
            *pos = start + i + 1;
            (rest[i], true)
        }
        None => {
            *pos = s.len();
            (0, false)
        }
    };
    let next = s.get(*pos).copied();
    if c == RBRACE && next != Some(RBRACE) {
        return Err(value_error("Single '}' encountered in format string"));
    }
    if c == LBRACE && next.is_none() {
        return Err(value_error("Single '{' encountered in format string"));
    }
    let mut literal_end = *pos;
    if let Some(next) = next {
        if next == c {
            *pos += 1;
            markup = false;
        } else {
            literal_end -= 1;
        }
    }
    let literal = start..literal_end;
    if !markup {
        return Ok(Some((literal, None)));
    }
    let field = parse_field(s, pos)?;
    Ok(Some((literal, Some(field))))
}

/// The shim's `_refuse_attribute_fields`: `TypeError` if any field (or a
/// field nested one level in a spec) reads an attribute. A template that
/// `formatter_parser` rejects anywhere is skipped entirely.
fn refuse_attribute_fields(s: &[u32], depth: u32) -> PcResult<()> {
    // Every refusal is the same error, so field order doesn't matter: the
    // template is refused if it parses and any field reads an attribute.
    let mut refused = false;
    let mut pos = 0;
    loop {
        match next_markup(s, &mut pos) {
            Ok(Some((_, Some(field)))) => {
                if refused {
                    continue;
                }
                let name = s.get(field.name.clone()).unwrap_or_default();
                let spec = s.get(field.spec.clone()).unwrap_or_default();
                // A spec without `{` holds no fields.
                refused = field_reads_attribute(name)
                    || (field.spec_needs_expanding
                        && depth < 1
                        && refuse_attribute_fields(spec, depth + 1).is_err());
            }
            Ok(Some((_, None))) => {}
            Ok(None) => break,
            Err(_) => return Ok(()),
        }
    }
    if refused {
        return Err(PyError::type_error(
            "pc_format doesn't support attribute access in format fields",
        ));
    }
    Ok(())
}

/// Walks `formatter_field_name_split(name)` until it yields an attribute
/// (`true`) or ends, normally or with a `ValueError` (`false`).
fn field_reads_attribute(name: &[u32]) -> bool {
    let split = name
        .iter()
        .position(|&c| c == u32::from('.') || c == u32::from('['))
        .unwrap_or(name.len());
    let (first, rest) = name.split_at(split);
    if get_integer(first).is_err() {
        return false;
    }
    let mut pos = 0;
    while let Some(&c) = rest.get(pos) {
        pos += 1;
        let name_start = pos;
        if c == u32::from('.') {
            while rest
                .get(pos)
                .is_some_and(|&x| x != u32::from('.') && x != u32::from('['))
            {
                pos += 1;
            }
            // Non-empty attribute names are yielded; empty ones raise ValueError.
            return pos > name_start;
        }
        if c != u32::from('[') {
            return false;
        }
        let Some(len) = rest
            .get(pos..)
            .and_then(|r| r.iter().position(|&x| x == u32::from(']')))
        else {
            return false;
        };
        pos += len + 1;
        let item = rest.get(name_start..name_start + len).unwrap_or_default();
        if item.is_empty() || get_integer(item).is_err() {
            return false;
        }
    }
    false
}

struct Context<'a> {
    args: &'a [Value],
    kwargs: &'a [(Value, Value)],
    auto: AutoNumber,
}

impl<'a> Context<'a> {
    /// CPython's `build_string` / `output_markup` loop.
    fn build_string(&mut self, s: &[u32], depth: u32) -> PcResult<PyStr> {
        if depth == 0 {
            return Err(value_error("Max string recursion exceeded"));
        }
        // Room for the fields' text too, which usually outgrows its markup.
        let mut out = Vec::with_capacity(s.len().saturating_mul(2));
        let mut pos = 0;
        while let Some((literal, field)) = next_markup(s, &mut pos)? {
            out.extend_from_slice(s.get(literal).unwrap_or_default());
            if let Some(field) = field {
                self.output_markup(s, &field, depth, &mut out)?;
            }
        }
        Ok(PyStr::from_code_points(out))
    }

    /// Formats one field, appending its text to `out`.
    fn output_markup(
        &mut self,
        s: &[u32],
        field: &Field,
        depth: u32,
        out: &mut Vec<u32>,
    ) -> PcResult<()> {
        let name = s.get(field.name.clone()).unwrap_or_default();
        let mut obj = self.field_object(name)?;
        if field.conversion != 0 {
            obj = obj.convert(field.conversion)?;
        }
        let spec_text = s.get(field.spec.clone()).unwrap_or_default();
        if field.spec_needs_expanding {
            let spec = self.build_string(spec_text, depth - 1)?;
            obj.format_into(spec.code_points(), out)
        } else {
            obj.format_into(spec_text, out)
        }
    }

    /// CPython's `get_field_object` (with `field_name_split`).
    fn field_object(&mut self, name: &[u32]) -> PcResult<Obj<'a>> {
        let split = name
            .iter()
            .position(|&c| c == u32::from('.') || c == u32::from('['))
            .unwrap_or(name.len());
        let (first, rest) = name.split_at(split);
        let mut index = get_integer(first)?;
        let is_empty = first.is_empty();
        if is_empty || index.is_some() {
            if self.auto.state == AutoState::Init {
                self.auto.state = if is_empty {
                    AutoState::Auto
                } else {
                    AutoState::Manual
                };
            }
            match (self.auto.state, is_empty) {
                (AutoState::Manual, true) => return Err(value_error(
                    "cannot switch from manual field specification to automatic field numbering",
                )),
                (AutoState::Auto, false) => return Err(value_error(
                    "cannot switch from automatic field numbering to manual field specification",
                )),
                _ => {}
            }
            if is_empty {
                index = Some(self.auto.next);
                self.auto.next += 1;
            }
        }
        let mut obj = match index {
            None => kwarg_lookup(self.kwargs, first)?,
            Some(i) => self
                .args
                .get(i)
                .map(|v| Obj::Val(Cow::Borrowed(v)))
                .ok_or_else(|| {
                    PyError::index_error(format!(
                        "Replacement index {i} out of range for positional args tuple"
                    ))
                })?,
        };
        let mut pos = 0;
        while let Some(&c) = rest.get(pos) {
            pos += 1;
            let name_start = pos;
            let is_attribute = c == u32::from('.');
            let item: &[u32] = if is_attribute {
                while rest
                    .get(pos)
                    .is_some_and(|&x| x != u32::from('.') && x != u32::from('['))
                {
                    pos += 1;
                }
                rest.get(name_start..pos).unwrap_or_default()
            } else if c == u32::from('[') {
                let Some(len) = rest
                    .get(pos..)
                    .and_then(|r| r.iter().position(|&x| x == u32::from(']')))
                else {
                    return Err(value_error("Missing ']' in format string"));
                };
                pos += len + 1;
                rest.get(name_start..name_start + len).unwrap_or_default()
            } else {
                return Err(value_error(
                    "Only '.' or '[' may follow ']' in format field specifier",
                ));
            };
            let key_index = if is_attribute {
                None
            } else {
                get_integer(item)?
            };
            if item.is_empty() {
                return Err(value_error("Empty attribute in format string"));
            }
            let text = PyStr::from_code_points(item.to_vec());
            obj = if is_attribute {
                obj.getattr(&text)?
            } else {
                match key_index {
                    Some(i) => obj.getitem(&Key::Index(i))?,
                    None => obj.getitem(&Key::Name(text))?,
                }
            };
        }
        Ok(obj)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::ExcKind;

    fn fmt(t: &str, args: Vec<Value>) -> PcResult<Value> {
        str_format_api(&Value::str(t), &Value::Tuple(args), &Value::None)
    }

    #[test]
    fn basics() {
        assert_eq!(
            fmt("{}-{}", vec![Value::int(1), Value::int(2)]),
            Ok(Value::str("1-2"))
        );
        assert_eq!(fmt("{{}}", vec![]), Ok(Value::str("{}")));
        assert_eq!(
            fmt("{0:{1}}", vec![Value::str("a"), Value::int(3)]),
            Ok(Value::str("a  "))
        );
        assert!(fmt("{0:{1:{2}}}", vec![Value::int(1); 3]).is_err());
    }

    fn err(t: &str, args: Vec<Value>) -> (ExcKind, String) {
        match fmt(t, args) {
            Err(PyError {
                kind,
                arg: Value::Str(m),
            }) => (kind, m.to_string_lossy()),
            other => (ExcKind::NotImplementedError, format!("{other:?}")),
        }
    }

    #[test]
    fn attribute_fields_match_cpython() {
        use ExcKind::{AttributeError, TypeError, ValueError};
        let refuse = "pc_format doesn't support attribute access in format fields";
        let int = |n: i64| vec![Value::int(n)];
        let cases: Vec<(&str, Vec<Value>, ExcKind, &str)> = vec![
            (
                "{0.real}{",
                int(5),
                ValueError,
                "Single '{' encountered in format string",
            ),
            (
                "{0.x}{",
                int(5),
                AttributeError,
                "'int' object has no attribute 'x'",
            ),
            (
                "{0.real.x}{",
                vec![Value::Bool(true)],
                AttributeError,
                "'int' object has no attribute 'x'",
            ),
            (
                "{0.bit_length:x}{",
                int(5),
                TypeError,
                "unsupported format string passed to builtin_function_or_method.__format__",
            ),
            (
                "{0.bit_length[0]}{",
                int(5),
                TypeError,
                "'builtin_function_or_method' object is not subscriptable",
            ),
            ("{0.x.}", vec![], TypeError, refuse),
            (
                "{0[}",
                vec![],
                ValueError,
                "expected '}' before end of string",
            ),
            ("{a.b}", vec![], TypeError, refuse),
            (
                "{0.\u{e9}}{",
                vec![Value::Float(1.5)],
                AttributeError,
                "'float' object has no attribute '\u{e9}'",
            ),
            (
                "{0.__class__.x}{",
                int(1),
                AttributeError,
                "type object 'int' has no attribute 'x'",
            ),
            (
                "{0.__class__[0]}{",
                int(1),
                TypeError,
                "type 'int' is not subscriptable",
            ),
            (
                "{0.__class__:x}{",
                int(1),
                TypeError,
                "unsupported format string passed to type.__format__",
            ),
            ("{0:{0.x}}", int(1), TypeError, refuse),
            (
                "{0:{1:{0.x}}}",
                vec![Value::int(1), Value::int(2)],
                ValueError,
                "Max string recursion exceeded",
            ),
            (
                "{0:{0.x}{!}}",
                int(1),
                AttributeError,
                "'int' object has no attribute 'x'",
            ),
            (
                "{0.imag!r:d}{",
                vec![Value::Float(1.5)],
                ValueError,
                "Unknown format code 'd' for object of type 'str'",
            ),
            (
                "{0.count!r:d}{",
                vec![Value::List(vec![])],
                ValueError,
                "Unknown format code 'd' for object of type 'str'",
            ),
            (
                "{0.real:{1}}{",
                vec![Value::int(1), Value::str("q")],
                ValueError,
                "Unknown format code 'q' for object of type 'int'",
            ),
            (
                "{0[0].x}{",
                vec![Value::List(int(1))],
                AttributeError,
                "'int' object has no attribute 'x'",
            ),
            ("{.x}", vec![], TypeError, refuse),
            (
                "{0.}{",
                int(1),
                ValueError,
                "Empty attribute in format string",
            ),
            (
                "{0!x.real}",
                int(1),
                ValueError,
                "expected ':' after conversion specifier",
            ),
            (
                "{0.__doc__.x}{",
                int(1),
                AttributeError,
                "'str' object has no attribute 'x'",
            ),
            (
                "{0.__hash__}{",
                vec![Value::List(vec![])],
                ValueError,
                "Single '{' encountered in format string",
            ),
            ("{0:{a.b}{}}", int(1), TypeError, refuse),
        ];
        for (t, args, kind, msg) in cases {
            assert_eq!(err(t, args), (kind, msg.to_owned()), "{t}");
        }
    }
}
