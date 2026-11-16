"""The report says which review findings and proposals later rounds settled (#60)."""

import json
from types import SimpleNamespace

from portwright.report import build_markdown


def gate(passed):
    return {"passed": passed, "summary": "x", "details": {}}


def report(tmp_path, gates, stopped=None, followup=None):
    public, control = tmp_path / "public", tmp_path / "control"
    (control / "differences").mkdir(parents=True)
    public.mkdir()
    (public / "review.md").write_text("## Must fix\n### 1. Long formats fail\n")
    (control / "differences" / "a.json").write_text(json.dumps({"signature": "f(None)", "reason": "r"}))
    paths = SimpleNamespace(public=public, control=control)
    config = SimpleNamespace(lib="toy", url="https://example.invalid")
    state = {"gates": gates, "stopped": stopped}
    if followup is not None:
        state["review_followups"] = {"final_review": followup}
    return build_markdown(paths, config, state, {}, {}, {})


CLEAN = {"main_differential": gate(True), "self_comparison_after_final_review": gate(True)}


def test_only_reproduced_differences_are_called_fixed(tmp_path):
    text = report(tmp_path, CLEAN, followup={"reproduced": ["strptime outcome.type"], "fixed": True})
    assert "fix round fixed all of them: `strptime outcome.type`" in text
    assert "may still stand" in text


def test_checks_that_reproduced_nothing_fix_nothing(tmp_path):
    text = report(tmp_path, CLEAN, followup={"reproduced": [], "fixed": True})
    assert "reproduced none of the findings" in text and "fixed all" not in text


def test_an_unfinished_fix_round_isnt_called_fixed(tmp_path):
    text = report(tmp_path, {**CLEAN, "main_differential": gate(False)}, followup={"reproduced": ["f outcome"], "fixed": False})
    assert "weren't all fixed" in text


def test_a_stopped_run_never_claims_fixes(tmp_path):
    text = report(tmp_path, CLEAN, stopped="limit", followup={"reproduced": ["f outcome"], "fixed": True})
    assert "fixed all" not in text


def test_old_runs_without_a_record_claim_nothing(tmp_path):
    text = report(tmp_path, CLEAN)
    assert "didn't record which findings" in text and "fixed all" not in text


def test_rejected_review_checks_say_nothing_was_checked(tmp_path):
    text = report(tmp_path, {**CLEAN, "self_comparison_after_final_review": gate(False)})
    assert "were not checked or fixed" in text


def test_proposals_are_not_called_fixed(tmp_path):
    text = report(tmp_path, CLEAN)
    assert "was fixed later, is covered by an approved difference" in text
    assert "resolved them" not in text


def test_runs_without_a_final_review_fix_round_are_unchanged(tmp_path):
    text = report(tmp_path, {"main_differential": gate(True)})
    assert "## Must fix" in text and "fix round" not in text


def test_a_run_stopped_before_porting_still_gets_a_report(tmp_path):
    """#62: the report phase crashed when intake stopped the run before port/ existed."""
    from portwright.report import write_report
    from portwright.state import RunPaths

    paths = RunPaths(tmp_path / "run")
    paths.control.mkdir(parents=True)
    config = SimpleNamespace(lib="toy", url="https://example.invalid")
    written = write_report(paths, config, {"gates": {}, "stopped": "the original's own tests don't pass here"}, {}, {}, {})
    assert "stopped early" in written.read_text()


def test_rulings_by_a_person_are_in_the_report(tmp_path):
    """#67: isodate's accepted introspection difference is a ruling, not an approved signature."""
    public, control = tmp_path / "public", tmp_path / "control"
    public.mkdir()
    control.mkdir()
    (control / "human_decisions.md").write_text("- Accepted difference: operators aren't Python functions.\n")
    paths = SimpleNamespace(public=public, control=control)
    text = build_markdown(paths, SimpleNamespace(lib="toy", url="https://example.invalid"), {"gates": {}}, {}, {}, {})
    assert "## Rulings by a person" in text and "operators aren't Python functions" in text


def test_a_run_without_rulings_has_no_rulings_section(tmp_path):
    (tmp_path / "public").mkdir()
    (tmp_path / "control").mkdir()
    paths = SimpleNamespace(public=tmp_path / "public", control=tmp_path / "control")
    assert "Rulings by a person" not in build_markdown(paths, SimpleNamespace(lib="toy", url="https://example.invalid"), {"gates": {}}, {}, {}, {})
