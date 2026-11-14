//! Python values as they cross the boundary: the built-in types this library accepts.

use std::fmt::Write as _;

use num_bigint::BigInt;

use crate::error::{PyError, PyResult};
use crate::pystr::PyStr;

/// A Python value. Containers keep Python's iteration order (sets and dicts as the
/// PyO3 layer iterated them). `Other` is any non-built-in object, by its type name.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// `None`.
    None,
    /// `bool`. Checked before `int`, since `True` is an `int` in Python.
    Bool(bool),
    /// `int`, unbounded.
    Int(BigInt),
    /// `float`.
    Float(f64),
    /// `str`.
    Str(PyStr),
    /// `bytes`.
    Bytes(Vec<u8>),
    /// `bytearray`.
    ByteArray(Vec<u8>),
    /// `list`.
    List(Vec<Value>),
    /// `tuple`.
    Tuple(Vec<Value>),
    /// `set`, in iteration order.
    Set(Vec<Value>),
    /// `frozenset`, in iteration order.
    FrozenSet(Vec<Value>),
    /// `dict`, in insertion order.
    Dict(Vec<(Value, Value)>),
    /// Any other object, such as a `Decimal`, by `type(obj).__name__`.
    Other(String),
}

impl Value {
    /// An `int`.
    pub fn int(n: impl Into<BigInt>) -> Self {
        Self::Int(n.into())
    }

    /// A `str`.
    pub fn str(s: impl Into<PyStr>) -> Self {
        Self::Str(s.into())
    }

    /// A `tuple`.
    pub fn tuple(items: Vec<Value>) -> Self {
        Self::Tuple(items)
    }

    /// `type(v).__name__`.
    pub fn type_name(&self) -> &str {
        match self {
            Self::None => "NoneType",
            Self::Bool(_) => "bool",
            Self::Int(_) => "int",
            Self::Float(_) => "float",
            Self::Str(_) => "str",
            Self::Bytes(_) => "bytes",
            Self::ByteArray(_) => "bytearray",
            Self::List(_) => "list",
            Self::Tuple(_) => "tuple",
            Self::Set(_) => "set",
            Self::FrozenSet(_) => "frozenset",
            Self::Dict(_) => "dict",
            Self::Other(name) => name,
        }
    }

    /// The shim's `_check_builtin`: TypeError for the first non-built-in value, depth first,
    /// dict keys before their values.
    ///
    /// # Errors
    /// `TypeError("pc_datetime supports built-in types only, not T")`.
    pub fn check_builtin(&self) -> PyResult<()> {
        match self {
            Self::Other(name) => Err(PyError::type_error(format!(
                "pc_datetime supports built-in types only, not {name}"
            ))),
            Self::List(items) | Self::Tuple(items) | Self::Set(items) | Self::FrozenSet(items) => {
                items.iter().try_for_each(Self::check_builtin)
            }
            Self::Dict(pairs) => pairs.iter().try_for_each(|(key, item)| {
                key.check_builtin()?;
                item.check_builtin()
            }),
            _ => Ok(()),
        }
    }

    /// `v[index]` for a non-negative index, with CPython's errors. The shim indexes
    /// tuples it was given (`t[0]`, `t[1]`, ...), so any built-in may arrive here.
    ///
    /// # Errors
    /// IndexError, KeyError or TypeError, as `v[index]` raises them.
    pub fn get_index(&self, index: usize) -> PyResult<Value> {
        let out_of_range = |what: &str| PyError::index_error(format!("{what} index out of range"));
        match self {
            Self::Tuple(items) => items
                .get(index)
                .cloned()
                .ok_or_else(|| out_of_range("tuple")),
            Self::List(items) => items
                .get(index)
                .cloned()
                .ok_or_else(|| out_of_range("list")),
            Self::Str(s) => s
                .code_points()
                .get(index)
                .map(|&c| Self::Str(PyStr::from_code_points(vec![c])))
                .ok_or_else(|| out_of_range("string")),
            Self::Bytes(b) => b
                .get(index)
                .map(|&x| Self::int(x))
                .ok_or_else(|| PyError::index_error("index out of range")),
            Self::ByteArray(b) => b
                .get(index)
                .map(|&x| Self::int(x))
                .ok_or_else(|| out_of_range("bytearray")),
            Self::Dict(pairs) => pairs
                .iter()
                .find(|(key, _)| key.equals_index(index))
                .map(|(_, item)| item.clone())
                .ok_or_else(|| PyError::KeyError(Self::int(index))),
            _ => Err(PyError::type_error(format!(
                "'{}' object is not subscriptable",
                self.type_name()
            ))),
        }
    }

    /// Whether `self == index` in Python (`0 == False == 0.0`).
    fn equals_index(&self, index: usize) -> bool {
        match self {
            Self::Int(n) => *n == BigInt::from(index),
            Self::Bool(b) => usize::from(*b) == index,
            #[expect(clippy::cast_precision_loss, reason = "indexes here are tiny")]
            Self::Float(f) => *f == index as f64,
            _ => false,
        }
    }

    /// The items `*v` unpacks to, or `None` if the value isn't iterable.
    /// A `str` yields one-character strings, `bytes` yields ints, a `dict` yields keys.
    pub fn iterate(&self) -> Option<Vec<Value>> {
        match self {
            Self::List(items) | Self::Tuple(items) | Self::Set(items) | Self::FrozenSet(items) => {
                Some(items.clone())
            }
            Self::Str(s) => Some(
                s.code_points()
                    .iter()
                    .map(|&c| Self::Str(PyStr::from_code_points(vec![c])))
                    .collect(),
            ),
            Self::Bytes(b) | Self::ByteArray(b) => Some(b.iter().map(|&x| Self::int(x)).collect()),
            Self::Dict(pairs) => Some(pairs.iter().map(|(key, _)| key.clone()).collect()),
            _ => None,
        }
    }

    /// `repr(v)` as CPython writes it. `Other` values give `<Name object>`.
    pub fn repr(&self) -> String {
        match self {
            Self::None => "None".to_owned(),
            Self::Bool(true) => "True".to_owned(),
            Self::Bool(false) => "False".to_owned(),
            Self::Int(n) => n.to_string(),
            Self::Float(f) => float_repr(*f),
            Self::Str(s) => s.repr(),
            Self::Bytes(b) => format!("b{}", bytes_repr(b)),
            Self::ByteArray(b) => format!("bytearray(b{})", bytes_repr(b)),
            Self::List(items) => format!("[{}]", join_reprs(items)),
            Self::Tuple(items) if items.len() == 1 => format!("({},)", items[0].repr()),
            Self::Tuple(items) => format!("({})", join_reprs(items)),
            Self::Set(items) if items.is_empty() => "set()".to_owned(),
            Self::Set(items) => format!("{{{}}}", join_reprs(items)),
            Self::FrozenSet(items) if items.is_empty() => "frozenset()".to_owned(),
            Self::FrozenSet(items) => format!("frozenset({{{}}})", join_reprs(items)),
            Self::Dict(pairs) => {
                let parts: Vec<String> = pairs
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k.repr(), v.repr()))
                    .collect();
                format!("{{{}}}", parts.join(", "))
            }
            Self::Other(name) => format!("<{name} object>"),
        }
    }
}

fn join_reprs(items: &[Value]) -> String {
    items.iter().map(Value::repr).collect::<Vec<_>>().join(", ")
}

fn bytes_repr(bytes: &[u8]) -> String {
    let quote = if bytes.contains(&b'\'') && !bytes.contains(&b'"') {
        b'"'
    } else {
        b'\''
    };
    let mut out = String::new();
    out.push(char::from(quote));
    for &b in bytes {
        match b {
            b'\\' => out.push_str("\\\\"),
            b'\t' => out.push_str("\\t"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            _ if b == quote => {
                out.push('\\');
                out.push(char::from(b));
            }
            0x20..=0x7E => out.push(char::from(b)),
            _ => {
                let _ = write!(out, "\\x{b:02x}");
            }
        }
    }
    out.push(char::from(quote));
    out
}

/// `repr(f)` for a float, as CPython writes it (shortest round-trip digits,
/// scientific notation outside `1e-4 <= |f| < 1e16`).
pub fn float_repr(f: f64) -> String {
    if f.is_nan() {
        return "nan".to_owned();
    }
    if f.is_infinite() {
        return if f > 0.0 { "inf" } else { "-inf" }.to_owned();
    }
    // Rust's `{:e}` gives the shortest round-trip digits, like "-1.25e-7".
    let sci = format!("{f:e}");
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let (sign, mantissa) = match mantissa.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", mantissa),
    };
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let ndigits = i32::try_from(digits.len()).unwrap_or(i32::MAX);
    let body = if (-4..16).contains(&exp) {
        let decpt = exp + 1;
        if decpt <= 0 {
            format!("0.{}{digits}", "0".repeat(decpt.unsigned_abs() as usize))
        } else if decpt >= ndigits {
            format!(
                "{digits}{}.0",
                "0".repeat((decpt - ndigits).unsigned_abs() as usize)
            )
        } else {
            let (int_part, frac) = digits.split_at(decpt.unsigned_abs() as usize);
            format!("{int_part}.{frac}")
        }
    } else {
        let (first, rest) = digits.split_at(1);
        let frac = if rest.is_empty() {
            String::new()
        } else {
            format!(".{rest}")
        };
        let exp_sign = if exp < 0 { '-' } else { '+' };
        format!("{first}{frac}e{exp_sign}{:02}", exp.unsigned_abs())
    };
    format!("{sign}{body}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_repr_matches_cpython() {
        let cases = [
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (1.5, "1.5"),
            (1e16, "1e+16"),
            (1e15, "1000000000000000.0"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (1.0 / 3.0, "0.3333333333333333"),
            (-2.5e-7, "-2.5e-07"),
            (1.7976931348623157e308, "1.7976931348623157e+308"),
            (123456.789, "123456.789"),
        ];
        for (f, want) in cases {
            assert_eq!(float_repr(f), want);
        }
    }

    #[test]
    fn indexing_matches_cpython() {
        let d = Value::Dict(vec![(Value::Bool(false), Value::int(3))]);
        assert_eq!(d.get_index(0), Ok(Value::int(3)));
        assert_eq!(d.get_index(1), Err(PyError::KeyError(Value::int(1))));
        assert_eq!(
            Value::None.get_index(0),
            Err(PyError::type_error(
                "'NoneType' object is not subscriptable"
            ))
        );
        assert_eq!(Value::Bytes(vec![7]).get_index(0), Ok(Value::int(7)));
    }
}
