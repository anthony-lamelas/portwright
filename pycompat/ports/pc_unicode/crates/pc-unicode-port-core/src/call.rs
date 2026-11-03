//! Argument binding with CPython's messages for a plain `def f(a, b=..., ...)`.
//!
//! Upstream wraps every API in `def checked(*args, **kwargs)`, which first runs the
//! built-in guard over every argument and then calls the real function, so CPython's
//! own call errors (`missing`, `takes N positional`, ...) follow the guard.

use crate::error::{Error, Result};
use crate::value::{check_builtin, Value};

/// One positional-or-keyword parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Param {
    /// Parameter name.
    pub name: &'static str,
    /// True when the parameter has no default.
    pub required: bool,
}

impl Param {
    /// A parameter without a default.
    pub const fn required(name: &'static str) -> Self {
        Self {
            name,
            required: true,
        }
    }

    /// A parameter with a default.
    pub const fn optional(name: &'static str) -> Self {
        Self {
            name,
            required: false,
        }
    }
}

/// Runs the guard over every positional and keyword value, in call order.
pub fn guard<'a>(
    args: impl IntoIterator<Item = &'a Value>,
    kwargs: impl IntoIterator<Item = &'a Value>,
) -> Result<()> {
    args.into_iter().chain(kwargs).try_for_each(check_builtin)
}

/// Binds `args`/`kwargs` to `params` like CPython does for function `name`.
/// Returns one slot per parameter; `None` means "use the default".
/// Generic so the PyO3 layer can bind original objects alongside [`Value`]s.
pub fn bind<T>(
    name: &str,
    params: &[Param],
    args: Vec<T>,
    kwargs: Vec<(String, T)>,
) -> Result<Vec<Option<T>>> {
    let n_args = args.len();
    let mut slots: Vec<Option<T>> = params.iter().map(|_| None).collect();
    for (slot, arg) in slots.iter_mut().zip(args) {
        *slot = Some(arg);
    }
    for (key, value) in kwargs {
        let Some(index) = params.iter().position(|p| p.name == key) else {
            return Err(Error::type_error(format!(
                "{name}() got an unexpected keyword argument '{key}'"
            )));
        };
        if slots[index].is_some() {
            return Err(Error::type_error(format!(
                "{name}() got multiple values for argument '{key}'"
            )));
        }
        slots[index] = Some(value);
    }
    if n_args > params.len() {
        return Err(too_many_positional(name, params, n_args));
    }
    let missing: Vec<&str> = params
        .iter()
        .zip(&slots)
        .filter(|(p, s)| p.required && s.is_none())
        .map(|(p, _)| p.name)
        .collect();
    if !missing.is_empty() {
        return Err(missing_error(name, &missing));
    }
    Ok(slots)
}

fn too_many_positional(name: &str, params: &[Param], given: usize) -> Error {
    let max = params.len();
    let min = params.iter().filter(|p| p.required).count();
    let takes = if min == max {
        format!("{max} positional argument{}", plural(max))
    } else {
        format!("from {min} to {max} positional arguments")
    };
    let verb = if given == 1 { "was" } else { "were" };
    Error::type_error(format!("{name}() takes {takes} but {given} {verb} given"))
}

fn missing_error(name: &str, missing: &[&str]) -> Error {
    let quoted: Vec<String> = missing.iter().map(|m| format!("'{m}'")).collect();
    let list = match quoted.as_slice() {
        [one] => one.clone(),
        [a, b] => format!("{a} and {b}"),
        [init @ .., last] => format!("{}, and {last}", init.join(", ")),
        [] => String::new(),
    };
    Error::type_error(format!(
        "{name}() missing {} required positional argument{}: {list}",
        missing.len(),
        plural(missing.len())
    ))
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

    const SPLIT: [Param; 3] = [
        Param::required("s"),
        Param::optional("sep"),
        Param::optional("maxsplit"),
    ];

    fn msg<T: std::fmt::Debug>(r: Result<T>) -> String {
        r.unwrap_err().message
    }

    #[test]
    fn messages_match_cpython() {
        assert_eq!(
            msg(bind::<u8>("split", &SPLIT, vec![1, 2, 3, 4], vec![])),
            "split() takes from 1 to 3 positional arguments but 4 were given"
        );
        assert_eq!(
            msg(bind::<u8>(
                "split",
                &SPLIT,
                vec![1, 2, 3, 4],
                vec![("x".into(), 1)]
            )),
            "split() got an unexpected keyword argument 'x'"
        );
        assert_eq!(
            msg(bind::<u8>("split", &SPLIT, vec![1], vec![("s".into(), 1)])),
            "split() got multiple values for argument 's'"
        );
        assert_eq!(
            msg(bind::<u8>("unidata_version", &[], vec![1], vec![])),
            "unidata_version() takes 0 positional arguments but 1 was given"
        );
        let center = [
            Param::required("s"),
            Param::required("width"),
            Param::optional("fillchar"),
        ];
        assert_eq!(
            msg(bind::<u8>("center", &center, vec![], vec![])),
            "center() missing 2 required positional arguments: 's' and 'width'"
        );
    }
}
