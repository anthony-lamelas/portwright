"""Runs the checks against real compiled Rust, not just Python copies."""

import shutil
import sys
from pathlib import Path

import pytest

from portwright.gates.differential import differential_gate
from portwright.manifest import extract_api
from portwright.toolchain import build_python_package, image_available

FIXTURES = Path(__file__).parent / "fixtures"
TEXTKIT = FIXTURES / "textkit"
REFERENCE = {"path": str(TEXTKIT), "module": "textkit", "python": sys.executable}

pytestmark = pytest.mark.docker


@pytest.fixture(scope="module")
def rust_side(tmp_path_factory):
    if not image_available():
        pytest.skip("toolchain image not built")
    install_dir = tmp_path_factory.mktemp("rust") / "site"
    built = build_python_package(FIXTURES / "textkit_rust", "crates/textkit-py", install_dir)
    assert built.returncode == 0, built.stderr[-3000:]
    return {"path": str(install_dir), "module": "textkit", "python": sys.executable}


def test_rust_port_matches_python(rust_side):
    result = differential_gate(
        FIXTURES / "checks_good", REFERENCE, rust_side,
        api=extract_api(TEXTKIT / "textkit"), max_examples=500,
    )
    assert result.passed, f"{result.summary}\n{result.details.get('divergences')}\n{result.details.get('output', '')}"
    assert list(Path(rust_side["path"]).rglob("*.so")), "the candidate must be compiled Rust"


CORE = "crates/textkit-core/src/lib.rs"
BINDING = "crates/textkit-py/src/lib.rs"
PORTING_BUGS = {
    "truncating_division": (CORE, "n.div_mod_floor(size)", "n.div_rem(size)", "floor_bucket"),
    "reordered_dict": (CORE, "    counts\n}", "    counts.sort_keys();\n    counts\n}", "word_count"),
    "wrong_exception_base": (BINDING, "create_exception!(textkit, ParseError, PyValueError);", "create_exception!(textkit, ParseError, PyException);", "parse_kv"),
    "changed_error_text": (BINDING, '"stack full"', '"stack is full"', "Stack.push"),
    "argument_not_updated": (BINDING, 'values.call_method1("__setitem__", (PySlice::full(py), numbers))?;', "let _ = (py, numbers);", "normalize_in_place"),
}


@pytest.mark.parametrize("name", PORTING_BUGS)
def test_classic_porting_bugs_in_real_rust_are_caught(name):
    if not image_available():
        pytest.skip("toolchain image not built")
    file, old, new, api = PORTING_BUGS[name]
    root = Path(__file__).parent.parent / "target" / "rust-bugs"
    workspace = root / name
    if workspace.exists():
        shutil.rmtree(workspace)
    shutil.copytree(FIXTURES / "textkit_rust", workspace, ignore=shutil.ignore_patterns("target"))
    source = workspace / file
    text = source.read_text()
    assert old in text
    source.write_text(text.replace(old, new, 1))
    site = root / f"{name}-site"
    built = build_python_package(workspace, "crates/textkit-py", site)
    assert built.returncode == 0, built.stderr[-2000:]
    result = differential_gate(
        FIXTURES / "checks_good", REFERENCE, {**REFERENCE, "path": str(site)}, max_examples=200,
    )
    assert not result.passed
    assert any(signature.startswith(f"{api} ") for signature in result.details["divergences"])
