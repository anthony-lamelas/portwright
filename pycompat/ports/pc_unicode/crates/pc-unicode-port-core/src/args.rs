//! Argument checks shared by several CPython wrappers.

use crate::error::{Error, Result};
use crate::value::Value;

/// The `self` check of an unbound `str` method descriptor such as `str.upper`.
pub(crate) fn self_text<'a>(name: &str, s: &'a Value) -> Result<&'a [u32]> {
    s.as_text().ok_or_else(|| {
        Error::type_error(format!(
            "descriptor '{name}' for 'str' objects doesn't apply to a '{}' object",
            s.type_name()
        ))
    })
}

/// The type name CPython's `_PyArg_BadArgument` shows: `None` rather than `NoneType`.
pub(crate) fn bad_arg_type_name(v: &Value) -> &str {
    match v {
        Value::None => "None",
        other => other.type_name(),
    }
}
