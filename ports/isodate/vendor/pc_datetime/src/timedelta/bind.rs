//! A borrowing front end to [`args::bind`] for the shims' hot paths.

use std::borrow::Cow;

use crate::args::{self, Default, Param};
use crate::error::PyResult;
use crate::{Kwargs, Value};

/// [`args::bind`] without cloning the arguments: one value per parameter, borrowed
/// from `args`/`kwargs`, defaults owned.
///
/// Only calls that `args::bind` accepts take the borrowing path. Anything unusual (a
/// non-built-in value, too many positionals, an unknown, repeated or duplicate keyword,
/// a missing argument) is handed to `args::bind`, which raises the exact error.
///
/// # Errors
/// [`args::bind`]'s.
pub(crate) fn bind_ref<'a>(
    func: &str,
    params: &[Param],
    args: &'a [Value],
    kwargs: &'a Kwargs,
) -> PyResult<Vec<Cow<'a, Value>>> {
    bind_borrowed(params, args, kwargs).map_or_else(
        || {
            args::bind(func, params, args, kwargs)
                .map(|values| values.into_iter().map(Cow::Owned).collect())
        },
        Ok,
    )
}

fn bind_borrowed<'a>(
    params: &[Param],
    args: &'a [Value],
    kwargs: &'a Kwargs,
) -> Option<Vec<Cow<'a, Value>>> {
    if args.len() > params.len() {
        return None;
    }
    let all_builtin = args
        .iter()
        .chain(kwargs.iter().map(|(_, v)| v))
        .all(|v| v.check_builtin().is_ok());
    if !all_builtin {
        return None;
    }
    for (index, (key, _)) in kwargs.iter().enumerate() {
        let position = params.iter().position(|p| *key == p.name)?;
        let repeated = kwargs[..index].iter().any(|(earlier, _)| earlier == key);
        if position < args.len() || repeated {
            return None;
        }
    }
    let mut out = Vec::with_capacity(params.len());
    out.extend(args.iter().map(Cow::Borrowed));
    for param in &params[args.len()..] {
        let value = match kwargs.iter().find(|(key, _)| *key == param.name) {
            Some((_, value)) => Cow::Borrowed(value),
            None => Cow::Owned(match param.default {
                Default::Required => return None,
                Default::None => Value::None,
                Default::Int(n) => Value::int(n),
                Default::Str(s) => Value::str(s),
            }),
        };
        out.push(value);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pystr::PyStr;

    fn same(params: &[Param], args: &[Value], kwargs: &Kwargs) {
        let fast = bind_ref("f", params, args, kwargs)
            .map(|v| v.into_iter().map(Cow::into_owned).collect::<Vec<_>>());
        assert_eq!(fast, args::bind("f", params, args, kwargs));
    }

    #[test]
    fn matches_args_bind() {
        let params = [
            Param::required("a"),
            Param::optional("b", Default::Int(1)),
            Param::optional("c", Default::Str("x")),
            Param::optional("d", Default::None),
        ];
        let one = Value::int(1);
        let kw = |k: &str| (PyStr::from(k), Value::int(7));
        same(&params, &[], &[]);
        same(&params, std::slice::from_ref(&one), &[]);
        same(&params, std::slice::from_ref(&one), &[kw("c")]);
        same(&params, &[], &[kw("a"), kw("d")]);
        same(&params, std::slice::from_ref(&one), &[kw("a")]);
        same(&params, std::slice::from_ref(&one), &[kw("c"), kw("c")]);
        same(&params, std::slice::from_ref(&one), &[kw("z")]);
        same(&params, &vec![one.clone(); 5], &[]);
        same(&params, &[Value::Other("Decimal".into())], &[]);
        same(&params, &[one], &[("b".into(), Value::Other("X".into()))]);
    }
}
