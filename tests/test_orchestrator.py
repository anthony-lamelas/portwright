"""Runs the whole pipeline with scripted agents and real gates."""

import json
import shutil
import subprocess
from pathlib import Path

import pytest

from fake_agents import (
    ALL_APIS,
    FakeAgents,
    broken_port,
    cheat_import_original,
    limit_once,
    needs_shared_change,
    regressing_cleanup,
    weak_checks_only,
)
from portwright.agents import LimitReached
from portwright.orchestrator import Config, Porter
from portwright.report import verdict
from portwright.toolchain import image_available

FIXTURES = Path(__file__).parent / "fixtures"
RUNS = Path(__file__).parent.parent / "target" / "orchestrator-tests"

pytestmark = pytest.mark.skipif(not image_available(), reason="toolchain image not built")


@pytest.fixture(scope="module")
def source():
    """A fake upstream project with a license, tests, and git history."""
    path = RUNS / "source"
    if path.exists():
        shutil.rmtree(path)
    (path / "tests").mkdir(parents=True)
    shutil.copytree(FIXTURES / "textkit" / "textkit", path / "textkit")
    (path / "LICENSE").write_text("Permission is hereby granted, free of charge, to any person\n")
    (path / "tests" / "test_lib.py").write_text(
        "import textkit\n"
        + "".join(f"def test_{n}():\n    assert textkit.truncate('abcdefgh', {n}) == 'abcdefgh'[:{n}]\n" for n in range(1, 12))
    )
    identity = ["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false"]
    subprocess.run(["git", "init", "-q"], cwd=path, check=True)
    subprocess.run(["git", *identity, "add", "-A"], cwd=path, check=True)
    subprocess.run(["git", *identity, "commit", "-qm", "start"], cwd=path, check=True)
    return path


def make_config(source, name, **overrides):
    run_dir = RUNS / name
    if run_dir.exists():
        shutil.rmtree(run_dir)
    settings = dict(
        lib="textkit", module="textkit", source=source, package_path="textkit", tests_path="tests",
        url="https://example.invalid/textkit", run_dir=run_dir,
        canary_sample=12, canary_examples=60, examples=100, hard_examples=150, fuzz_seconds=3,
        mutation_sample=3, mutation_minimum=0.0, performance_rounds=1, check_rounds=2, harden_cycles=1,
    )
    settings.update(overrides)
    return Config(**settings)


def gates(state):
    return {name: gate["passed"] for name, gate in state["gates"].items()}


def test_full_run_with_a_good_port_reaches_a_pass(source):
    agents = FakeAgents()
    config = make_config(source, "good")
    state = Porter(config, agents).run()
    assert state["stopped"] is None, state["stopped"]
    assert state["phases_done"][-1] == "report"
    result, reasons = verdict(state)
    assert result == "matched on what was tested", reasons
    report = (Path(config.run_dir) / "port" / "CONFORMANCE.md").read_text()
    assert "doesn't prove the two are equivalent" in report
    assert "unofficial" in report
    roles = {call.split(":")[0] for call in agents.calls}
    assert {"test_writer", "planner", "porter", "reviewer", "cleanup", "performance"} <= roles
    assert (Path(config.run_dir) / "port" / "LICENSE.upstream").exists()


def test_rerunning_a_finished_run_does_nothing(source):
    config = make_config(source, "rerun", fuzz_seconds=1)
    Porter(config, FakeAgents()).run()
    agents = FakeAgents()
    Porter(config, agents).run()
    assert agents.calls == []


def test_weak_checks_stop_the_run_before_any_porting(source):
    agents = FakeAgents(behaviors={"test_writer": weak_checks_only})
    config = make_config(source, "weak")
    state = Porter(config, agents).run()
    assert "too weak" in state["stopped"]
    assert not any(call.startswith("porter") for call in agents.calls)
    assert verdict(state)[0] == "not conforming"
    assert "not conforming" in (Path(config.run_dir) / "port" / "CONFORMANCE.md").read_text()


def test_port_importing_the_original_is_rejected_then_retried(source):
    agents = FakeAgents(behaviors={"porter": cheat_import_original})
    config = make_config(source, "cheat", fuzz_seconds=1)
    state = Porter(config, agents).run()
    assert sum(call.startswith("porter") for call in agents.calls) >= 2
    prompt_failure = (Path(config.run_dir) / "control" / "board.json").read_text()
    assert "merged" in prompt_failure
    assert state["stopped"] is None


def test_repeated_failures_escalate_to_the_debugger(source):
    agents = FakeAgents(behaviors={"porter": broken_port})
    config = make_config(source, "debugger", fuzz_seconds=1)
    state = Porter(config, agents).run()
    roles = [call.split(":")[0] for call in agents.calls]
    assert roles.count("porter") == 2, roles
    assert "debugger" in roles
    assert state["stopped"] is None
    regressions = Path(config.run_dir) / "checks" / "regressions.json"
    assert regressions.exists(), "the failing input must become a permanent test"
    assert "floor_bucket" in regressions.read_text()


def test_repeated_requests_for_frozen_changes_block_the_task_early(source):
    agents = FakeAgents(behaviors={"porter": needs_shared_change, "debugger": needs_shared_change})
    config = make_config(source, "frozen", fuzz_seconds=1, max_attempts=5)
    state = Porter(config, agents).run()
    attempts = [call for call in agents.calls if call.split(":")[0] in ("porter", "debugger")]
    assert len(attempts) == 2, attempts
    assert "not finished" in state["stopped"]
    board = json.loads((Path(config.run_dir) / "control" / "board.json").read_text())
    assert any("frozen shared code" in str(task.get("failure")) for task in board["tasks"].values()), board


def test_a_persons_decisions_reach_every_later_prompt(source):
    from portwright.cli import main

    agents = FakeAgents()
    config = make_config(source, "decide", fuzz_seconds=1)
    assert main(["decide", "--run-dir", str(config.run_dir), "Return 0 for huge negative ndigits."]) == 0
    Porter(config, agents).run()
    assert agents.prompts and all("Return 0 for huge negative ndigits." in prompt for prompt in agents.prompts)


def test_usage_limit_pauses_and_resume_finishes_without_redoing_work(source):
    agents = FakeAgents(behaviors={"porter": limit_once})
    config = make_config(source, "limit", fuzz_seconds=1)
    with pytest.raises(LimitReached):
        Porter(config, agents).run()
    before = list(agents.calls)
    assert sum(call.startswith("test_writer") for call in before) == 1
    state = Porter(config, agents).run()
    assert state["stopped"] is None
    assert sum(call.startswith("planner") for call in agents.calls) == 1, "finished phases must not rerun"
    task = json.loads((Path(config.run_dir) / "control" / "board.json").read_text())["tasks"]["everything"]
    assert task["attempts"] == 0, "a usage limit must not use up an attempt"


def test_killed_mid_module_resumes_with_state_intact(source):
    """Simulates the harness dying right after an agent wrote work but before any merge."""
    config = make_config(source, "killed", fuzz_seconds=1)

    class Killed(BaseException):
        pass

    agents = FakeAgents()

    def die_after_writing(fake, session):
        fake.porter(session)
        raise Killed

    agents.behaviors["porter"] = die_after_writing
    with pytest.raises(Killed):
        Porter(config, agents).run()
    board_path = Path(config.run_dir) / "control" / "board.json"
    stored = json.loads(board_path.read_text())
    stored["tasks"]["everything"]["claim"]["expires"] = 0  # the lease has run out
    board_path.write_text(json.dumps(stored))
    fresh = FakeAgents()
    state = Porter(config, fresh).run()
    assert state["stopped"] is None
    assert sum(call.startswith("planner") for call in fresh.calls) == 0


def test_cleanup_that_changes_behavior_is_rejected(source):
    agents = FakeAgents(behaviors={"cleanup": regressing_cleanup})
    config = make_config(source, "badcleanup", fuzz_seconds=1)
    state = Porter(config, agents).run()
    assert not state["gates"]["cleanup"]["passed"]
    assert state["rejected"]
    core = Path(config.run_dir) / "port" / "crates" / "textkit-port-core" / "src" / "lib.rs"
    assert "div_mod_floor" in core.read_text(), "the bad change must not reach main"
    assert verdict(state)[0] == "matched on what was tested"


def test_parallel_tasks_merge_through_the_queue(source):
    """Two tasks with no dependency run at once, both editing shared files."""
    plan = {
        "tasks": [
            {"id": "first", "apis": ALL_APIS[:7], "depends_on": []},
            {"id": "second", "apis": ALL_APIS[7:], "depends_on": []},
        ],
        "shared_paths": [],
    }
    agents = FakeAgents(plan=plan)
    config = make_config(source, "parallel", max_agents=2, fuzz_seconds=1)
    state = Porter(config, agents).run()
    assert state["stopped"] is None, state["stopped"]
    assert sum(call.startswith("porter") for call in agents.calls) >= 2
    tasks = json.loads((Path(config.run_dir) / "control" / "board.json").read_text())["tasks"]
    assert all(task["status"] == "merged" for task in tasks.values())


def test_invalid_plan_is_rejected(source):
    def bad_plan(fake, session):
        from fake_agents import mount

        (mount(session, "/work/out") / "plan.json").write_text(json.dumps({"tasks": [{"id": "a", "apis": ["slug"]}]}))
        return None or __import__("portwright.agents", fromlist=["AgentResult"]).AgentResult("ok", "", {})

    agents = FakeAgents(behaviors={"planner": bad_plan})
    state = Porter(make_config(source, "badplan", design_rounds=1), agents).run()
    assert "no valid plan" in state["stopped"]
    assert not any(call.startswith("porter") for call in agents.calls)
