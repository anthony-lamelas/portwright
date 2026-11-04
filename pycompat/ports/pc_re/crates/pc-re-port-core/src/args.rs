//! Call plumbing shared by every API: the shim's `_check_builtin`, CPython's
//! argument binding (with its exact `TypeError` messages) and `_text`.

use smallvec::SmallVec;

use crate::error::{Error, Result};
use crate::value::Value;

/// Bound parameter slots; every signature fits inline, so binding doesn't allocate.
pub type Slots = SmallVec<[Option<ArgSource>; 6]>;

/// Where a bound argument came from, so callers (and the bindings) can pass
/// the original object through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgSource {
    /// Index into the positional arguments.
    Positional(usize),
    /// Index into the keyword arguments.
    Keyword(usize),
}

impl ArgSource {
    /// The value this source points at.
    pub fn get<'v>(self, args: &'v [Value], kwargs: &'v [(String, Value)]) -> Option<&'v Value> {
        match self {
            ArgSource::Positional(i) => args.get(i),
            ArgSource::Keyword(i) => kwargs.get(i).map(|(_, v)| v),
        }
    }
}

/// A Python function signature with positional-or-keyword parameters,
/// optionally followed by `*args`.
#[derive(Debug, Clone, Copy)]
pub struct Signature<'a> {
    /// The qualified name used in messages (`escape`, `error.__init__`).
    pub name: &'a str,
    /// Parameter names, excluding any implicit `self`.
    pub params: &'a [&'a str],
    /// How many leading parameters have no default.
    pub required: usize,
    /// Whether extra positionals are collected (`*args`).
    pub varargs: bool,
    /// Whether the function is a method whose `self` counts in messages.
    pub method: bool,
}

/// The result of binding: one optional source per parameter, and the extra
/// positionals collected by `*args`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bound {
    /// One entry per parameter; `None` means "use the default".
    pub slots: Slots,
    /// Positional indices beyond the named parameters (only with `varargs`).
    pub extra: std::ops::Range<usize>,
}

impl Bound {
    /// The value bound to parameter `i`, if given.
    pub fn value<'v>(
        &self,
        i: usize,
        args: &'v [Value],
        kwargs: &'v [(String, Value)],
    ) -> Option<&'v Value> {
        self.slots.get(i).copied().flatten()?.get(args, kwargs)
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

/// `'a'`, `'a' and 'b'`, `'a', 'b', and 'c'` (CPython's `format_missing`).
fn name_list(names: &[&str]) -> String {
    let quoted: Vec<String> = names.iter().map(|n| format!("'{n}'")).collect();
    match quoted.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [a, b] => format!("{a} and {b}"),
        [init @ .., last] => format!("{}, and {last}", init.join(", ")),
    }
}

/// Arguments bound to a [`Signature`].
#[derive(Debug)]
pub struct Call<'v> {
    args: &'v [Value],
    kwargs: &'v [(String, Value)],
    /// Which argument fills each parameter.
    pub bound: Bound,
}

impl<'v> Call<'v> {
    /// The argument bound to parameter `i`, if given.
    #[must_use]
    pub fn get(&self, i: usize) -> Option<&'v Value> {
        self.bound.value(i, self.args, self.kwargs)
    }

    /// The argument bound to required parameter `i` (binding fails when a
    /// required argument is missing, so it is always there).
    #[must_use]
    pub fn required(&self, i: usize) -> &'v Value {
        const NONE: &Value = &Value::None;
        let value = self.get(i);
        debug_assert!(value.is_some(), "required parameter {i} is unbound");
        value.unwrap_or(NONE)
    }

    /// The positionals collected by `*args`.
    #[must_use]
    pub fn extra(&self) -> &'v [Value] {
        self.args.get(self.bound.extra.clone()).unwrap_or(&[])
    }
}

impl<'a> Signature<'a> {
    /// A plain function signature: no `*args`, no `self`.
    #[must_use]
    pub const fn new(name: &'a str, params: &'a [&'a str], required: usize) -> Self {
        Self {
            name,
            params,
            required,
            varargs: false,
            method: false,
        }
    }

    /// The shim's order: [`check_builtins`] on every argument, then
    /// [`Signature::bind`].
    ///
    /// # Errors
    /// `TypeError` from either step.
    pub fn check_and_bind<'v>(
        &self,
        args: &'v [Value],
        kwargs: &'v [(String, Value)],
    ) -> Result<Call<'v>> {
        check_builtins(args, kwargs)?;
        let bound = self.bind(args, kwargs)?;
        Ok(Call {
            args,
            kwargs,
            bound,
        })
    }

    /// Binds arguments exactly as CPython's `initialize_locals` does, in its
    /// order: keywords (unexpected / multiple values), then too many
    /// positionals, then missing required arguments.
    ///
    /// # Errors
    /// `TypeError` with CPython's message.
    pub fn bind(&self, args: &[Value], kwargs: &[(String, Value)]) -> Result<Bound> {
        let n = self.params.len();
        let mut slots: Slots = smallvec::smallvec![None; n];
        let taken = args.len().min(n);
        for (i, slot) in slots.iter_mut().enumerate().take(taken) {
            *slot = Some(ArgSource::Positional(i));
        }
        for (k, (key, _)) in kwargs.iter().enumerate() {
            let Some(i) = self.params.iter().position(|p| p == key) else {
                return Err(Error::Type(format!(
                    "{}() got an unexpected keyword argument '{key}'",
                    self.name
                )));
            };
            if let Some(slot) = slots.get_mut(i) {
                if slot.is_some() {
                    return Err(Error::Type(format!(
                        "{}() got multiple values for argument '{key}'",
                        self.name
                    )));
                }
                *slot = Some(ArgSource::Keyword(k));
            }
        }
        if args.len() > n && !self.varargs {
            return Err(self.too_many(args.len()));
        }
        let missing: SmallVec<[&str; 6]> = self
            .params
            .iter()
            .zip(&slots)
            .take(self.required)
            .filter(|(_, s)| s.is_none())
            .map(|(p, _)| *p)
            .collect();
        if !missing.is_empty() {
            return Err(Error::Type(format!(
                "{}() missing {} required positional argument{}: {}",
                self.name,
                missing.len(),
                plural(missing.len()),
                name_list(&missing)
            )));
        }
        Ok(Bound {
            slots,
            extra: taken..args.len().max(taken),
        })
    }

    fn too_many(&self, given: usize) -> Error {
        let offset = usize::from(self.method);
        let (min, max, given) = (
            self.required + offset,
            self.params.len() + offset,
            given + offset,
        );
        let takes = if min == max {
            format!("{max} positional argument{}", plural(max))
        } else {
            format!("from {min} to {max} positional arguments")
        };
        let was = if given == 1 { "was" } else { "were" };
        Error::Type(format!(
            "{}() takes {takes} but {given} {was} given",
            self.name
        ))
    }
}

/// The shim's `_check_builtin`, applied to every positional and then every
/// keyword value, depth-first.
///
/// # Errors
/// `TypeError: pc_re supports built-in types only, not NAME`.
pub fn check_builtins(args: &[Value], kwargs: &[(String, Value)]) -> Result<()> {
    args.iter()
        .chain(kwargs.iter().map(|(_, v)| v))
        .try_for_each(check_builtin)
}

fn check_builtin(value: &Value) -> Result<()> {
    match value {
        Value::Decimal(_) | Value::Other(_) => Err(Error::Type(format!(
            "pc_re supports built-in types only, not {}",
            value.type_name()
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

/// The shim's `_text(value, what)`: the code points of a `str`.
///
/// # Errors
/// `TypeError: WHAT must be str, not TYPE`.
pub fn text<'v>(value: &'v Value, what: &str) -> Result<&'v [u32]> {
    match value {
        Value::Str(s) => Ok(s),
        other => Err(Error::Type(format!(
            "{what} must be str, not {}",
            other.type_name()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_bigint::BigInt;

    const ERROR: Signature<'static> = Signature {
        name: "error.__init__",
        params: &["msg", "pattern", "pos"],
        required: 1,
        varargs: false,
        method: true,
    };
    const ESCAPE: Signature<'static> = Signature {
        name: "escape",
        params: &["pattern"],
        required: 1,
        varargs: false,
        method: false,
    };

    fn int(i: i64) -> Value {
        Value::Int(BigInt::from(i))
    }

    fn msg(r: Result<Bound>) -> String {
        r.err().map(|e| e.to_string()).unwrap_or_default()
    }

    #[test]
    fn messages() {
        assert_eq!(
            msg(ERROR.bind(&[], &[])),
            "error.__init__() missing 1 required positional argument: 'msg'"
        );
        assert_eq!(
            msg(ERROR.bind(&[int(1), int(2), int(3), int(4)], &[])),
            "error.__init__() takes from 2 to 4 positional arguments but 5 were given"
        );
        assert_eq!(
            msg(ERROR.bind(&[int(1)], &[("msg".into(), int(2))])),
            "error.__init__() got multiple values for argument 'msg'"
        );
        assert_eq!(
            msg(ESCAPE.bind(&[int(1), int(2)], &[("x".into(), int(3))])),
            "escape() got an unexpected keyword argument 'x'"
        );
        assert_eq!(
            msg(ESCAPE.bind(&[int(1), int(2)], &[])),
            "escape() takes 1 positional argument but 2 were given"
        );
        assert_eq!(name_list(&["a", "b", "c"]), "'a', 'b', and 'c'");
    }

    #[test]
    fn binds() {
        let kw = [("pos".to_owned(), int(3))];
        let b = ERROR.bind(&[int(1)], &kw);
        assert_eq!(
            b.ok().map(|b| b.slots.to_vec()),
            Some(vec![
                Some(ArgSource::Positional(0)),
                None,
                Some(ArgSource::Keyword(0))
            ])
        );
    }
}
