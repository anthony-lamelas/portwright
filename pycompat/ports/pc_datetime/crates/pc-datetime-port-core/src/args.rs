//! Entering a shim function: the built-in type check, then Python's argument binding.

use crate::error::{PyError, PyResult};
use crate::pystr::PyStr;
use crate::value::Value;

/// Keyword arguments in call order. Names are `PyStr`: they may hold lone surrogates,
/// which binding errors echo back.
pub type Kwargs = [(PyStr, Value)];

/// The default of a parameter of a shim function.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Default {
    /// No default: the argument is required.
    Required,
    /// `None`.
    None,
    /// An `int`.
    Int(i64),
    /// A `str`.
    Str(&'static str),
}

/// One positional-or-keyword parameter of a shim function.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Param {
    /// Its Python name.
    pub name: &'static str,
    /// Its default.
    pub default: Default,
}

impl Param {
    /// A parameter without a default.
    pub const fn required(name: &'static str) -> Self {
        Self {
            name,
            default: Default::Required,
        }
    }

    /// A parameter with a default.
    pub const fn optional(name: &'static str, default: Default) -> Self {
        Self { name, default }
    }
}

/// Runs what every shim function does on entry: `_builtins_only`'s type check over the
/// positional then keyword values, then binds them to `params` as a CPython 3.12 call
/// of `func(...)` would, with its TypeError messages. Returns one value per parameter.
///
/// # Errors
/// The `_builtins_only` TypeError, or CPython's binding TypeError for a bad call.
pub fn bind(func: &str, params: &[Param], args: &[Value], kwargs: &Kwargs) -> PyResult<Vec<Value>> {
    for value in args.iter().chain(kwargs.iter().map(|(_, v)| v)) {
        value.check_builtin()?;
    }
    let mut slots: Vec<Option<Value>> = vec![None; params.len()];
    for (slot, value) in slots.iter_mut().zip(args) {
        *slot = Some(value.clone());
    }
    for (key, value) in kwargs {
        let Some(index) = params.iter().position(|p| *key == p.name) else {
            return Err(PyError::type_error(quoting(
                &format!("{func}() got an unexpected keyword argument"),
                key,
            )));
        };
        if slots[index].is_some() {
            return Err(PyError::type_error(quoting(
                &format!("{func}() got multiple values for argument"),
                key,
            )));
        }
        slots[index] = Some(value.clone());
    }
    if args.len() > params.len() {
        let required = params
            .iter()
            .filter(|p| p.default == Default::Required)
            .count();
        let takes = if required == params.len() {
            format!(
                "{} positional argument{}",
                params.len(),
                plural(params.len())
            )
        } else {
            format!("from {required} to {} positional arguments", params.len())
        };
        let verb = if args.len() == 1 { "was" } else { "were" };
        return Err(PyError::type_error(format!(
            "{func}() takes {takes} but {} {verb} given",
            args.len()
        )));
    }
    let missing: Vec<String> = params
        .iter()
        .zip(&slots)
        .filter(|(p, slot)| slot.is_none() && p.default == Default::Required)
        .map(|(p, _)| format!("'{}'", p.name))
        .collect();
    if !missing.is_empty() {
        let names = match missing.as_slice() {
            [one] => one.clone(),
            [a, b] => format!("{a} and {b}"),
            [init @ .., last] => format!("{}, and {last}", init.join(", ")),
            [] => String::new(),
        };
        return Err(PyError::type_error(format!(
            "{func}() missing {} required positional argument{}: {names}",
            missing.len(),
            plural(missing.len())
        )));
    }
    Ok(params
        .iter()
        .zip(slots)
        .map(|(p, slot)| {
            slot.unwrap_or_else(|| match p.default {
                Default::Required | Default::None => Value::None,
                Default::Int(n) => Value::int(n),
                Default::Str(s) => Value::str(s),
            })
        })
        .collect())
}

/// `"{prefix} '{name}'"`, keeping any lone surrogates in `name` (CPython's `%S`).
fn quoting(prefix: &str, name: &PyStr) -> PyStr {
    let mut out = PyStr::from(format!("{prefix} '"));
    out.push_str(name);
    out.push_str(&PyStr::from("'"));
    out
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const F3: [Param; 3] = [
        Param::required("a"),
        Param::required("b"),
        Param::required("c"),
    ];

    fn message(r: PyResult<Vec<Value>>) -> String {
        r.map_or_else(|e| e.to_string(), |_| "ok".to_owned())
    }

    #[test]
    fn messages_match_cpython() {
        let one = [Value::int(1)];
        let four: Vec<Value> = (1..=4).map(Value::int).collect();
        assert_eq!(
            message(bind("f3", &F3, &[], &[])),
            "TypeError: f3() missing 3 required positional arguments: 'a', 'b', and 'c'"
        );
        assert_eq!(
            message(bind("f3", &F3, &four, &[("a".into(), Value::int(1))])),
            "TypeError: f3() got multiple values for argument 'a'"
        );
        assert_eq!(
            message(bind("f3", &F3, &four, &[])),
            "TypeError: f3() takes 3 positional arguments but 4 were given"
        );
        assert_eq!(
            message(bind("f0", &[], &one, &[])),
            "TypeError: f0() takes 0 positional arguments but 1 was given"
        );
        let f2 = [Param::required("a"), Param::optional("b", Default::Int(1))];
        assert_eq!(
            message(bind("f2", &f2, &four[..3], &[])),
            "TypeError: f2() takes from 1 to 2 positional arguments but 3 were given"
        );
        assert_eq!(
            message(bind(
                "f3",
                &F3,
                &one,
                &[("x".into(), Value::int(1)), ("a".into(), Value::int(2))]
            )),
            "TypeError: f3() got an unexpected keyword argument 'x'"
        );
    }

    #[test]
    fn keyword_names_keep_lone_surrogates() {
        let key = PyStr::from_code_points(vec![0xDC80]);
        let err = bind("f3", &F3, &[], &[(key.clone(), Value::int(1))]);
        let mut expected = PyStr::from("f3() got an unexpected keyword argument '");
        expected.push_str(&key);
        expected.push_str(&PyStr::from("'"));
        assert_eq!(err, Err(PyError::TypeError(expected)));
    }
}
