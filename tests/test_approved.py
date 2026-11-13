"""An approved difference is accepted where it occurs, and nowhere else.

Approving a difference used to end each check at the first call that showed it, so
nothing after that call was compared, and any other failure in the run was excused.
"""

import sys

import pytest

from portwright.evaluation import Project, differential_for
from portwright.gates.differential import PYTEST_EXIT_OK, run_checks

REFERENCE = '''
def f(x):
    if x % 2 == 0:
        print("debug dump")
    return x

def g(x):
    return x
'''
APPROVED = "f outcome.stdout"


def library(folder, text):
    folder.mkdir(parents=True)
    (folder / "toy.py").write_text(text)
    return str(folder)


@pytest.fixture
def sides(tmp_path):
    def make(name, text):
        return {"path": library(tmp_path / name, text), "module": "toy", "python": sys.executable}
    return make


@pytest.fixture
def checks(tmp_path):
    folder = tmp_path / "checks"
    folder.mkdir()
    (folder / "test_toy.py").write_text(
        "from hypothesis import given, strategies as st\n"
        "from portwright.compare import same\n"
        "@given(st.integers(0, 20))\n"
        "def test_both(x):\n"
        "    same('f', x)\n"
        "    same('g', x)\n"
    )
    return folder


def project(reference, checks, approved=(APPROVED,)):
    return Project(
        module="toy", lib="toy", core_crate="c", py_crate="p", reference=reference,
        checks=str(checks), tests_visible="", approved_differences=list(approved),
    )


def test_a_bug_after_an_approved_difference_is_still_found(sides, checks):
    reference = sides("ref", REFERENCE)
    port = sides("port", "def f(x):\n    return x\n\ndef g(x):\n    return -1 if x == 7 else x\n")
    result = differential_for(project(reference, checks), port["path"], max_examples=200)
    assert not result.passed, result.summary
    assert [s.split()[0] for s in result.details["divergences"]] == ["g"]
    stats = result.details["stats"]
    assert stats["g"]["calls"] == stats["f"]["calls"], "every call after the approved one still ran"


def test_a_port_differing_only_where_approved_passes(sides, checks):
    reference = sides("ref", REFERENCE)
    port = sides("port", "def f(x):\n    return x\n\ndef g(x):\n    return x\n")
    result = differential_for(project(reference, checks), port["path"], max_examples=100)
    assert result.passed, result.summary
    assert result.details["approved_seen"] == [APPROVED]


def test_an_approved_difference_doesnt_excuse_other_check_failures(sides, checks):
    (checks / "test_other.py").write_text("def test_broken():\n    raise RuntimeError('not a difference')\n")
    reference = sides("ref", REFERENCE)
    port = sides("port", "def f(x):\n    return x\n\ndef g(x):\n    return x\n")
    result = differential_for(project(reference, checks), port["path"], max_examples=50)
    assert not result.passed
    assert "no recorded difference" in result.summary


def test_an_approved_difference_alone_doesnt_fail_the_run(sides, checks):
    """So a Rust mutant that only shows the approved difference counts as missed, not caught."""
    reference = sides("ref", REFERENCE)
    mutant = sides("mutant", "def f(x):\n    return x\n\ndef g(x):\n    return x\n")
    run = run_checks(checks, reference, mutant, max_examples=50, stop_at_first=True, approved=[APPROVED])
    assert run.exit_status == PYTEST_EXIT_OK, run.output[-2000:]
    assert APPROVED in run.divergences, "it is still recorded"
