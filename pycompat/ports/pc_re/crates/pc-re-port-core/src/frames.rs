//! CPython's recursion limit, as `re` hits it when compiling deeply nested
//! patterns.
//!
//! Depths count Python frames relative to `_parser._parse_sub(nested=0)`
//! (depth 0). `_parser.parse` and `_compiler._code` run at depth -1 and
//! `_compiler.compile` at -2, so `_compiler._compile(code, p.data)`,
//! `_compile_info`, `SubPattern.dump` and `dis_` all start at depth 0.
//! Every Python-level call counts: helpers such as `SubPattern.__getitem__`
//! (iterating a `SubPattern`), `SubPattern.__len__` (truth tests),
//! `_NamedIntConstant.__repr__` (printing opcodes) and `error.__init__`.

use crate::error::{Error, Result};

/// Python frames between the shim's caller and `_parse_sub(nested=0)`, plus
/// the harness's own frames: depth 0 is Python depth 10 (worker `<module>`,
/// `main`, `run_request`, the shim's `checked` wrapper and function,
/// `re.search`, `re._compile`, `_compiler.compile`, `_parser.parse`). This
/// approximates CPython's dynamic stack depth for that caller.
const FRAME_BASE: usize = 10;
/// The recursion limit: frames up to this depth run, one more raises
/// `RecursionError`.
const FRAME_LIMIT: usize = 1000;

#[cfg(test)]
thread_local! {
    /// Deepest frame checked so far (tests compare it with CPython's).
    pub static DEEPEST: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Raises `RecursionError` where CPython would run out of frames: a Python
/// frame at relative depth `depth` doesn't fit.
///
/// # Errors
/// `RecursionError` when the frame is past the limit.
pub fn frame(depth: usize) -> Result<()> {
    #[cfg(test)]
    DEEPEST.with(|d| d.set(d.get().max(depth)));
    if depth + FRAME_BASE > FRAME_LIMIT {
        Err(Error::Recursion("maximum recursion depth exceeded".into()))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::pattern::Pattern;
    use crate::text::cps;
    use num_bigint::BigInt;

    /// Deepest frame for `pattern`, compiled with `flags`.
    fn deepest(pattern: &str, flags: u32) -> usize {
        super::DEEPEST.with(|d| d.set(0));
        let _outcome = Pattern::compile(&cps(pattern), &BigInt::from(flags));
        super::DEEPEST.with(std::cell::Cell::get)
    }

    /// Expected depths were measured on CPython 3.12 with `sys.setprofile`,
    /// relative to `_parse_sub(nested=0)`.
    #[test]
    fn depths_match_cpython() {
        let cond = format!("({}a{})", "(?(1)".repeat(10), ")*".repeat(10));
        assert_eq!(deepest(&cond, 0), 23, "getwidth through conditionals");
        let groups = format!("{}{}", "(".repeat(10), ")".repeat(10));
        assert_eq!(deepest(&groups, 0), 22, "parser");
        let repeats = format!("(){}a{}", "(?:".repeat(10), "){2}".repeat(10));
        assert_eq!(deepest(&repeats, 0), 23, "compiler");
        let debug = format!("{}a{}", "(".repeat(5), ")*".repeat(5));
        assert_eq!(deepest(&debug, 128), 13, "dump and dis");
        let behind = format!("{}(\\1){}", "(?<=".repeat(5), ")".repeat(5));
        assert_eq!(deepest(&behind, 0), 16, "lookbehind group reference");
        assert_eq!(deepest("(?:(?:(?:\\x41)))", 0), 10, "hex escape");
        assert_eq!(deepest("(?:[\\d-z])", 0), 5, "bad range");
    }
}
