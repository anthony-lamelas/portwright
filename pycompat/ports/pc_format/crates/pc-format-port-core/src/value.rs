//! Shared value and error types. Frozen: owned by the planner (see
//! `/work/out/decisions.md`). Add helpers in your own module, not here.

use std::fmt;

use num_bigint::BigInt;

/// A Python `str`: a sequence of code points, which may include lone
/// surrogates (U+D800..=U+DFFF), so it can't be a Rust `String`.
///
/// `len()` counts code points, like Python's `len()`. Ordering and equality
/// compare code points, like Python's `str` comparisons.
#[derive(Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PyStr(Vec<u32>);

impl PyStr {
    /// The empty string.
    #[must_use]
    pub fn new() -> Self {
        PyStr(Vec::new())
    }

    /// Builds a string from code points (each must be `<= 0x10FFFF`).
    #[must_use]
    pub fn from_code_points(code_points: Vec<u32>) -> Self {
        PyStr(code_points)
    }

    /// The code points.
    #[must_use]
    pub fn code_points(&self) -> &[u32] {
        &self.0
    }

    /// Consumes the string, returning its code points.
    #[must_use]
    pub fn into_code_points(self) -> Vec<u32> {
        self.0
    }

    /// Python's `len()`: the number of code points.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the string is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Appends one character.
    pub fn push(&mut self, c: char) {
        self.0.push(u32::from(c));
    }

    /// Appends one code point (possibly a lone surrogate).
    pub fn push_code_point(&mut self, cp: u32) {
        self.0.push(cp);
    }

    /// Appends Rust text.
    pub fn push_str(&mut self, s: &str) {
        self.0.extend(s.chars().map(u32::from));
    }

    /// Appends another `PyStr`.
    pub fn push_py_str(&mut self, s: &PyStr) {
        self.0.extend_from_slice(&s.0);
    }

    /// The text as a `String`, or `None` if it holds a lone surrogate.
    #[must_use]
    pub fn to_string_strict(&self) -> Option<String> {
        self.0.iter().map(|&cp| char::from_u32(cp)).collect()
    }

    /// The text as a `String`, with lone surrogates replaced by U+FFFD.
    /// Only for code that can't observe the difference (e.g. ASCII parsing).
    #[must_use]
    pub fn to_string_lossy(&self) -> String {
        self.0
            .iter()
            .map(|&cp| char::from_u32(cp).unwrap_or('\u{fffd}'))
            .collect()
    }
}

impl From<&str> for PyStr {
    fn from(s: &str) -> Self {
        // Pre-sized: collecting `chars()` would grow from its size hint
        // (a quarter of the byte length) and reallocate.
        let mut code_points = Vec::with_capacity(s.len());
        if s.is_ascii() {
            code_points.extend(s.bytes().map(u32::from));
        } else {
            code_points.extend(s.chars().map(u32::from));
        }
        PyStr(code_points)
    }
}

impl From<String> for PyStr {
    fn from(s: String) -> Self {
        PyStr::from(s.as_str())
    }
}

impl From<char> for PyStr {
    fn from(c: char) -> Self {
        PyStr(vec![u32::from(c)])
    }
}

impl fmt::Debug for PyStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.to_string_lossy())
    }
}

/// A Python value as it crosses the boundary: exactly the built-in types
/// `pc_format` accepts, plus [`Value::Foreign`] for anything else.
///
/// `Bool` is distinct from `Int` (Python's `True` is an `int`, but its
/// `repr`, `str` and `format` differ). Numeric code must treat `Bool(b)` as
/// the int `b as i64` wherever Python does.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// `None`.
    None,
    /// `True` / `False`.
    Bool(bool),
    /// Arbitrary-precision `int`.
    Int(BigInt),
    /// `float` (IEEE-754 double; NaN keeps its sign bit).
    Float(f64),
    /// `str`.
    Str(PyStr),
    /// `bytes`.
    Bytes(Vec<u8>),
    /// `bytearray`.
    ByteArray(Vec<u8>),
    /// `tuple`.
    Tuple(Vec<Value>),
    /// `list`.
    List(Vec<Value>),
    /// `set`, items in the iteration order the binding saw.
    Set(Vec<Value>),
    /// `frozenset`, items in the iteration order the binding saw.
    FrozenSet(Vec<Value>),
    /// `dict`, in insertion order. Lookups must use Python equality
    /// (`1 == 1.0 == True`), so it is a list of pairs, not a map.
    Dict(Vec<(Value, Value)>),
    /// Any other type; carries `type(value).__name__`. Never valid: every
    /// API rejects it through [`check_builtins`] before doing anything.
    Foreign(String),
}

impl Value {
    /// `type(v).__name__`, as used in Python error messages.
    #[must_use]
    pub fn type_name(&self) -> &str {
        match self {
            Value::None => "NoneType",
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Str(_) => "str",
            Value::Bytes(_) => "bytes",
            Value::ByteArray(_) => "bytearray",
            Value::Tuple(_) => "tuple",
            Value::List(_) => "list",
            Value::Set(_) => "set",
            Value::FrozenSet(_) => "frozenset",
            Value::Dict(_) => "dict",
            Value::Foreign(name) => name,
        }
    }

    /// Builds a `Value::Str`.
    #[must_use]
    pub fn str(s: impl Into<PyStr>) -> Self {
        Value::Str(s.into())
    }

    /// Builds a `Value::Int`.
    #[must_use]
    pub fn int(n: impl Into<BigInt>) -> Self {
        Value::Int(n.into())
    }
}

/// The `TypeError` `pc_format`'s guard raises when `value` (or something
/// inside it) isn't a supported built-in, checking depth-first: for a dict,
/// each key before its value.
///
/// # Errors
/// `TypeError("pc_format supports built-in types only, not <name>")`.
pub fn check_builtin(value: &Value) -> PcResult<()> {
    match value {
        Value::Foreign(name) => Err(PyError::type_error(format!(
            "pc_format supports built-in types only, not {name}"
        ))),
        Value::Dict(pairs) => pairs.iter().try_for_each(|(k, v)| {
            check_builtin(k)?;
            check_builtin(v)
        }),
        Value::Tuple(items) | Value::List(items) | Value::Set(items) | Value::FrozenSet(items) => {
            items.iter().try_for_each(check_builtin)
        }
        _ => Ok(()),
    }
}

/// [`check_builtin`] over every argument, in order. Every public API calls
/// this first, on the arguments the caller passed.
///
/// # Errors
/// As [`check_builtin`], for the first unsupported value.
pub fn check_builtins(values: &[&Value]) -> PcResult<()> {
    values.iter().try_for_each(|v| check_builtin(v))
}

/// The Python exception class an error maps to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExcKind {
    /// `ValueError`.
    ValueError,
    /// `TypeError`.
    TypeError,
    /// `OverflowError`.
    OverflowError,
    /// `ZeroDivisionError`.
    ZeroDivisionError,
    /// `KeyError`.
    KeyError,
    /// `IndexError`.
    IndexError,
    /// `AttributeError`.
    AttributeError,
    /// `MemoryError`.
    MemoryError,
    /// `RecursionError`.
    RecursionError,
    /// `NotImplementedError`: only for APIs not ported yet.
    NotImplementedError,
}

/// A Python exception: its class and its single argument.
///
/// For every kind except `KeyError` the argument is `Value::Str(message)`.
/// For `KeyError` it is the missing key itself, because Python's
/// `str(KeyError(k))` is `repr(k)`; the binding passes the key through.
#[derive(Debug, Clone, PartialEq)]
pub struct PyError {
    /// Exception class.
    pub kind: ExcKind,
    /// The exception's single argument.
    pub arg: Value,
}

/// Result type used throughout the core.
pub type PcResult<T> = Result<T, PyError>;

impl PyError {
    /// An exception whose argument is a message string.
    #[must_use]
    pub fn new(kind: ExcKind, msg: impl Into<PyStr>) -> Self {
        PyError {
            kind,
            arg: Value::Str(msg.into()),
        }
    }

    /// `ValueError(msg)`.
    #[must_use]
    pub fn value_error(msg: impl Into<PyStr>) -> Self {
        Self::new(ExcKind::ValueError, msg)
    }

    /// `TypeError(msg)`.
    #[must_use]
    pub fn type_error(msg: impl Into<PyStr>) -> Self {
        Self::new(ExcKind::TypeError, msg)
    }

    /// `OverflowError(msg)`.
    #[must_use]
    pub fn overflow_error(msg: impl Into<PyStr>) -> Self {
        Self::new(ExcKind::OverflowError, msg)
    }

    /// `ZeroDivisionError(msg)`.
    #[must_use]
    pub fn zero_division_error(msg: impl Into<PyStr>) -> Self {
        Self::new(ExcKind::ZeroDivisionError, msg)
    }

    /// `IndexError(msg)`.
    #[must_use]
    pub fn index_error(msg: impl Into<PyStr>) -> Self {
        Self::new(ExcKind::IndexError, msg)
    }

    /// `AttributeError(msg)`.
    #[must_use]
    pub fn attribute_error(msg: impl Into<PyStr>) -> Self {
        Self::new(ExcKind::AttributeError, msg)
    }

    /// `KeyError(key)`: pass the key, not its repr.
    #[must_use]
    pub fn key_error(key: Value) -> Self {
        PyError {
            kind: ExcKind::KeyError,
            arg: key,
        }
    }

    /// The error an API raises until its task is ported.
    #[must_use]
    pub fn not_ported(api: &str) -> Self {
        Self::new(
            ExcKind::NotImplementedError,
            format!("{api} is not ported yet"),
        )
    }
}

impl fmt::Display for PyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.arg {
            Value::Str(s) => write!(f, "{:?}: {}", self.kind, s.to_string_lossy()),
            other => write!(f, "{:?}: {other:?}", self.kind),
        }
    }
}

impl std::error::Error for PyError {}
