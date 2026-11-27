//! Upstream's public module-level data: `PARSE_RE`, `MONTHS_MAP`, `ALLOWED_TYPES`
//! and the date patterns. Dependents read them (parse_type splits formats with
//! `parse.PARSE_RE`), so they exist here too, built from the core's own tables.

use parse_port_core::convert::MONTHS_MAP;
use parse_port_core::format::{parse_re_spans, ALLOWED_TYPES};
use parse_port_core::pyre::clamp_pos;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PySet, PySlice, PyString};

use super::{ssize, str_of, text_of};

/// Upstream's `PARSE_RE.pattern`.
const PARSE_RE_PATTERN: &str = r"({{|}}|{[\w-]*(?:\.[\w-]+|\[[^]]+])*(?::[^}]+)?})";
/// `re.UNICODE`, the flags of a compiled `str` pattern.
const UNICODE_FLAG: i64 = 32;

const DAYS_PAT: &str = r"(Mon|Tue|Wed|Thu|Fri|Sat|Sun)";
const MONTHS_PAT: &str = r"(Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)";
const TIME_PAT: &str = r"(\d{1,2}:\d{1,2}(:\d{1,2}(\.\d+)?)?)";
const AM_PAT: &str = r"(\s+[AP]M)";
const TZ_PAT: &str = r"(\s+[-+]\d\d?:?\d\d)";

/// `parse.PARSE_RE`. Upstream's is a compiled `re.Pattern`; this one is built on
/// the port's own matcher and offers what dependents use: `split`, `findall` and
/// the pattern's attributes. `match`, `search`, `finditer` and `sub` aren't here.
#[pyclass(module = "parse", name = "FieldPattern", frozen)]
pub struct FieldPattern;

/// `str.__getitem__(s, slice(start, end))`: exact, lone surrogates included, and
/// a plain `str` for a `str` subclass, as `re` returns.
fn piece<'py>(s: &Bound<'py, PyString>, start: usize, end: usize) -> PyResult<Bound<'py, PyAny>> {
    let py = s.py();
    let slice = PySlice::new(py, start.try_into()?, end.try_into()?, 1);
    py.get_type::<PyString>()
        .call_method1("__getitem__", (s, slice))
}

#[pymethods]
impl FieldPattern {
    #[getter]
    fn pattern(&self) -> &'static str {
        PARSE_RE_PATTERN
    }

    #[getter]
    fn flags(&self) -> i64 {
        UNICODE_FLAG
    }

    #[getter]
    fn groups(&self) -> i64 {
        1
    }

    #[getter]
    fn groupindex<'py>(&self, py: Python<'py>) -> Bound<'py, PyDict> {
        PyDict::new(py)
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "re.compile({})",
            PyString::new(py, PARSE_RE_PATTERN).repr()?
        ))
    }

    /// `re.split`: the text between fields, with each field (the one group) between.
    #[pyo3(signature = (string, maxsplit = 0))]
    fn split<'py>(
        &self,
        string: &Bound<'py, PyAny>,
        maxsplit: isize,
    ) -> PyResult<Bound<'py, PyList>> {
        let s = str_of(string)?;
        let text = text_of(string)?;
        let out = PyList::empty(string.py());
        let mut start = 0;
        if maxsplit >= 0 {
            for (n, (a, b)) in parse_re_spans(&text).into_iter().enumerate() {
                if maxsplit > 0 && n >= maxsplit.unsigned_abs() {
                    break;
                }
                out.append(piece(s, start, a)?)?;
                out.append(piece(s, a, b)?)?;
                start = b;
            }
        }
        out.append(piece(s, start, text.len())?)?;
        Ok(out)
    }

    /// `re.findall`: every field in `string[pos:endpos]`.
    #[pyo3(signature = (string, pos = None, endpos = None))]
    fn findall<'py>(
        &self,
        string: &Bound<'py, PyAny>,
        pos: Option<&Bound<'py, PyAny>>,
        endpos: Option<&Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyList>> {
        let s = str_of(string)?;
        let text = text_of(string)?;
        let pos = pos.map(ssize).transpose()?.unwrap_or(0);
        let endpos = endpos.map(ssize).transpose()?.unwrap_or(isize::MAX);
        let (pos, endpos) = clamp_pos(pos, endpos, text.len());
        let out = PyList::empty(string.py());
        let window = text.get(pos..endpos).unwrap_or(&[]);
        for (a, b) in parse_re_spans(window) {
            out.append(piece(s, pos + a, pos + b)?)?;
        }
        Ok(out)
    }
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    // The class itself isn't added to the module, which upstream doesn't have,
    // and it has no constructor, like the compiled pattern it stands in for.
    m.add("PARSE_RE", Bound::new(py, FieldPattern)?)?;
    let months = PyDict::new(py);
    for (name, number) in MONTHS_MAP {
        months.set_item(name, number)?;
    }
    m.add("MONTHS_MAP", months)?;
    let names: Vec<&str> = MONTHS_MAP.iter().map(|(name, _)| *name).collect();
    m.add("ALL_MONTHS_PAT", format!("({})", names.join("|")))?;
    m.add("ALLOWED_TYPES", PySet::new(py, ALLOWED_TYPES)?)?;
    for (name, value) in [
        ("DAYS_PAT", DAYS_PAT),
        ("MONTHS_PAT", MONTHS_PAT),
        ("TIME_PAT", TIME_PAT),
        ("AM_PAT", AM_PAT),
        ("TZ_PAT", TZ_PAT),
    ] {
        m.add(name, value)?;
    }
    Ok(())
}
