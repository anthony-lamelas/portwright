"""The gates must fail when they should. A gate that can't fail proves nothing."""

import sys
from pathlib import Path

import pytest

from portwright.gates.canaries import build_mutant, canary_gate, list_mutants
from portwright.gates.differential import differential_gate, self_comparison_gate
from portwright.manifest import extract_api

FIXTURES = Path(__file__).parent / "fixtures"
TEXTKIT = FIXTURES / "textkit"
GOOD = FIXTURES / "checks_good"
WEAK = FIXTURES / "checks_weak"
SIDE = {"path": str(TEXTKIT), "module": "textkit", "python": sys.executable}
API = extract_api(TEXTKIT / "textkit")


def test_manifest_lists_the_public_api():
    assert {"slug", "Stack", "Stack.push", "Stack.pop", "ParseError"} <= set(API)
    assert not any(name.startswith("_") for name in API)


def test_good_checks_pass_against_the_library_itself():
    result = self_comparison_gate(GOOD, SIDE, api=API, max_examples=100)
    assert result.passed, result.summary
    assert result.details["calls"] > 500


def test_broken_generator_fails_even_with_zero_differences():
    """The textdistance failure: everything raises the same error, so nothing is tested."""
    result = differential_gate(WEAK, SIDE, SIDE, api=API, max_examples=20)
    assert not result.passed
    assert result.details["divergences"] == {}
    assert {"slug", "truncate"} <= set(result.details["reach"]["rarely_returned"])
    assert "floor_bucket" in result.details["reach"]["never_called"]


def test_missing_checks_directory_fails(tmp_path):
    result = differential_gate(tmp_path, SIDE, SIDE)
    assert not result.passed
    assert "did not run" in result.summary


def test_one_planted_bug_is_caught_with_a_useful_message(tmp_path):
    mutant = next(
        m for m in list_mutants(TEXTKIT)
        if m.description == "swap operator FloorDiv" and m.file.endswith("__init__.py")
    )
    build_mutant(TEXTKIT, mutant, tmp_path)
    result = differential_gate(
        GOOD, SIDE, {**SIDE, "path": str(tmp_path)}, api=API, max_examples=100
    )
    assert not result.passed
    assert "floor_bucket" in str(result.details["divergences"])


def test_good_checks_catch_nearly_all_planted_bugs():
    result = canary_gate(GOOD, TEXTKIT, "textkit", minimum=0.9, max_examples=60, workers=8)
    assert result.passed, f"{result.summary}\n{result.details['missed']}"


def test_weak_checks_fail_the_canary_gate():
    result = canary_gate(WEAK, TEXTKIT, "textkit", minimum=0.5, sample=15, max_examples=10, workers=8)
    assert not result.passed, result.summary
    assert result.details["missed"]
