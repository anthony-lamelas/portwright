"""Agent code that a gate runs can't touch the host: not the run's state, not the network."""

import sys

import pytest

from portwright import isolation
from portwright.gates.bench import run_benchmarks
from portwright.gates.differential import PYTEST_EXIT_OK, run_checks
from portwright.gates.upstream_tests import run_tests
from portwright.toolchain import image_available

pytestmark = pytest.mark.skipif(not image_available(), reason="toolchain image not built")

GOOD = "def f(x):\n    return x\n"


def library(folder, text):
    folder.mkdir(parents=True)
    (folder / "toy.py").write_text(text)
    return {"path": str(folder), "module": "toy", "python": sys.executable}


def checks(folder, body):
    folder.mkdir()
    (folder / "test_toy.py").write_text(
        "from hypothesis import given, settings, strategies as st\n"
        "from portwright.compare import same\n"
        "@settings(max_examples=5)\n"
        "@given(st.integers(0, 5))\n"
        "def test_f(x):\n"
        "    same('f', x)\n" + body
    )
    return folder


@pytest.fixture
def state(tmp_path):
    """Stands in for control/run.json, which a gate must never let agent code change."""
    path = tmp_path / "control" / "run.json"
    path.parent.mkdir()
    path.write_text('{"approved_differences": []}')
    return path


def test_gates_are_contained_by_default():
    assert isolation.enabled()


def test_honest_checks_still_pass_contained(tmp_path):
    reference = library(tmp_path / "ref", GOOD)
    port = library(tmp_path / "port", GOOD)
    run = run_checks(checks(tmp_path / "checks", ""), reference, port, max_examples=5)
    assert run.exit_status == PYTEST_EXIT_OK, run.output[-2000:]
    assert run.stats["f"]["calls"] > 0


def test_a_check_file_cant_rewrite_run_state(tmp_path, state):
    body = (
        "def test_escape():\n"
        f"    open({str(state)!r}, 'w').write('{{\"approved_differences\": [\"everything\"]}}')\n"
    )
    reference = library(tmp_path / "ref", GOOD)
    run_checks(checks(tmp_path / "checks", body), reference, reference, max_examples=5)
    assert state.read_text() == '{"approved_differences": []}'


def test_the_ports_module_cant_rewrite_run_state_on_import(tmp_path, state):
    reference = library(tmp_path / "ref", GOOD)
    hostile = GOOD + f"try:\n    open({str(state)!r}, 'w').write('pwned')\nexcept OSError:\n    pass\n"
    port = library(tmp_path / "port", hostile)
    run = run_checks(checks(tmp_path / "checks", ""), reference, port, max_examples=5)
    assert run.stats, run.output[-2000:]
    assert state.read_text() == '{"approved_differences": []}'


def test_checks_have_no_network(tmp_path):
    body = (
        "import socket\n"
        "def test_network():\n"
        "    socket.create_connection(('1.1.1.1', 443), timeout=3)\n"
    )
    reference = library(tmp_path / "ref", GOOD)
    run = run_checks(checks(tmp_path / "checks", body), reference, reference, max_examples=5)
    assert any("test_network" in failure["test"] for failure in run.failures), run.output[-2000:]


def test_upstream_tests_run_contained(tmp_path, state):
    tests = tmp_path / "tests"
    tests.mkdir()
    (tests / "test_up.py").write_text(
        "import toy\n"
        "def test_value():\n    assert toy.f(2) == 2\n"
        "def test_escape():\n"
        f"    open({str(state)!r}, 'w').write('pwned')\n"
    )
    outcome = run_tests(tests, library(tmp_path / "ref", GOOD))
    assert outcome["tests.test_up::test_value"] == "passed"
    assert outcome["tests.test_up::test_escape"] == "failed"
    assert state.read_text() == '{"approved_differences": []}'


def test_benchmarks_run_contained(tmp_path):
    side = library(tmp_path / "ref", GOOD)
    results = run_benchmarks(side, [{"name": "f", "target": "f", "args": [3]}], samples=3)
    assert results["f"]["kind"] == "benchmark", results


def test_files_a_contained_gate_needs_cant_be_in_tmp():
    with pytest.raises(ValueError, match="temporary directory"):
        isolation.run(["true"], cwd="/tmp")
