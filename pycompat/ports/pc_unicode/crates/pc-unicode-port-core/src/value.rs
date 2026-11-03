//! Python argument values as seen by the core.

use num_bigint::BigInt;

use crate::error::{Error, Result};

/// Python text: a sequence of code points `0..=0x10FFFF`, lone surrogates included.
/// Never use Rust `char`/`String` for user text; they cannot hold surrogates.
pub type Text = Vec<u32>;

/// A Python argument, classified by its *exact* type (`type(x) is ...`).
/// Subclasses of built-ins and every other type are [`Value::Other`].
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// `None`.
    None,
    /// `bool`.
    Bool(bool),
    /// `int` (arbitrary precision).
    Int(BigInt),
    /// `float`.
    Float(f64),
    /// `str`.
    Str(Text),
    /// `bytes`.
    Bytes(Vec<u8>),
    /// `bytearray`.
    ByteArray(Vec<u8>),
    /// `list`, items in order.
    List(Vec<Value>),
    /// `tuple`, items in order.
    Tuple(Vec<Value>),
    /// `set`, items in Python iteration order.
    Set(Vec<Value>),
    /// `frozenset`, items in Python iteration order.
    FrozenSet(Vec<Value>),
    /// `dict`, `(key, value)` pairs in insertion order.
    Dict(Vec<(Value, Value)>),
    /// Any other type; holds `type(x).__name__`.
    Other(String),
}

impl Value {
    /// `type(x).__name__`.
    pub fn type_name(&self) -> &str {
        match self {
            Value::None => "NoneType",
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Str(_) => "str",
            Value::Bytes(_) => "bytes",
            Value::ByteArray(_) => "bytearray",
            Value::List(_) => "list",
            Value::Tuple(_) => "tuple",
            Value::Set(_) => "set",
            Value::FrozenSet(_) => "frozenset",
            Value::Dict(_) => "dict",
            Value::Other(name) => name,
        }
    }

    /// Python truthiness (`bool(x)`). `Other` values are never reached because the
    /// built-in guard rejects them first; they count as true.
    pub fn truthy(&self) -> bool {
        match self {
            Value::None => false,
            Value::Bool(b) => *b,
            Value::Int(i) => i.sign() != num_bigint::Sign::NoSign,
            Value::Float(f) => *f != 0.0,
            Value::Str(s) => !s.is_empty(),
            Value::Bytes(b) | Value::ByteArray(b) => !b.is_empty(),
            Value::List(v) | Value::Tuple(v) | Value::Set(v) | Value::FrozenSet(v) => !v.is_empty(),
            Value::Dict(d) => !d.is_empty(),
            Value::Other(_) => true,
        }
    }

    /// The text if this is exactly a `str`.
    pub fn as_text(&self) -> Option<&[u32]> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
}

/// Upstream `_check_builtin`: depth-first, dict key before its value, first offender wins.
pub fn check_builtin(value: &Value) -> Result<()> {
    match value {
        Value::Other(name) => Err(Error::type_error(format!(
            "pc_unicode supports built-in types only, not {name}"
        ))),
        Value::List(items) | Value::Tuple(items) | Value::Set(items) | Value::FrozenSet(items) => {
            items.iter().try_for_each(check_builtin)
        }
        Value::Dict(pairs) => pairs.iter().try_for_each(|(k, v)| {
            check_builtin(k)?;
            check_builtin(v)
        }),
        _ => Ok(()),
    }
}
