"""The dependents report flags a test only if it passed in every original run and failed with the port."""

import importlib.util
import sys
from pathlib import Path

spec = importlib.util.spec_from_file_location("dependents_run", Path(__file__).resolve().parents[1] / "tools/dependents/run.py")
dependents = importlib.util.module_from_spec(spec)
sys.modules["dependents_run"] = dependents  # dataclasses look their module up here
spec.loader.exec_module(dependents)
Outcome = dependents.Outcome


def _outcome(variant, **tests):
    return Outcome(variant, {name: (state, f"{state} message") for name, state in tests.items()})


def test_only_stable_passes_that_fail_with_the_port_are_flagged():
    result = {"outcomes": [
        _outcome("original-1", same="passed", broke="passed", flaky="passed", bad="failed", gone="passed", skip="skipped"),
        _outcome("original-2", same="passed", broke="passed", flaky="failed", bad="failed", gone="passed", skip="skipped"),
        _outcome("port-1", same="passed", broke="error", flaky="failed", bad="failed", skip="failed"),
    ]}
    summary = dependents.classify(result)
    assert [test for test, _, _ in summary["flagged"]] == ["broke"]
    assert summary["flaky"] == ["flaky"]
    assert summary["original_fails"] == ["bad"]
    assert summary["missing_in_port"] == ["gone"]
    assert summary["stable_pass"] == 3


def test_a_broken_original_run_flags_nothing():
    broken = _outcome("original-2")
    broken.problem = "no test results"
    result = {"outcomes": [_outcome("original-1", a="passed"), broken, _outcome("port-1", a="failed")]}
    assert dependents.classify(result)["flagged"] == []


def test_junit_keeps_the_worst_entry_for_a_test(tmp_path):
    junit = tmp_path / "junit.xml"
    junit.write_text(
        '<testsuites><testsuite><testcase classname="m" name="t"/>'
        '<testcase classname="m" name="t"><error message="teardown">boom</error></testcase>'
        '<testcase classname="m" name="u"><skipped message="no"/></testcase></testsuite></testsuites>'
    )
    tests = dependents.read_junit(junit)
    assert tests["m::t"][0] == "error" and tests["m::u"][0] == "skipped"
