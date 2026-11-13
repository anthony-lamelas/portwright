"""Bypasses of the integrity gate a reviewer found, and the finished ports that must still pass.

Python can ship inside a port's wheel, and --strict-bindings used to scan one folder
for a short list of names. Each test below is one way around the old checks.
"""

import shutil
import textwrap
from pathlib import Path

import pytest

from portwright.gates.integrity import integrity_gate, site_problems
from portwright.scaffold import crate_names
from portwright.toolchain import build_python_package, image_available

REPO = Path(__file__).resolve().parents[1]
RUST = Path(__file__).parent / "fixtures" / "textkit_rust"
BINDINGS = "crates/textkit-py"

# Conversion only, written the ways the finished strict ports write it.
CLEAN_BINDINGS = """\
use pyo3::exceptions::PyTypeError;
use pyo3::ffi;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyString, PyTuple};

const ENCODING: &str = "utf-32-le";

/// Code points of a str, with lone surrogates (the URL "https://x" mustn't hide code).
fn points(s: &Bound<'_, PyString>) -> PyResult<Vec<u32>> {
    let encoded = s.call_method1("encode", (ENCODING, "surrogatepass"))?;
    let len = s.len()?;
    let _ = ffi::Py_ssize_t::try_from(len);
    Ok(encoded.cast::<PyBytes>()?.as_bytes().iter().map(|b| u32::from(*b)).collect())
}

fn first(args: &Bound<'_, PyTuple>, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Py<PyAny>> {
    if let Some(kw) = kwargs {
        let _ = kw.values().get_item(0);
    }
    Ok(args.get_item(0)?.unbind())
}

#[pyfunction]
fn slug<'py>(py: Python<'py>, text: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyDict>> {
    let Ok(s) = text.cast_exact::<PyString>() else {
        let name = text.get_type().name()?.to_string_lossy().into_owned();
        return Err(PyTypeError::new_err(format!("text must be str, got {name}")));
    };
    let out = PyDict::new(py);
    out.set_item("points", points(s)?.len())?;
    let message = textkit_core::slug(&[], &[]).len().to_string();
    out.set_item("message", message)?;
    Ok(out)
}

#[pymodule(name = "textkit")]
fn textkit_module(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(slug, module)?)?;
    let _ = first;
    Ok(())
}
"""


def make_port(tmp_path):
    port = tmp_path / "port"
    shutil.copytree(RUST, port, ignore=shutil.ignore_patterns("target"))
    (port / BINDINGS / "src" / "lib.rs").write_text(CLEAN_BINDINGS)
    return port


def check(port, *, strict=True, **kwargs):
    return integrity_gate(
        port, "textkit", core_crate="textkit-core", strict_bindings=strict, bindings_dir=BINDINGS, **kwargs,
    )


def problems(port, **kwargs):
    return check(port, **kwargs).details["problems"]


def add_to_bindings(port, code):
    source = port / BINDINGS / "src" / "lib.rs"
    source.write_text(source.read_text() + "\n" + textwrap.dedent(code) + "\n")


def test_clean_strict_bindings_pass(tmp_path):
    result = check(make_port(tmp_path))
    assert result.passed, result.details


# ---------------------------------------------------------------- 1. Python in the wheel


def test_python_under_a_nested_target_folder_is_found(tmp_path):
    port = make_port(tmp_path)
    hidden = port / BINDINGS / "target" / "textkit" / "__init__.py"
    hidden.parent.mkdir(parents=True)
    hidden.write_text("import re\nescape = re.escape\n")
    assert any("target/textkit/__init__.py: Python source" in p for p in problems(port, strict=False))


def test_only_the_top_level_target_folder_is_skipped(tmp_path):
    port = make_port(tmp_path)
    build = port / "target" / "release" / "build.py"
    build.parent.mkdir(parents=True)
    build.write_text("# build output\n")
    assert check(port).passed


@pytest.mark.parametrize("setting", ['python-source = "target"', 'include = ["extra/*.py"]', 'data = "data"', 'python-packages = ["re"]'])
def test_pyproject_cant_add_python_to_the_wheel(tmp_path, setting):
    port = make_port(tmp_path)
    pyproject = port / BINDINGS / "pyproject.toml"
    pyproject.write_text(pyproject.read_text() + setting + "\n")
    key = setting.split(" =")[0]
    assert any(f"[tool.maturin] {key} isn't allowed" in p for p in problems(port, strict=False))


def test_pyproject_cant_be_removed_or_replaced_by_one_elsewhere(tmp_path):
    port = make_port(tmp_path)
    (port / BINDINGS / "pyproject.toml").unlink()
    (port / "pyproject.toml").write_text(
        '[tool.maturin]\nmanifest-path = "crates/textkit-py/Cargo.toml"\npython-source = "pysrc"\n'
    )
    found = problems(port, strict=False)
    assert any("pyproject.toml is missing" in p for p in found)
    assert any("python-source isn't allowed" in p for p in found)
    assert any("manifest-path isn't allowed" in p for p in found)


def test_cargo_metadata_cant_configure_maturin(tmp_path):
    port = make_port(tmp_path)
    manifest = port / BINDINGS / "Cargo.toml"
    manifest.write_text(manifest.read_text() + '\n[package.metadata.maturin]\npython-source = "pysrc"\n')
    assert any("package.metadata.maturin" in p for p in problems(port, strict=False))


def fake_site(tmp_path, module="textkit"):
    site = tmp_path / "site"
    package = site / module
    package.mkdir(parents=True)
    (package / "__init__.py").write_text(
        f'from .{module} import *\n\n__doc__ = {module}.__doc__\nif hasattr({module}, "__all__"):\n    __all__ = {module}.__all__'
    )
    (package / f"{module}.cpython-312-x86_64-linux-gnu.so").write_bytes(b"\x7fELF")
    info = site / f"{module}-0.1.0.dist-info"
    (info / "sboms").mkdir(parents=True)
    for name in ("METADATA", "WHEEL", "RECORD"):
        (info / name).write_text("x")
    (info / "sboms" / f"{module}.cyclonedx.json").write_text("{}")
    return site


def test_a_wheel_with_only_the_compiled_module_passes(tmp_path):
    assert site_problems(fake_site(tmp_path), "textkit") == []


@pytest.mark.parametrize(
    "extra",
    [
        "textkit/helpers.py",
        "textkit/__pycache__/__init__.cpython-312.pyc",
        "textkit/sub/__init__.py",
        "textkit_extra.pth",
        "re.py",
        "textkit-0.1.0.dist-info/hook.py",
        "textkit-0.1.0.data/purelib/textkit2/__init__.py",
    ],
)
def test_a_wheel_with_anything_else_fails(tmp_path, extra):
    site = fake_site(tmp_path)
    (site / extra).parent.mkdir(parents=True, exist_ok=True)
    (site / extra).write_text("import re\n")
    assert any(p.startswith(extra) for p in site_problems(site, "textkit"))


def test_a_wheel_whose_init_was_rewritten_fails(tmp_path):
    site = fake_site(tmp_path)
    init = site / "textkit" / "__init__.py"
    init.write_text(init.read_text() + "\nimport re\nescape = re.escape\n")
    assert site_problems(site, "textkit") == ["textkit/__init__.py: the wheel may only hold the compiled module and its metadata"]


def test_a_wheel_without_the_compiled_module_fails(tmp_path):
    site = fake_site(tmp_path)
    (site / "textkit" / "textkit.cpython-312-x86_64-linux-gnu.so").unlink()
    assert "the wheel holds 0 compiled modules named textkit, not one" in site_problems(site, "textkit")


docker = pytest.mark.skipif(not image_available(), reason="toolchain image not built")


@docker
def test_a_real_wheel_passes_and_the_reviewers_wheel_fails():
    """maturin's own output is allowed; with python-source = "target" it packs Python."""
    base = REPO / "target" / "integrity-wheel"
    if base.exists():
        shutil.rmtree(base)
    port = base / "port"
    shutil.copytree(RUST, port, ignore=shutil.ignore_patterns("target"))
    built = build_python_package(port, BINDINGS, base / "site", target_dir=base / "cargo")
    assert built.returncode == 0, built.stderr[-2000:]
    assert site_problems(base / "site", "textkit") == []

    pyproject = port / BINDINGS / "pyproject.toml"
    pyproject.write_text(pyproject.read_text() + 'python-source = "target"\n')
    hidden = port / BINDINGS / "target" / "textkit" / "__init__.py"
    hidden.parent.mkdir(parents=True)
    hidden.write_text("from .textkit import *\nimport re\nescape = re.escape\n")
    built = build_python_package(port, BINDINGS, base / "site", target_dir=base / "cargo")
    assert built.returncode == 0, built.stderr[-2000:]
    assert "textkit/__init__.py: the wheel may only hold the compiled module and its metadata" in site_problems(base / "site", "textkit")
    found = problems(port, strict=False)
    assert any("python-source isn't allowed" in p for p in found)
    assert any("target/textkit/__init__.py: Python source" in p for p in found)


# ---------------------------------------------------------------- 2. strict bindings


@pytest.mark.parametrize(
    "code",
    [
        # Raw C API that formats, does arithmetic, or reprs floats.
        "fn a(x: &Bound<'_, PyAny>) { let _ = unsafe { ffi::PyObject_Format(x.as_ptr(), std::ptr::null_mut()) }; }",
        "fn a() { let _ = unsafe { ffi::PyOS_double_to_string(1.0, b'r' as _, 0, 0, std::ptr::null_mut()) }; }",
        "fn a(x: &Bound<'_, PyAny>) { let _ = unsafe { ffi::PyNumber_FloorDivide(x.as_ptr(), x.as_ptr()) }; }",
        "use pyo3::ffi::{PyObject_Repr};",
        "use pyo3::ffi as raw;",
        "use pyo3::ffi::*;",
        'unsafe extern "C" { fn PyObject_Repr(o: *mut u8) -> *mut u8; }',
        # PyO3's Display and Debug call str() and repr().
        "fn a(x: &Bound<'_, PyAny>) -> String { x.to_string() }",
        'fn a(x: &Bound<\'_, PyAny>) -> String { format!("{:?}", 1) }',
        'fn a(x: &Bound<\'_, PyAny>) -> String { format!("{x}") }',
        'fn a(x: &Bound<\'_, PyAny>) -> String { format!("{}", x) }',
        'fn a(e: PyErr) -> String { format!("{}", &e) }',
        "fn a(x: Py<PyAny>) { dbg!(&x); }",
        # Operators, comparisons, item access and attribute lookup dispatch to Python.
        "fn a(x: &Bound<'_, PyAny>) { let _ = x.floor_div(x); }",
        "fn a(x: &Bound<'_, PyAny>) { let _ = x.rem(x); }",
        "fn a(x: &Bound<'_, PyAny>) { let _ = x.divmod(x); }",
        "fn a(x: &Bound<'_, PyAny>) { let _ = x.div(x); }",
        "fn a(x: &Bound<'_, PyAny>) { let _ = x.lt(x); }",
        "fn a(x: &Bound<'_, PyAny>) { let _ = x.get_item(0); }",
        "fn a(x: &Bound<'_, PyAny>) { let _ = x.getattr_opt(\"real\"); }",
        "fn a(x: &Bound<'_, PyAny>) { let _ = x.len(); }",
        "fn a(x: &Bound<'_, PyAny>) { let _ = PyAnyMethods::repr(x); }",
        "fn a(x: &Bound<'_, PyAny>) { let _ = x.call_method1(\"encode\", (\"unicode_escape\",)); }",
        # Imports, including by a name the scan can't read.
        "fn a(py: Python<'_>, name: &str) { let _ = PyModule::import(py, name); }",
        "fn a(py: Python<'_>) { let _ = py.import(\"decimal\"); }",
        # A comment marker inside a string used to hide the rest of the line.
        'fn a(x: &Bound<\'_, PyAny>) { let _s = "//"; let _ = x.repr(); }',
    ],
)
def test_strict_bindings_reject(tmp_path, code):
    port = make_port(tmp_path)
    add_to_bindings(port, code)
    found = problems(port)
    assert any("calls into Python" in p for p in found), found


def test_strict_bindings_scan_a_sibling_crate_that_depends_on_pyo3(tmp_path):
    port = make_port(tmp_path)
    helper = port / "crates" / "helper"
    (helper / "src").mkdir(parents=True)
    (helper / "Cargo.toml").write_text('[package]\nname = "helper"\nversion = "0.1.0"\n\n[dependencies]\npyo3 = "0.29"\n')
    (helper / "src" / "lib.rs").write_text("use pyo3::prelude::*;\npub fn show(x: &Bound<'_, PyAny>) -> PyResult<String> { Ok(x.str()?.to_string()) }\n")
    assert any("crates/helper/src/lib.rs:2" in p and "calls into Python" in p for p in problems(port))


def test_the_core_cant_reach_pyo3_through_another_crate(tmp_path):
    port = make_port(tmp_path)
    helper = port / "crates" / "helper"
    (helper / "src").mkdir(parents=True)
    (helper / "Cargo.toml").write_text('[package]\nname = "helper"\nversion = "0.1.0"\n\n[dependencies]\npyo3 = "0.29"\n')
    (helper / "src" / "lib.rs").write_text("pub use pyo3;\n")
    manifest = port / "crates" / "textkit-core" / "Cargo.toml"
    manifest.write_text(manifest.read_text() + 'helper = { path = "../helper" }\n')
    assert any("even through another crate" in p for p in problems(port, strict=False))


@pytest.mark.parametrize("how", ['#[path = "../../../shared/evil.rs"]\nmod evil;', 'include!("../../../shared/evil.rs");'])
def test_strict_bindings_scan_files_pulled_in_from_outside_the_crate(tmp_path, how):
    port = make_port(tmp_path)
    (port / "shared").mkdir()
    (port / "shared" / "evil.rs").write_text("pub fn show(x: &pyo3::Bound<'_, pyo3::PyAny>) -> String { x.repr().unwrap().to_string() }\n")
    add_to_bindings(port, how)
    assert any(p.startswith("shared/evil.rs:1") and "calls into Python" in p for p in problems(port))
    assert check(port, strict=False).passed


def test_files_pulled_in_from_build_output_or_outside_the_port_fail(tmp_path):
    port = make_port(tmp_path)
    add_to_bindings(port, 'include!("../../../target/gen.rs");\ninclude!("../../../../elsewhere.rs");\ninclude!(concat!(env!("OUT_DIR"), "/gen.rs"));')
    found = problems(port, strict=False)
    assert any("which is build output" in p for p in found)
    assert any("which is outside the port" in p for p in found)
    assert any("computed path" in p for p in found)


def test_a_folder_named_fuzz_inside_a_crate_is_still_scanned(tmp_path):
    port = make_port(tmp_path)
    nested = port / BINDINGS / "src" / "fuzz"
    nested.mkdir()
    (nested / "mod.rs").write_text("pub fn show(x: &pyo3::Bound<'_, pyo3::PyAny>) { let _ = x.repr(); }\n")
    add_to_bindings(port, "mod fuzz;")
    assert any("src/fuzz/mod.rs:1" in p for p in problems(port))


def test_a_macro_from_the_core_cant_carry_dispatch_into_the_bindings(tmp_path):
    port = make_port(tmp_path)
    core = port / "crates" / "textkit-core" / "src" / "lib.rs"
    core.write_text(core.read_text() + "\n#[macro_export]\nmacro_rules! show { ($x:expr) => { $x.repr() }; }\n")
    assert any("textkit-core/src/lib.rs" in p and "a macro the bindings use" in p for p in problems(port))
    assert check(port, strict=False).passed


def test_pyo3_features_that_run_python_are_rejected(tmp_path):
    port = make_port(tmp_path)
    manifest = port / BINDINGS / "Cargo.toml"
    manifest.write_text(manifest.read_text().replace('"num-bigint"]', '"num-bigint", "rust_decimal"]'))
    assert any("PyO3 feature rust_decimal" in p for p in problems(port))
    assert check(port, strict=False).passed


def test_imports_by_an_unreadable_name_fail_without_strict_bindings_too(tmp_path):
    port = make_port(tmp_path)
    add_to_bindings(port, "fn a(py: Python<'_>, name: &str) { let _ = PyModule::import(py, name); }")
    assert any("named by an expression" in p for p in problems(port, strict=False))


def test_imports_through_a_const_or_raw_string_are_still_read(tmp_path):
    port = make_port(tmp_path)
    add_to_bindings(port, 'const LIB: &str = "textkit";\nfn a(py: Python<\'_>) { let _ = py.import(LIB); let _ = py.import(r"textkit"); }')
    assert sum("imports the original library textkit" in p for p in problems(port, strict=False)) >= 1


def test_dependencies_from_outside_crates_io_or_the_port_fail(tmp_path):
    port = make_port(tmp_path)
    manifest = port / "crates" / "textkit-core" / "Cargo.toml"
    manifest.write_text(
        manifest.read_text()
        + 'regex = { git = "https://example.com/regex" }\nnear = { path = "../../../near" }\n'
        + "\n[target.'cfg(unix)'.dependencies]\nsneaky = \"1\"\n\n[patch.crates-io]\nregex = { path = \"../regex\" }\n"
    )
    found = problems(port, strict=False)
    assert any("comes from git" in p for p in found)
    assert any("dependency near is outside the port" in p for p in found)
    assert any("crate sneaky isn't approved" in p for p in found)
    assert any("[patch] overrides dependencies" in p for p in found)


# ---------------------------------------------------------------- finished ports


FINISHED = [
    ("pycompat/ports/pc_format", "pc_format", True),
    ("pycompat/ports/pc_unicode", "pc_unicode", True),
    ("pycompat/ports/pc_re", "pc_re", False),
    ("ports/parse", "parse", False),
]


def finished_port(tmp_path, folder):
    port = tmp_path / "port"
    # The checks sit next to a finished port, but never inside the workspace being gated.
    shutil.copytree(REPO / folder, port, ignore=shutil.ignore_patterns("target", "checks"))
    return port


@pytest.mark.parametrize(("folder", "module", "strict"), FINISHED)
def test_each_finished_port_passes(tmp_path, folder, module, strict):
    core, py = crate_names(module)
    result = integrity_gate(
        finished_port(tmp_path, folder), module, core_crate=core, strict_bindings=strict, bindings_dir=f"crates/{py}",
    )
    assert result.passed, result.details["problems"]


def test_pc_re_under_strict_bindings_fails_only_on_its_decimal_conversion(tmp_path):
    """pc_re turns a decimal.Decimal argument into text with PyO3's rust_decimal
    conversion, which calls str() on it, and builds Decimals back by calling
    decimal.Decimal(). That runs the decimal module's code. Reported, not fixed here."""
    core, py = crate_names("pc_re")
    result = integrity_gate(
        finished_port(tmp_path, "pycompat/ports/pc_re"), "pc_re", core_crate=core, strict_bindings=True, bindings_dir=f"crates/{py}",
    )
    assert result.details["problems"] == [
        "crates/pc-re-port-py/Cargo.toml: PyO3 feature rust_decimal converts values by running Python code, "
        "which strict bindings don't allow"
    ]


def test_build_scripts_and_crate_sources_elsewhere_are_caught(tmp_path):
    port = make_port(tmp_path)
    (port / BINDINGS / "build.rs").write_text("fn main() {}\n")
    (port / "shared").mkdir()
    (port / "shared" / "lib.rs").write_text("pub fn show(x: &pyo3::Bound<'_, pyo3::PyAny>) { let _ = x.repr(); }\n")
    manifest = port / BINDINGS / "Cargo.toml"
    manifest.write_text(manifest.read_text().replace('crate-type = ["cdylib"]', 'crate-type = ["cdylib"]\npath = "../../shared/lib.rs"'))
    found = problems(port)
    assert any("build scripts run code" in p for p in found)
    assert any(p.startswith("shared/lib.rs:1") and "calls into Python" in p for p in found)


def test_maturins_reexport_is_recognised_by_structure_not_exact_text():
    from portwright.gates.integrity import _MATURIN_INIT, _is_maturin_reexport

    m = "pc_re"
    for text in (_MATURIN_INIT.format(m), f"from .{m} import *\n__doc__ = {m}.__doc__\n", f'"""Doc."""\nfrom .{m} import *\n'):
        assert _is_maturin_reexport(text, m), text
    for text in (
        f"from .{m} import *\nimport re\n",
        f"from .{m} import *\nescape = re.escape\n",
        f"from .{m} import *\nif hasattr({m}, '__all__'):\n    __all__ = {m}.__all__\nelse:\n    import os\n",
        f"from .{m} import *\n__doc__ = open('x').read()\n",
        "exec('1')",
    ):
        assert not _is_maturin_reexport(text, m), text
