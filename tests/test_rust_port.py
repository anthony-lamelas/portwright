"""Runs the checks against real compiled Rust, not just Python copies."""

import sys
from pathlib import Path

import pytest

from porter.gates.differential import differential_gate
from porter.manifest import extract_api
from porter.toolchain import build_python_package, image_available

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
