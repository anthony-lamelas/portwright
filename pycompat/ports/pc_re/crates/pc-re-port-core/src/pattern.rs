//! Compiled patterns (`re.Pattern`): `_compiler.compile` plus `_sre.compile`.

use indexmap::IndexMap;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

use crate::compiler;
use crate::debug;
use crate::error::{Error, Result};
use crate::flags;
use crate::frames;
use crate::parser;
use crate::pyrepr;
use crate::text::{push_str, PyStr};

/// A compiled pattern, as `_sre.compile` builds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    /// `pattern.pattern`.
    pub(crate) pattern: PyStr,
    /// `pattern.flags` (a C int): the caller's flags, the inline flags and
    /// `UNICODE` unless `ASCII`.
    pub(crate) flags: i32,
    /// The sre program the matcher interprets.
    pub(crate) code: Vec<u32>,
    /// `pattern.groups`: number of capturing groups.
    pub(crate) groups: usize,
    /// `pattern.groupindex`, in definition order.
    pub(crate) groupindex: IndexMap<PyStr, usize>,
    /// Group number to name (`indexgroup`), index 0 unused.
    pub(crate) indexgroup: Vec<Option<PyStr>>,
    /// Single-character items one of which every match starts with
    /// ([`crate::engine::first_units`]), to skip positions in searches.
    pub(crate) first: Option<crate::engine::FirstSet>,
    /// The program's charsets with their ASCII answers precomputed.
    pub(crate) sets: crate::engine::AsciiSets,
}

/// Flag names in the order `pattern_repr` lists them.
const FLAG_NAMES: [(&str, u32); 9] = [
    ("re.TEMPLATE", flags::TEMPLATE),
    ("re.IGNORECASE", flags::IGNORECASE),
    ("re.LOCALE", flags::LOCALE),
    ("re.MULTILINE", flags::MULTILINE),
    ("re.DOTALL", flags::DOTALL),
    ("re.UNICODE", flags::UNICODE),
    ("re.VERBOSE", flags::VERBOSE),
    ("re.DEBUG", flags::DEBUG),
    ("re.ASCII", flags::ASCII),
];

impl Pattern {
    /// Compiles a str pattern with the caller's (already validated) flags.
    ///
    /// # Errors
    /// `re.error`, `ValueError`, `OverflowError` and `RecursionError` exactly as
    /// `re.compile` raises them.
    pub fn compile(pattern: &[u32], flags_value: &BigInt) -> Result<Self> {
        let low = (flags_value & BigInt::from(0xFFFF_u32))
            .to_u32()
            .unwrap_or(0);
        let parsed = parser::parse_pattern(pattern, low)?;
        let debugging = low & flags::DEBUG != 0;
        if debugging {
            // `_parser.parse` calls `p.dump()` at depth 0.
            frames::frame(debug::dump_depth(&parsed.data, 0))?;
        }
        let state = &parsed.state;
        let code = compiler::code(&parsed.data, state.flags, &state.groupwidths)?;
        if debugging {
            frames::frame(debug::dis_depth(&code))?;
        }
        let combined = flags_value | BigInt::from(state.flags);
        let flags = combined
            .to_i32()
            .ok_or_else(|| Error::Overflow("Python int too large to convert to C int".into()))?;
        let mut indexgroup = vec![None; state.groups()];
        for (name, &i) in &state.groupdict {
            if let Some(slot) = indexgroup.get_mut(i) {
                *slot = Some(name.clone());
            }
        }
        let first = crate::engine::first_units(&code);
        let sets = crate::engine::AsciiSets::new(&code);
        Ok(Pattern {
            pattern: pattern.to_vec(),
            flags,
            first,
            sets,
            code,
            groups: state.groups() - 1,
            groupindex: state.groupdict.clone(),
            indexgroup,
        })
    }

    /// `pattern.pattern`.
    #[must_use]
    pub fn pattern(&self) -> &[u32] {
        &self.pattern
    }

    /// `pattern.flags`.
    #[must_use]
    pub fn flags(&self) -> i32 {
        self.flags
    }

    /// `pattern.groups`.
    #[must_use]
    pub fn groups(&self) -> usize {
        self.groups
    }

    /// `pattern.groupindex`, in definition order.
    #[must_use]
    pub fn groupindex(&self) -> &IndexMap<PyStr, usize> {
        &self.groupindex
    }

    /// `repr(pattern)`: `re.compile(%.200R[, FLAGS])`.
    pub fn repr(&self) -> PyStr {
        let mut out = crate::text::cps("re.compile(");
        out.extend(pyrepr::str_repr(&self.pattern).into_iter().take(200));
        // Two's complement bits of the C int, as `%x` prints them.
        let mut bits = u32::from_ne_bytes(self.flags.to_ne_bytes());
        if bits & (flags::LOCALE | flags::UNICODE | flags::ASCII) == flags::UNICODE {
            bits &= !flags::UNICODE;
        }
        let mut items: Vec<String> = Vec::new();
        for (name, value) in FLAG_NAMES {
            if bits & value != 0 {
                items.push(name.to_owned());
                bits &= !value;
            }
        }
        if bits != 0 {
            items.push(format!("0x{bits:x}"));
        }
        if !items.is_empty() {
            push_str(&mut out, ", ");
            push_str(&mut out, &items.join("|"));
        }
        out.push(u32::from(')'));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::{cps, to_string_lossy};

    fn repr(p: &str, f: i64) -> String {
        match Pattern::compile(&cps(p), &BigInt::from(f)) {
            Ok(p) => to_string_lossy(&p.repr()),
            Err(e) => format!("!{e}"),
        }
    }

    #[test]
    fn reprs() {
        assert_eq!(repr("a", 0), "re.compile('a')");
        assert_eq!(repr("a", -(1 << 31)), "re.compile('a', 0x80000000)");
        assert_eq!(
            repr("a", 512 | 1 | 2),
            "re.compile('a', re.TEMPLATE|re.IGNORECASE|0x200)"
        );
        assert_eq!(
            repr("a", 256 | 2),
            "re.compile('a', re.IGNORECASE|re.ASCII)"
        );
        assert_eq!(
            repr("a", 1 << 40),
            "!Python int too large to convert to C int"
        );
        assert_eq!(repr("a", 4), "!cannot use LOCALE flag with a str pattern");
        assert_eq!(
            repr("a{2}", 1),
            "!internal: unsupported template operator MAX_REPEAT"
        );
        let long = "a".repeat(300);
        assert_eq!(repr(&long, 0).chars().count(), 212);
    }
}
