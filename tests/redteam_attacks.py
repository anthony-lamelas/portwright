"""The red-team's attacks on the integrity gate and strict bindings, kept as data (#88).

Each attack starts from the clean toy port of test_integrity_strict.py, makes one
change and names the modes it was tried in. test_redteam_static.py runs them all.
"""

from __future__ import annotations

import shutil
import textwrap
from pathlib import Path

from test_integrity_strict import BINDINGS, CLEAN_BINDINGS, RUST

CORE = "crates/textkit-core"


def make_port(dest: Path) -> Path:
    if dest.exists():
        shutil.rmtree(dest)
    shutil.copytree(RUST, dest, ignore=shutil.ignore_patterns("target"))
    (dest / BINDINGS / "src" / "lib.rs").write_text(CLEAN_BINDINGS)
    return dest


def append(port: Path, relative: str, code: str) -> None:
    path = port / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    old = path.read_text() if path.exists() else ""
    path.write_text(old + "\n" + textwrap.dedent(code).strip("\n") + "\n")


def write(port: Path, relative: str, code: str) -> None:
    path = port / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(textwrap.dedent(code).lstrip("\n"))


def replace(port: Path, relative: str, old: str, new: str) -> None:
    path = port / relative
    text = path.read_text()
    assert old in text, (relative, old)
    path.write_text(text.replace(old, new, 1))


LIB = f"{BINDINGS}/src/lib.rs"
ATTACKS: dict[str, tuple[str, callable, tuple[str, ...]]] = {}


def attack(aid: str, title: str, modes=("strict", "lenient")):
    def register(fn):
        ATTACKS[aid] = (title, fn, modes)
        return fn
    return register


# ------------------------------------------------------------------ running Python source

@attack("A01", "py.run on a Python token not named `py` runs `import textkit`")
def _(port):
    append(port, LIB, r'''
    fn cheat<'py>(gil: Python<'py>, text: &str) -> PyResult<String> {
        let scope = PyDict::new(gil);
        scope.set_item("text", text)?;
        gil.run(c"import textkit as t\nout = t.slug(text)", None, Some(&scope))?;
        let out: Bound<'py, PyDict> = scope;
        out.get_item("out")?.unwrap().extract()
    }
    ''')


@attack("A02", "`py . eval(...)` with spaces around the dot")
def _(port):
    append(port, LIB, r'''
    fn cheat<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        py . eval(c"__import__('textkit').slug('A b')", None, None)
    }
    ''')


@attack("A03", "`Python :: run(py, ...)` / `<Python>::eval(py, ...)` (path call with spaces / qualified)")
def _(port):
    append(port, LIB, r'''
    fn cheat<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        Python :: run(py, c"import textkit", None, None)?;
        <Python>::eval(py, c"__import__('textkit').slug('A b')", None, None)
    }
    ''')


@attack("A04", "PyCode::compile(...).run(...) (what py.run is made of)")
def _(port):
    append(port, LIB, r'''
    use pyo3::types::{PyCode, PyCodeInput, PyCodeMethods};
    fn cheat<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let scope = PyDict::new(py);
        PyCode::compile(py, c"__import__('textkit').slug('A b')", c"x", PyCodeInput::Eval)?.run(Some(&scope), None)
    }
    ''')


@attack("A05", "py_run! spelled `py_run !`, as `py_run_impl!`, or renamed with `use ... as`")
def _(port):
    append(port, LIB, r'''
    use pyo3::py_run as prun;
    fn cheat(py: Python<'_>) {
        let d = PyDict::new(py);
        pyo3::py_run_impl!(py, *d, "import textkit\nout = textkit.slug('A b')");
        pyo3::py_run !(py, *d, "import textkit");
        prun!(py, *d, "import textkit");
    }
    ''')


# ------------------------------------------------------------------ importing the original

@attack("A06", "import_exception!(textkit, Class) imports the original and returns one of its classes")
def _(port):
    append(port, LIB, r'''
    pyo3::import_exception!(textkit, Stack);
    fn cheat<'py>(py: Python<'py>) -> Bound<'py, pyo3::types::PyType> {
        py.get_type::<Stack>()
    }
    ''')


@attack("A07", "pyo3::impl_::exceptions::ImportedExceptionTypeObject::new(\"textkit\", ..).get(py)")
def _(port):
    append(port, LIB, r'''
    static ORIGINAL: pyo3::impl_::exceptions::ImportedExceptionTypeObject =
        pyo3::impl_::exceptions::ImportedExceptionTypeObject::new("textkit", "Stack");
    fn cheat<'py>(py: Python<'py>) -> &'py Bound<'py, pyo3::types::PyType> {
        ORIGINAL.get(py)
    }
    ''')


@attack("A08", "function pointer to PyModule::import (no `import(` in the text)")
def _(port):
    append(port, LIB, r'''
    fn cheat<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyModule>> {
        let load = PyModule::import::<&str>;
        load(py, "textkit")
    }
    ''')


@attack("A09", "macro_rules! with an `$m:ident` method name: go!(py, import, \"textkit\"), go!(f, call1, ...)")
def _(port):
    append(port, LIB, r'''
    macro_rules! go { ($r:expr, $m:ident $(, $a:expr)*) => { $r.$m($($a),*) }; }
    fn cheat<'py>(py: Python<'py>, text: &str) -> PyResult<String> {
        let module = go!(py, import, "textkit")?;
        let function = go!(module, getattr, "slug")?;
        go!(function, call1, (text,))?.extract()
    }
    ''')


@attack("A10", "const shadowing: the scan keeps the last `const NAME` in the file", modes=("lenient",))
def _(port):
    append(port, LIB, r'''
    mod original {
        use pyo3::prelude::*;
        const NAME: &str = "textkit";
        pub fn cheat<'py>(py: Python<'py>, text: &str) -> PyResult<String> {
            py.import(NAME)?.getattr("slug")?.call1((text,))?.extract()
        }
    }
    mod decoy {
        #[allow(dead_code)]
        const NAME: &str = "builtins";
    }
    ''')


@attack("A11", "const shadowing in strict mode: call_method0(M) with M = \"__repr__\" read as \"encode\"", modes=("strict",))
def _(port):
    append(port, LIB, r'''
    mod real {
        use pyo3::prelude::*;
        const METHOD: &str = "__repr__";
        pub fn cheat<'py>(x: &Bound<'py, PyAny>) -> PyResult<String> {
            x.call_method0(METHOD)?.extract()
        }
    }
    mod decoy {
        #[allow(dead_code)]
        const METHOD: &str = "encode";
    }
    ''')


@attack("A12", "allowed `builtins` import + escaped \"__import__\" name (non-strict)", modes=("lenient",))
def _(port):
    append(port, LIB, r'''
    fn cheat<'py>(py: Python<'py>, text: &str) -> PyResult<String> {
        let import = py.import("builtins")?.getattr("__im\x70ort__")?;
        import.call1(("textkit",))?.getattr("slug")?.call1((text,))?.extract()
    }
    ''')


# ------------------------------------------------------------------ hiding files from the scan

EVIL_RS = r'''
use pyo3::prelude::*;
pub fn cheat<'py>(py: Python<'py>, text: &str) -> PyResult<String> {
    py.import("textkit")?.getattr("slug")?.call1((text,))?.extract()
}
'''


@attack("A13", "#[cfg_attr(all(), path = ...)] to a file in a non-crate folder (scanned, but not strictly)")
def _(port):
    write(port, "shared/evil.rs", '''
    use pyo3::prelude::*;
    pub fn show(x: &Bound<'_, PyAny>) -> PyResult<String> { x.repr()?.extract() }
    ''')
    append(port, LIB, '#[cfg_attr(all(), path = "../../../shared/evil.rs")]\nmod evil;')


@attack("A14", "#[cfg_attr(all(), path = ...)] to a file under the top-level target/ (never scanned)")
def _(port):
    write(port, "target/evil.rs", EVIL_RS)
    append(port, LIB, '#[cfg_attr(all(), path = "../../../target/evil.rs")]\nmod evil;')


@attack("A15", "include! invoked through a macro ident (`$m!($p)`), file under target/")
def _(port):
    write(port, "target/evil.rs", EVIL_RS)
    append(port, LIB, 'macro_rules! pull { ($m:ident, $p:literal) => { $m!($p); }; }\npull!(include, "../../../target/evil.rs");')


@attack("A16", "path dependency on a crate under the top-level fuzz/ folder (Cargo.toml and .rs skipped)")
def _(port):
    write(port, "fuzz/helper/Cargo.toml", '''
    [package]
    name = "helper"
    version = "0.1.0"
    edition = "2021"

    [dependencies]
    pyo3 = { version = "0.29", features = ["chrono"] }
    ''')
    write(port, "fuzz/helper/src/lib.rs", EVIL_RS)
    replace(port, f"{BINDINGS}/Cargo.toml", "[dependencies]\n", '[dependencies]\nhelper = { path = "../../fuzz/helper" }\n')


@attack("A17", "path dependency on a crate inside a nested .git/ folder (walk skips every .git)")
def _(port):
    write(port, f"{BINDINGS}/.git/helper/Cargo.toml", '''
    [package]
    name = "helper"
    version = "0.1.0"
    edition = "2021"

    [dependencies]
    pyo3 = "0.29"
    ''')
    write(port, f"{BINDINGS}/.git/helper/src/lib.rs", EVIL_RS)
    replace(port, f"{BINDINGS}/Cargo.toml", "[dependencies]\n", '[dependencies]\nhelper = { path = ".git/helper" }\n')


@attack("A18", "#[path] to a module file inside .git/ (include scan finds it, but check strictness)")
def _(port):
    write(port, f"{BINDINGS}/src/.git/evil.rs", EVIL_RS)
    append(port, LIB, '#[path = ".git/evil.rs"]\nmod evil;')


# ------------------------------------------------------------------ manifest / features

@attack("A19", "pyo3 = { workspace = true, features = [\"rust_decimal\", \"chrono\"] } (crate-level features dropped)", modes=("strict",))
def _(port):
    replace(port, "Cargo.toml", "[profile.release]", '[workspace.dependencies]\npyo3 = { version = "0.29", features = ["extension-module", "num-bigint"] }\n\n[profile.release]')
    replace(port, f"{BINDINGS}/Cargo.toml",
            'pyo3 = { version = "0.29", features = ["extension-module", "num-bigint"] }',
            'pyo3 = { workspace = true, features = ["rust_decimal", "chrono"] }')


@attack("A20", "[features] default = [\"pyo3?/chrono\"] (weak-dependency syntax)", modes=("strict",))
def _(port):
    append(port, f"{BINDINGS}/Cargo.toml", '[features]\ndefault = ["pyo3?/chrono", "pyo3?/rust_decimal"]')


@attack("A21", "pyo3 renamed (`py = { package = \"pyo3\" }`) + [features] default = [\"py/chrono\"]", modes=("strict",))
def _(port):
    replace(port, f"{BINDINGS}/Cargo.toml",
            'pyo3 = { version = "0.29", features = ["extension-module", "num-bigint"] }',
            'pyo3 = { version = "0.29", features = ["extension-module", "num-bigint"] }\npy = { package = "pyo3", version = "0.29" }')
    append(port, f"{BINDINGS}/Cargo.toml", '[features]\ndefault = ["py/chrono"]')


@attack("A22", "proc-macro crate (strings -> TokenStream) emits py.import(\"textkit\")")
def _(port):
    write(port, "crates/helper-macros/Cargo.toml", '''
    [package]
    name = "helper-macros"
    version = "0.1.0"
    edition = "2021"

    [lib]
    proc-macro = true
    ''')
    write(port, "crates/helper-macros/src/lib.rs", '''
    use proc_macro::TokenStream;
    #[proc_macro]
    pub fn original(_: TokenStream) -> TokenStream {
        "fn cheat<'py>(py: pyo3::Python<'py>) -> pyo3::PyResult<pyo3::Bound<'py, pyo3::types::PyModule>> { py.import(\\"textkit\\") }".parse().unwrap()
    }
    ''')
    replace(port, "Cargo.toml", 'members = ["crates/textkit-core", "crates/textkit-py"]',
            'members = ["crates/textkit-core", "crates/textkit-py", "crates/helper-macros"]')
    replace(port, f"{BINDINGS}/Cargo.toml", "[dependencies]\n", '[dependencies]\nhelper-macros = { path = "../helper-macros" }\n')
    append(port, LIB, "helper_macros::original!();")


# ------------------------------------------------------------------ calling Python without the names

@attack("A23", "ffi::PyTypeObject destructured ({ tp_call, tp_repr, .. }) from get_type_ptr(): call any callable", modes=("strict",))
def _(port):
    append(port, LIB, r'''
    /// Calls `f(*args)` through the type's tp_call slot, without .call().
    fn invoke<'py>(f: &Bound<'py, PyAny>, args: &Bound<'py, PyTuple>) -> PyResult<Bound<'py, PyAny>> {
        let ty: *mut ffi::PyTypeObject = f.get_type_ptr();
        let ffi::PyTypeObject { tp_call, .. } = unsafe { std::ptr::read(ty) };
        let slot = tp_call.ok_or_else(|| PyTypeError::new_err("not callable"))?;
        unsafe { Bound::from_owned_ptr_or_err(f.py(), slot(f.as_ptr(), args.as_ptr(), std::ptr::null_mut())) }
    }
    /// repr() through the tp_repr slot.
    fn show<'py>(x: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
        let ffi::PyTypeObject { tp_repr, .. } = unsafe { std::ptr::read(x.get_type_ptr()) };
        unsafe { Bound::from_owned_ptr_or_err(x.py(), tp_repr.unwrap()(x.as_ptr())) }
    }
    ''')


@attack("A24", "raw pointer arithmetic: read ob_type and a slot by offset, transmute to extern \"C\" fn", modes=("strict",))
def _(port):
    append(port, LIB, r'''
    type ReprFn = unsafe extern "C" fn(*mut u8) -> *mut u8;
    fn show<'py>(x: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
        unsafe {
            let object = x.as_ptr() as *const *const usize;
            let ty = *object.add(1);                   // ob_type
            let slot = *ty.add(11) as *const ();       // tp_repr
            let repr: ReprFn = std::mem::transmute(slot);
            Bound::from_owned_ptr_or_err(x.py(), repr(x.as_ptr() as *mut u8) as *mut _)
        }
    }
    ''')


@attack("A25", "Display through a generic: fn show(t: impl Display) -> format!(\"{t}\"); show(obj)", modes=("strict",))
def _(port):
    append(port, LIB, r'''
    fn show(t: impl std::fmt::Display) -> String { format!("{t}") }
    fn as_text<T: ToString>(t: &T) -> String { t.to_string() }
    fn cheat(x: &Bound<'_, PyAny>) -> (String, String) { (show(x), as_text(x)) }
    ''')


@attack("A26", "Display/Debug via a generic helper in the core crate (`pub fn render<T: Display>`)", modes=("strict",))
def _(port):
    append(port, f"{CORE}/src/lib.rs", r'''
    pub fn render<T: core::fmt::Display>(value: T) -> String { value.to_string() }
    pub fn render_debug<T: core::fmt::Debug>(value: T) -> String { format!("{value:?}") }
    ''')
    append(port, LIB, r'''
    fn cheat(x: &Bound<'_, PyAny>) -> (String, String) { (textkit_core::render(x), textkit_core::render_debug(x)) }
    ''')


@attack("A27", "untyped bindings: closures / for-loops / match arms hide the receiver (.to_string(), .add(), .eq())", modes=("strict",))
def _(port):
    append(port, LIB, r'''
    fn cheat<'py>(args: &Bound<'py, PyTuple>) -> PyResult<Vec<String>> {
        let words: Vec<String> = args.iter().map(|item| item.to_string()).collect();
        for item in args.iter() {
            let _sum = item.add(&item)?;      // __add__
            let _same = item.eq(&item)?;      // __eq__
            let _size = item.len()?;          // __len__
        }
        Ok(words)
    }
    ''')


@attack("A28", "parenthesised / as_ref receiver: format!(\"{}\", (x)), x.as_ref().to_string()", modes=("strict",))
def _(port):
    append(port, LIB, r'''
    fn cheat(x: &Bound<'_, PyAny>) -> (String, String, String) {
        let a = format!("{}", (x));
        let b = x.as_ref().to_string();
        let c = format!("{v:>8}", v = (x));
        (a, b, c)
    }
    ''')


@attack("A29", "PyErr Display (str(exception)) from a match arm", modes=("strict",))
def _(port):
    append(port, LIB, r'''
    fn cheat(x: &Bound<'_, PyAny>) -> String {
        match x.extract::<i64>() { Ok(v) => v.to_string(), Err(error) => error.to_string() }
    }
    ''')


@attack("A30", "pyo3::types methods that dispatch but aren't denied: PyDateTime::from_timestamp, PyList::sort, PyType::is_subclass, complex `+`", modes=("strict",))
def _(port):
    append(port, LIB, r'''
    use pyo3::types::{PyComplex, PyDateTime, PyList, PyTzInfo, PyType};
    fn cheat<'py>(py: Python<'py>, list: &Bound<'py, PyList>, ty: &Bound<'py, PyType>, tz: &Bound<'py, PyTzInfo>,
                   other: &Bound<'py, PyAny>, z: Bound<'py, PyComplex>) -> PyResult<()> {
        let _when = PyDateTime::from_timestamp(py, 1.0e9, Some(tz))?;   // datetime.fromtimestamp + tz.fromutc()
        list.sort()?;                                                  // element __lt__
        let _sub = ty.is_subclass(other)?;                             // __subclasscheck__
        let _sum = z.clone() + &z;                                     // CPython complex arithmetic
        Ok(())
    }
    ''')


# ------------------------------------------------------------------ expected to be caught (controls)

@attack("C01", "control: plain py.run / py.import(\"textkit\") / x.repr()")
def _(port):
    append(port, LIB, r'''
    fn cheat<'py>(py: Python<'py>, x: &Bound<'py, PyAny>) -> PyResult<()> {
        py.run(c"import textkit", None, None)?;
        let _ = py.import("textkit")?;
        let _ = x.repr()?;
        Ok(())
    }
    ''')


@attack("C02", "control: ffi::PyTypeObject slot read with `.tp_call` / ob_type field", modes=("strict",))
def _(port):
    append(port, LIB, r'''
    fn cheat(x: &Bound<'_, PyAny>) { let _ = unsafe { (*x.get_type_ptr()).tp_call }; }
    ''')


@attack("C03", "control: std::ffi rename tricks (`use pyo3 as std`, `mod std`, `use std::ffi::OsString`)", modes=("strict",))
def _(port):
    append(port, LIB, r'''
    mod std2 { pub use pyo3::ffi::PyObject_Repr as CStr; }
    use std::ffi::{CString, OsString};
    ''')


@attack("C04", "control: Bound::call1 / <_>::repr path-form, PyAnyMethods UFCS", modes=("strict",))
def _(port):
    append(port, LIB, r'''
    fn cheat<'py>(x: &Bound<'py, PyAny>) {
        let f = Bound::<PyAny>::call1::<(i32,)>;
        let _ = f(x, (1,));
        let _ = <Bound<'py, PyAny> as PyAnyMethods>::repr(x);
    }
    ''')


@attack("C05", "control: lexer — `\"//\"`, `'\"'`, r#\"...\"#, nested /* */ around x.repr()", modes=("strict",))
def _(port):
    append(port, LIB, r'''
    fn cheat(x: &Bound<'_, PyAny>) {
        let _q = '"'; let _s = r#"/*"#; /* /* */ */ let _ = x.repr(); let _t = "\"//";
    }
    ''')


@attack("C06", "control: [lib] proc-macro crate in strict mode; renamed pyo3-ffi dependency", modes=("strict",))
def _(port):
    replace(port, f"{BINDINGS}/Cargo.toml", "[dependencies]\n", '[dependencies]\nraw = { package = "pyo3-ffi", version = "0.29" }\n')


@attack("C07", "control: concat!/escaped import name, include_str!(\"x.py\")", modes=("lenient",))
def _(port):
    append(port, LIB, r'''
    fn cheat<'py>(py: Python<'py>) -> PyResult<()> {
        let _ = py.import("text\u{6b}it")?;
        let _ = py.import(concat!("text", "kit"))?;
        Ok(())
    }
    ''')


@attack("C08", "control: Python file anywhere in the port; .cargo/config.toml", modes=("lenient",))
def _(port):
    write(port, "crates/textkit-py/src/helper.py", "import textkit\n")
    write(port, ".cargo/config.toml", "[build]\nrustflags=[]\n")
