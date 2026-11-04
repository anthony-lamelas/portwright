//! Python argument values, as the PyO3 layer hands them to the core.
//!
//! The shim accepts only built-in values (plus compiled patterns and matches
//! from this package), validates them recursively, binds them to parameters
//! and then lets `re` coerce them. All of that is behavior, so the PyO3 layer
//! converts every argument to a [`Value`] without interpreting it, and the core
//! does the rest.

use std::sync::Arc;

use num_bigint::BigInt;

use crate::matching::Match;
use crate::pattern::Pattern;
use crate::text::PyStr;

/// One Python argument value.
#[derive(Debug, Clone)]
pub enum Value {
    /// `None`
    None,
    /// `bool` (checked before `int`: `True` is an `int` in Python).
    Bool(bool),
    /// `int` of any size.
    Int(BigInt),
    /// `float`
    Float(f64),
    /// `str`
    Str(PyStr),
    /// `bytes`
    Bytes(Vec<u8>),
    /// `bytearray`
    ByteArray(Vec<u8>),
    /// `list`
    List(Vec<Value>),
    /// `tuple`
    Tuple(Vec<Value>),
    /// `set`, in the Python object's iteration order.
    Set(Vec<Value>),
    /// `frozenset`, in the Python object's iteration order.
    FrozenSet(Vec<Value>),
    /// `dict`, in insertion order.
    Dict(Vec<(Value, Value)>),
    /// `decimal.Decimal`, as the text `str()` gives it.
    Decimal(String),
    /// A compiled pattern made by this port (`re.Pattern`).
    Pattern(Arc<Pattern>),
    /// A match made by this port (`re.Match`).
    Match(Arc<Match>),
    /// Any other object; holds `type(value).__name__`.
    Other(String),
}

impl Value {
    /// `type(value).__name__`.
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
            Value::Decimal(_) => "Decimal",
            Value::Pattern(_) => "Pattern",
            Value::Match(_) => "Match",
            Value::Other(name) => name,
        }
    }

    /// `type(value).__module__ + '.' + __qualname__` as CPython prints it in
    /// attribute errors (`'re.Pattern' object has no attribute ...`): only the
    /// `re` types carry a module prefix.
    pub fn tp_name(&self) -> String {
        match self {
            Value::Pattern(_) => "re.Pattern".to_owned(),
            Value::Match(_) => "re.Match".to_owned(),
            Value::Decimal(_) => "decimal.Decimal".to_owned(),
            other => other.type_name().to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(Value::None.type_name(), "NoneType");
        assert_eq!(Value::Bool(true).type_name(), "bool");
        assert_eq!(Value::Other("Fraction".into()).type_name(), "Fraction");
        assert_eq!(Value::Decimal("1".into()).tp_name(), "decimal.Decimal");
        assert_eq!(Value::Int(BigInt::from(1)).tp_name(), "int");
    }
}
