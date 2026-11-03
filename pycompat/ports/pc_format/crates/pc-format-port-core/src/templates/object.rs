//! The objects a replacement field can reach (`{0[key]}`) and what a field
//! does with them: item lookup, `!r`/`!s`/`!a` and `format()`.
//!
//! Attribute access (`{0.attr}`) is refused by the shim's pre-scan, but a
//! template the pre-scan can't parse still reaches `str.format`, which does
//! real attribute lookups before it fails. Those use the tables in `attrs.rs`.

use std::borrow::Cow;

use num_bigint::BigInt;
use num_traits::FromPrimitive;

use crate::format_spec::{format_into, format_with_code_points};
use crate::repr::{ascii_repr, value_repr_py_str, value_str};
use crate::value::{PcResult, PyError, PyStr, Value};

use super::attrs::NODES;

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

/// What an attribute of a [`Node`] yields (generated from CPython 3.12).
#[derive(Debug)]
pub(super) enum Res {
    /// `None`.
    None,
    /// A small `int`.
    Int(i64),
    /// A `float`.
    Float(f64),
    /// A `str`.
    Str(&'static str),
    /// Another object, by its [`Node`] key.
    Node(&'static str),
    /// The lookup raises `AttributeError` with this message.
    Err(&'static str),
}

/// One kind of object a field can reach through attributes: the instances
/// of a type (`inst:<type>`) or a type object (`class:<type>`).
#[derive(Debug)]
pub(super) struct Node {
    /// `inst:<type name>` or `class:<type name>`.
    pub(super) key: &'static str,
    /// The `AttributeError` message for a missing name; `\0` marks the name.
    pub(super) missing: &'static str,
    /// The error `format(obj, 'x')` raises (`None`: it uses its own `__format__`).
    pub(super) format_error: Option<&'static str>,
    /// The error `obj[0]` raises (`None`: subscriptable).
    pub(super) subscript_error: Option<&'static str>,
    /// Every attribute name (sorted) and what it yields.
    pub(super) attrs: &'static [(&'static str, Res)],
}

fn node(key: &str) -> Option<&'static Node> {
    NODES
        .binary_search_by(|n| n.key.cmp(key))
        .ok()
        .and_then(|i| NODES.get(i))
}

fn str_equals(name: &PyStr, s: &str) -> bool {
    name.code_points()
        .iter()
        .copied()
        .eq(s.chars().map(u32::from))
}

/// A field's object: a built-in value, or something only reachable through
/// attributes (a method, descriptor, type object...). The latter only appear
/// when the pre-scan was skipped, so `str.format` always fails afterwards with
/// a parse error and only their *errors* are observable.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Obj<'a> {
    /// A built-in value, borrowed from the call's arguments when possible.
    Val(Cow<'a, Value>),
    /// An opaque object, by its attribute-table node key.
    Opaque(&'static str),
}

fn no_attribute(type_name: &str, name: &PyStr) -> PyError {
    let mut msg = PyStr::from(format!("'{type_name}' object has no attribute '"));
    msg.push_py_str(name);
    msg.push('\'');
    PyError::attribute_error(msg)
}

fn not_subscriptable(type_name: &str) -> PyError {
    PyError::type_error(format!("'{type_name}' object is not subscriptable"))
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
        Some((_, v)) => Ok(Obj::Val(Cow::Borrowed(v))),
        None => Err(PyError::key_error(key.to_value())),
    }
}

/// Looks `name` up in a str-keyed mapping (the `**kwargs` dict).
pub(super) fn kwarg_lookup<'a>(pairs: &'a [(Value, Value)], name: &[u32]) -> PcResult<Obj<'a>> {
    let found = pairs
        .iter()
        .find(|(k, _)| matches!(k, Value::Str(k) if k.code_points() == name));
    match found {
        Some((_, v)) => Ok(Obj::Val(Cow::Borrowed(v))),
        None => Err(PyError::key_error(Value::Str(PyStr::from_code_points(
            name.to_vec(),
        )))),
    }
}

/// The attributes whose value depends on the number itself.
fn numeric_attr(v: &Value, name: &PyStr) -> Option<Value> {
    let int = match v {
        Value::Int(n) => n.clone(),
        Value::Bool(b) => BigInt::from(u8::from(*b)),
        Value::Float(f) => {
            return if str_equals(name, "real") {
                Some(Value::Float(*f))
            } else if str_equals(name, "imag") {
                Some(Value::Float(0.0))
            } else {
                None
            };
        }
        _ => return None,
    };
    if str_equals(name, "real") || str_equals(name, "numerator") {
        Some(Value::Int(int))
    } else if str_equals(name, "imag") {
        Some(Value::int(0))
    } else if str_equals(name, "denominator") {
        Some(Value::int(1))
    } else {
        None
    }
}

fn owned(v: Value) -> Obj<'static> {
    Obj::Val(Cow::Owned(v))
}

fn resolve(res: &Res) -> Obj<'static> {
    match res {
        Res::None => owned(Value::None),
        Res::Int(n) => owned(Value::int(*n)),
        Res::Float(f) => owned(Value::Float(*f)),
        Res::Str(s) => owned(Value::str(*s)),
        Res::Node(k) | Res::Err(k) => Obj::Opaque(k),
    }
}

/// `getattr` against a node's table.
fn node_getattr(key: &str, name: &PyStr) -> PcResult<Obj<'static>> {
    let Some(n) = node(key) else {
        let type_name = key.split_once(':').map_or(key, |(_, t)| t);
        return Err(no_attribute(type_name, name));
    };
    match n.attrs.iter().find(|(a, _)| str_equals(name, a)) {
        Some((_, Res::Err(msg))) => Err(PyError::attribute_error(*msg)),
        Some((_, res)) => Ok(resolve(res)),
        None => {
            let mut msg = PyStr::new();
            for c in n.missing.chars() {
                if c == '\0' {
                    msg.push_py_str(name);
                } else {
                    msg.push(c);
                }
            }
            Err(PyError::attribute_error(msg))
        }
    }
}

/// The type name of an opaque node key (`inst:list` -> `list`).
fn opaque_type(key: &str) -> &str {
    match key.split_once(':') {
        Some(("class", _)) => "type",
        Some((_, t)) => t,
        None => key,
    }
}

/// A stand-in value for an opaque node whose type is a built-in value type.
fn placeholder(key: &str) -> Option<Value> {
    Some(match key {
        "inst:NoneType" => Value::None,
        "inst:bool" => Value::Bool(false),
        "inst:int" => Value::int(0),
        "inst:float" => Value::Float(0.0),
        "inst:str" => Value::str(""),
        "inst:bytes" => Value::Bytes(Vec::new()),
        "inst:bytearray" => Value::ByteArray(Vec::new()),
        "inst:tuple" => Value::Tuple(Vec::new()),
        "inst:list" => Value::List(Vec::new()),
        "inst:set" => Value::Set(Vec::new()),
        "inst:frozenset" => Value::FrozenSet(Vec::new()),
        "inst:dict" => Value::Dict(Vec::new()),
        _ => return None,
    })
}

impl<'a> Obj<'a> {
    /// `getattr(obj, name)`.
    pub(super) fn getattr(&self, name: &PyStr) -> PcResult<Obj<'a>> {
        match self {
            Obj::Val(v) => {
                if let Some(value) = numeric_attr(v, name) {
                    return Ok(owned(value));
                }
                node_getattr(&format!("inst:{}", v.type_name()), name)
            }
            Obj::Opaque(key) => node_getattr(key, name),
        }
    }

    /// `obj[key]`.
    pub(super) fn getitem(&self, key: &Key) -> PcResult<Obj<'a>> {
        match self {
            Obj::Val(Cow::Borrowed(v)) => value_getitem(v, key),
            Obj::Val(Cow::Owned(v)) => value_getitem(v, key).map(Obj::into_owned),
            Obj::Opaque(k) => match node(k).and_then(|n| n.subscript_error) {
                Some(msg) => Err(PyError::type_error(msg)),
                // Contents unknown: the result is discarded by the later parse error.
                None => Ok(Obj::Opaque("inst:object")),
            },
        }
    }

    /// Applies a `!r` / `!s` / `!a` conversion.
    pub(super) fn convert(&self, conversion: u32) -> PcResult<Obj<'a>> {
        let c = char::from_u32(conversion).filter(|c| matches!(c, 'a' | 'r' | 's'));
        let text = match (c, self) {
            (Some('a'), Obj::Val(v)) => PyStr::from(ascii_repr(v)?),
            (Some('r'), Obj::Val(v)) => value_repr_py_str(v)?,
            (Some(_), Obj::Val(v)) => value_str(v)?,
            (Some(_), Obj::Opaque(k)) => PyStr::from(format!("<{} object>", opaque_type(k))),
            (None, _) => {
                let msg = match char::from_u32(conversion) {
                    Some(c) if conversion > 32 && conversion < 127 => {
                        format!("Unknown conversion specifier {c}")
                    }
                    _ => format!("Unknown conversion specifier \\x{conversion:x}"),
                };
                return Err(PyError::value_error(msg));
            }
        };
        Ok(owned(Value::Str(text)))
    }

    /// The same object, no longer borrowing anything.
    fn into_owned(self) -> Obj<'static> {
        match self {
            Obj::Val(v) => owned(v.into_owned()),
            Obj::Opaque(k) => Obj::Opaque(k),
        }
    }

    /// [`Obj::format`], appending the text to `out`.
    pub(super) fn format_into(&self, spec: &[u32], out: &mut Vec<u32>) -> PcResult<()> {
        match self {
            Obj::Val(v) => format_into(v, spec, out),
            Obj::Opaque(_) => {
                out.extend_from_slice(self.format(spec)?.code_points());
                Ok(())
            }
        }
    }

    /// `format(obj, spec)`.
    pub(super) fn format(&self, spec: &[u32]) -> PcResult<PyStr> {
        match self {
            Obj::Val(v) => format_with_code_points(v, spec),
            Obj::Opaque(k) => {
                if let Some(v) = placeholder(k) {
                    return format_with_code_points(&v, spec);
                }
                if spec.is_empty() {
                    return Ok(PyStr::from(format!("<{} object>", opaque_type(k))));
                }
                let msg = node(k).and_then(|n| n.format_error).map_or_else(
                    || {
                        format!(
                            "unsupported format string passed to {}.__format__",
                            opaque_type(k)
                        )
                    },
                    str::to_owned,
                );
                Err(PyError::type_error(msg))
            }
        }
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
        other => return Err(not_subscriptable(other.type_name())),
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
    item.map(Obj::Val)
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
        assert_eq!(dict_lookup(&d, &Key::Index(1)), Ok(owned(Value::int(7))));
        assert_eq!(dict_lookup(&d, &Key::Index(2)), Ok(owned(Value::int(5))));
        assert!(dict_lookup(&d, &Key::Index(3)).is_err());
    }
}
