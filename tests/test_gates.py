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


def test_manifest_includes_public_names_outside_dunder_all(tmp_path):
    (tmp_path / "__init__.py").write_text(
        "__all__ = ['listed']\ndef listed(): pass\ndef also_public(): pass\ndef _private(): pass\n"
        "class Thing:\n    def method(self): pass\n"
    )
    assert extract_api(tmp_path) == ["Thing", "Thing.method", "also_public", "listed"]


def test_manifest_names_submodule_apis_by_where_callers_find_them(tmp_path):
    (tmp_path / "__init__.py").write_text("from pkg.time import shown\n__all__ = ['shown']\n")
    (tmp_path / "time.py").write_text("def shown(): pass\ndef hidden(): pass\nclass Unit:\n    def name(self): pass\n")
    (tmp_path / "_private.py").write_text("def secret(): pass\n")
    assert extract_api(tmp_path) == ["shown", "time.Unit", "time.Unit.name", "time.hidden"]


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


def test_hand_written_canaries_replace_ast_mutants(tmp_path):
    from portwright.gates.canaries import load_written_canaries

    (tmp_path / "001-upper.py").write_text(
        "# canary: slug keeps capitals\n_slug = slug\ndef slug(text, max_len=None, sep='-'):\n"
        "    return _slug(text, max_len, sep).upper()\n"
    )
    (tmp_path / "002-noop.py").write_text("# canary: changes nothing\n_unused = 1\n")
    written = load_written_canaries(tmp_path, "textkit/__init__.py")
    assert [m.description for m in written] == ["001-upper: slug keeps capitals", "002-noop: changes nothing"]
    result = canary_gate(GOOD, TEXTKIT, "textkit", minimum=0.5, max_examples=30, workers=2, written=written)
    assert result.details["caught"] == 1
    assert [m["bug"] for m in result.details["missed"]] == ["002-noop: changes nothing"]
    assert "hand-written" in result.summary


def test_vendored_crates_are_copied_in_and_frozen(tmp_path):
    from portwright.evaluation import _shared_problems
    from portwright.orchestrator import Config, Porter

    crate = tmp_path / "pycompat-fmt"
    (crate / "src").mkdir(parents=True)
    (crate / "Cargo.toml").write_text('[package]\nname = "pycompat-fmt"\nversion = "0.1.0"\nedition = "2021"\n')
    (crate / "src" / "lib.rs").write_text("pub fn f() {}\n")
    (crate / "target").mkdir()
    (crate / "target" / "junk").write_text("x")
    config = Config(lib="x", module="x", source=tmp_path, package_path="x", tests_path="t", url="u",
                    run_dir=tmp_path / "run", vendor={"pycompat-fmt": str(crate)})
    porter = Porter(config, None)
    porter.paths.port.mkdir(parents=True)
    porter._vendor_crates()
    vendored = porter.paths.port / "vendor" / "pycompat-fmt"
    assert (vendored / "src" / "lib.rs").exists() and not (vendored / "target").exists()
    shared = porter.run_state.read()["shared"]
    assert set(shared) == {"vendor/pycompat-fmt/Cargo.toml", "vendor/pycompat-fmt/src/lib.rs"}
    assert _shared_problems(porter.paths.port, shared) == []
    (vendored / "src" / "lib.rs").write_text("pub fn g() {}\n")
    assert _shared_problems(porter.paths.port, shared) == ["vendor/pycompat-fmt/src/lib.rs is frozen and can't be changed; file a request instead"]
    assert "vendor/pycompat-fmt" in porter._vendor_note()


def test_seed_code_is_copied_in_but_not_frozen(tmp_path):
    from portwright.orchestrator import Config, Porter

    seed = tmp_path / "pyre"
    seed.mkdir()
    (seed / "engine.rs").write_text("pub fn engine() {}\n")
    config = Config(lib="x", module="x", source=tmp_path, package_path="x", tests_path="t", url="u",
                    run_dir=tmp_path / "run", seed={"pyre": str(seed)})
    porter = Porter(config, None)
    porter.paths.port.mkdir(parents=True)
    porter._vendor_crates()
    assert (porter.paths.port / "seed" / "pyre" / "engine.rs").exists()
    assert porter.run_state.read()["shared"] == {}
    assert "seed/pyre" in porter._vendor_note()
