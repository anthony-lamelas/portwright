//! The objects a replacement field can reach (`{0[key]}`) and what a field
//! does with them: item lookup, `!r`/`!s`/`!a` and `format()`.
//!
//! Attribute access (`{0.attr}`) never gets here: the shim refuses every
//! attribute field `str.format` could evaluate before formatting starts.

use std::borrow::Cow;

use num_bigint::BigInt;
use num_traits::FromPrimitive;

use crate::format_spec::format_into;
use crate::repr::{ascii_repr, value_repr_py_str, value_str};
use crate::value::{PcResult, PyError, PyStr, Value};

/// An item key from `[...]`: digits give an int, anything else a str.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Key {
    Index(usize),
    Name(PyStr),
}

impl Key {
    fn to_value(&self) -> Value {
        match self {
            Key::Index(i) => Value::Int(BigInt::from(*i)),
            Key::Name(s) => Value::Str(s.clone()),
        }
    }
}

/// A field's object, borrowed from the call's arguments when possible.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Obj<'a>(Cow<'a, Value>);

impl<'a> Obj<'a> {
    pub(super) fn borrowed(v: &'a Value) -> Self {
        Obj(Cow::Borrowed(v))
    }

    fn owned(v: Value) -> Self {
        Obj(Cow::Owned(v))
    }

    /// `obj[key]`.
    pub(super) fn getitem(&self, key: &Key) -> PcResult<Obj<'a>> {
        match &self.0 {
            Cow::Borrowed(v) => value_getitem(v, key),
            Cow::Owned(v) => value_getitem(v, key).map(|o| Obj::owned(o.0.into_owned())),
        }
    }

    /// Applies a `!r` / `!s` / `!a` conversion.
    pub(super) fn convert(&self, conversion: u32) -> PcResult<Obj<'a>> {
        let v = self.0.as_ref();
        let text = match char::from_u32(conversion) {
            Some('a') => PyStr::from(ascii_repr(v)?),
            Some('r') => value_repr_py_str(v)?,
            Some('s') => value_str(v)?,
            _ => {
                let msg = match char::from_u32(conversion) {
                    Some(c) if conversion > 32 && conversion < 127 => {
                        format!("Unknown conversion specifier {c}")
                    }
                    _ => format!("Unknown conversion specifier \\x{conversion:x}"),
                };
                return Err(PyError::value_error(msg));
            }
        };
        Ok(Obj::owned(Value::Str(text)))
    }

    /// `format(obj, spec)`, appending the text to `out`.
    pub(super) fn format_into(&self, spec: &[u32], out: &mut Vec<u32>) -> PcResult<()> {
        format_into(&self.0, spec, out)
    }
}

/// Python `==` between an int key and a dict key.
fn int_equals(n: usize, key: &Value) -> bool {
    match key {
        Value::Int(k) => *k == BigInt::from(n),
        Value::Bool(b) => n == usize::from(*b),
        Value::Float(f) => {
            f.is_finite() && f.fract() == 0.0 && BigInt::from_f64(*f) == Some(BigInt::from(n))
        }
        _ => false,
    }
}

fn dict_lookup<'a>(pairs: &'a [(Value, Value)], key: &Key) -> PcResult<Obj<'a>> {
    let found = pairs.iter().find(|(k, _)| match (key, k) {
        (Key::Index(n), k) => int_equals(*n, k),
        (Key::Name(s), Value::Str(k)) => s == k,
        (Key::Name(_), _) => false,
    });
    match found {
        Some((_, v)) => Ok(Obj::borrowed(v)),
        None => Err(PyError::key_error(key.to_value())),
    }
}

/// Looks `name` up in a str-keyed mapping (the `**kwargs` dict).
pub(super) fn kwarg_lookup<'a>(pairs: &'a [(Value, Value)], name: &[u32]) -> PcResult<Obj<'a>> {
    let found = pairs
        .iter()
        .find(|(k, _)| matches!(k, Value::Str(k) if k.code_points() == name));
    match found {
        Some((_, v)) => Ok(Obj::borrowed(v)),
        None => Err(PyError::key_error(Value::Str(PyStr::from_code_points(
            name.to_vec(),
        )))),
    }
}

/// `value[key]` for a built-in value.
fn value_getitem<'a>(v: &'a Value, key: &Key) -> PcResult<Obj<'a>> {
    // Per sequence type: the IndexError text and the TypeError text for a str key.
    let (out_of_range, str_key) = match v {
        Value::Dict(pairs) => return dict_lookup(pairs, key),
        Value::List(_) => (
            "list index out of range",
            "list indices must be integers or slices, not str",
        ),
        Value::Tuple(_) => (
            "tuple index out of range",
            "tuple indices must be integers or slices, not str",
        ),
        Value::Str(_) => (
            "string index out of range",
            "string indices must be integers, not 'str'",
        ),
        Value::Bytes(_) => (
            "index out of range",
            "byte indices must be integers or slices, not str",
        ),
        Value::ByteArray(_) => (
            "bytearray index out of range",
            "bytearray indices must be integers or slices, not str",
        ),
        other => {
            return Err(PyError::type_error(format!(
                "'{}' object is not subscriptable",
                other.type_name()
            )))
        }
    };
    let Key::Index(i) = *key else {
        return Err(PyError::type_error(str_key));
    };
    let item = match v {
        Value::List(items) | Value::Tuple(items) => items.get(i).map(Cow::Borrowed),
        Value::Str(s) => s
            .code_points()
            .get(i)
            .map(|&cp| Cow::Owned(Value::Str(PyStr::from_code_points(vec![cp])))),
        Value::Bytes(b) | Value::ByteArray(b) => b.get(i).map(|&x| Cow::Owned(Value::int(x))),
        _ => None,
    };
    item.map(Obj)
        .ok_or_else(|| PyError::index_error(out_of_range))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dict_int_key_uses_python_equality() {
        let d = vec![
            (Value::Float(2.0), Value::int(5)),
            (Value::Bool(true), Value::int(7)),
        ];
        assert_eq!(
            dict_lookup(&d, &Key::Index(1)),
            Ok(Obj::owned(Value::int(7)))
        );
        assert_eq!(
            dict_lookup(&d, &Key::Index(2)),
            Ok(Obj::owned(Value::int(5)))
        );
        assert!(dict_lookup(&d, &Key::Index(3)).is_err());
    }
}
