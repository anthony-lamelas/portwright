//! Thin Python layer over isodate-port-core. Python-specific behavior belongs here.
//!
//! Shared and frozen. Builds the package tree `isodate.<submodule>` and lets each task
//! register its names through `register(&Modules)` in its own file. After every task
//! has registered, the root module re-exports the names `isodate/__init__.py` imports.
//!
//! Submodules are attributes of the root only: they are *not* in `sys.modules` (the
//! port may not import `sys`). Anything pickle must find by name (`__reduce__`
//! callables) is therefore registered on the root module, whose `__module__` is
//! `isodate`; see decisions.md.
#![forbid(unsafe_code)]

use pyo3::prelude::*;
use pyo3::types::PyList;

pub mod duration;
pub mod errors;
pub mod parse;
mod portwright_submodules;
pub mod strf;
pub mod text;
pub mod tz;

/// The package's modules, by their `isodate.<name>` submodule names.
pub struct Modules<'py> {
    /// `isodate`.
    pub root: Bound<'py, PyModule>,
    /// `isodate.isoerror`.
    pub isoerror: Bound<'py, PyModule>,
    /// `isodate.version`.
    pub version: Bound<'py, PyModule>,
    /// `isodate.tzinfo`.
    pub tzinfo: Bound<'py, PyModule>,
    /// `isodate.isotzinfo`.
    pub isotzinfo: Bound<'py, PyModule>,
    /// `isodate.duration`.
    pub duration: Bound<'py, PyModule>,
    /// `isodate.isostrf`.
    pub isostrf: Bound<'py, PyModule>,
    /// `isodate.isodates`.
    pub isodates: Bound<'py, PyModule>,
    /// `isodate.isotime`.
    pub isotime: Bound<'py, PyModule>,
    /// `isodate.isodatetime`.
    pub isodatetime: Bound<'py, PyModule>,
    /// `isodate.isoduration`.
    pub isoduration: Bound<'py, PyModule>,
}

/// `isodate.__all__`, in the original's order. Each is copied from the submodule that
/// defines it (`(submodule, name)`), when a task has registered it.
const EXPORTS: &[(&str, &str)] = &[
    ("isodates", "parse_date"),
    ("isodates", "date_isoformat"),
    ("isotime", "parse_time"),
    ("isotime", "time_isoformat"),
    ("isodatetime", "parse_datetime"),
    ("isodatetime", "datetime_isoformat"),
    ("isoduration", "parse_duration"),
    ("isoduration", "duration_isoformat"),
    ("isoerror", "ISO8601Error"),
    ("isotzinfo", "parse_tzinfo"),
    ("isotzinfo", "tz_isoformat"),
    ("tzinfo", "UTC"),
    ("tzinfo", "FixedOffset"),
    ("tzinfo", "LOCAL"),
    ("duration", "Duration"),
    ("isostrf", "strftime"),
    ("isostrf", "DATE_BAS_COMPLETE"),
    ("isostrf", "DATE_BAS_ORD_COMPLETE"),
    ("isostrf", "DATE_BAS_WEEK"),
    ("isostrf", "DATE_BAS_WEEK_COMPLETE"),
    ("isostrf", "DATE_CENTURY"),
    ("isostrf", "DATE_EXT_COMPLETE"),
    ("isostrf", "DATE_EXT_ORD_COMPLETE"),
    ("isostrf", "DATE_EXT_WEEK"),
    ("isostrf", "DATE_EXT_WEEK_COMPLETE"),
    ("isostrf", "DATE_YEAR"),
    ("isostrf", "DATE_BAS_MONTH"),
    ("isostrf", "DATE_EXT_MONTH"),
    ("isostrf", "TIME_BAS_COMPLETE"),
    ("isostrf", "TIME_BAS_MINUTE"),
    ("isostrf", "TIME_EXT_COMPLETE"),
    ("isostrf", "TIME_EXT_MINUTE"),
    ("isostrf", "TIME_HOUR"),
    ("isostrf", "TZ_BAS"),
    ("isostrf", "TZ_EXT"),
    ("isostrf", "TZ_HOUR"),
    ("isostrf", "DT_BAS_COMPLETE"),
    ("isostrf", "DT_EXT_COMPLETE"),
    ("isostrf", "DT_BAS_ORD_COMPLETE"),
    ("isostrf", "DT_EXT_ORD_COMPLETE"),
    ("isostrf", "DT_BAS_WEEK_COMPLETE"),
    ("isostrf", "DT_EXT_WEEK_COMPLETE"),
    ("isostrf", "D_DEFAULT"),
    ("isostrf", "D_WEEK"),
    ("isostrf", "D_ALT_EXT"),
    ("isostrf", "D_ALT_BAS"),
    ("isostrf", "D_ALT_BAS_ORD"),
    ("isostrf", "D_ALT_EXT_ORD"),
];

/// The package's submodule names, appended to `__all__`.
const SUBMODULES: &[&str] = &[
    "isoerror",
    "version",
    "tzinfo",
    "isotzinfo",
    "duration",
    "isostrf",
    "isodates",
    "isotime",
    "isodatetime",
    "isoduration",
];

/// `isodate.version.version`.
const VERSION: &str = "0.7.2";

impl<'py> Modules<'py> {
    fn new(root: &Bound<'py, PyModule>) -> PyResult<Self> {
        let py = root.py();
        let sub = |name: &str| -> PyResult<Bound<'py, PyModule>> {
            let module = PyModule::new(py, &format!("isodate.{name}"))?;
            root.add_submodule(&module)?;
            Ok(module)
        };
        Ok(Self {
            root: root.clone(),
            isoerror: sub("isoerror")?,
            version: sub("version")?,
            tzinfo: sub("tzinfo")?,
            isotzinfo: sub("isotzinfo")?,
            duration: sub("duration")?,
            isostrf: sub("isostrf")?,
            isodates: sub("isodates")?,
            isotime: sub("isotime")?,
            isodatetime: sub("isodatetime")?,
            isoduration: sub("isoduration")?,
        })
    }

    /// The submodule called `name`.
    pub fn get(&self, name: &str) -> Option<&Bound<'py, PyModule>> {
        Some(match name {
            "isoerror" => &self.isoerror,
            "version" => &self.version,
            "tzinfo" => &self.tzinfo,
            "isotzinfo" => &self.isotzinfo,
            "duration" => &self.duration,
            "isostrf" => &self.isostrf,
            "isodates" => &self.isodates,
            "isotime" => &self.isotime,
            "isodatetime" => &self.isodatetime,
            "isoduration" => &self.isoduration,
            _ => return None,
        })
    }
}

fn register_version(modules: &Modules<'_>) -> PyResult<()> {
    let py = modules.root.py();
    let version = &modules.version;
    version.add("__version__", VERSION)?;
    version.add("version", VERSION)?;
    let tuple = (0, 7, 2).into_pyobject(py)?;
    version.add("__version_tuple__", &tuple)?;
    version.add("version_tuple", &tuple)?;
    modules.root.add("__version__", VERSION)
}

#[pymodule(name = "isodate")]
fn module_init(root: &Bound<'_, PyModule>) -> PyResult<()> {
    let modules = Modules::new(root)?;
    register_version(&modules)?;
    errors::register(&modules)?;
    tz::register(&modules)?;
    duration::register(&modules)?;
    strf::register(&modules)?;
    parse::register(&modules)?;

    let py = root.py();
    let all = PyList::empty(py);
    for &(module, name) in EXPORTS {
        let Some(source) = modules.get(module) else {
            continue;
        };
        // Names a task hasn't registered yet are skipped, so the package always imports.
        if let Some(value) = source.dict().get_item(name)? {
            root.add(name, value)?;
            all.append(name)?;
        }
    }
    all.append("__version__")?;
    // Submodule names, so maturin's `from .isodate import *` exposes `isodate.tzinfo` etc.
    for name in SUBMODULES {
        all.append(name)?;
    }
    root.add("__all__", all)?;
    portwright_submodules::register_submodules(root)?;
    Ok(())
}
