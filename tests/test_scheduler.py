"""Scheduling must stay correct when agents die, conflict, or hit usage limits."""

import subprocess
import threading
from types import SimpleNamespace

import pytest

from portwright.agents import LimitReached
from portwright.orchestrator import Porter
from portwright.scheduler import BLOCKED, CLAIMED, MERGED, PENDING, RETRY, TaskBoard, Workspaces, run_board


class Clock:
    def __init__(self):
        self.now = 1000.0

    def __call__(self):
        return self.now


def make_board(tmp_path, clock=None):
    return TaskBoard(tmp_path / "board.json", lease_seconds=100, clock=clock or Clock())


def test_tasks_start_only_after_dependencies_merge(tmp_path):
    board = make_board(tmp_path)
    board.add("a", ["x"])
    board.add("b", ["y"], ["a"])
    assert board.ready() == ["a"]
    assert board.claim("a", "w1")
    assert board.ready() == []
    board.finish("a", MERGED)
    assert board.ready() == ["b"]


def test_only_one_claimant_wins(tmp_path):
    board = make_board(tmp_path)
    board.add("a", [])
    wins = []
    threads = [threading.Thread(target=lambda n=n: wins.append(board.claim("a", f"w{n}"))) for n in range(20)]
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join()
    assert sum(wins) == 1


def test_a_dead_agents_claim_expires_and_the_task_is_reassigned(tmp_path):
    clock = Clock()
    board = make_board(tmp_path, clock)
    board.add("a", [])
    assert board.claim("a", "dead-agent")
    assert board.tasks()["a"]["status"] == CLAIMED
    clock.now += 99
    assert board.ready() == []
    clock.now += 2
    assert board.ready() == ["a"]
    assert board.claim("a", "new-agent")


def test_state_survives_a_restart(tmp_path):
    board = make_board(tmp_path)
    board.add("a", [])
    board.finish("a", MERGED)
    assert make_board(tmp_path).tasks()["a"]["status"] == MERGED


def test_a_retry_counts_an_attempt_and_keeps_the_failure(tmp_path):
    board = make_board(tmp_path)
    board.add("a", [])
    board.claim("a", "w")
    board.finish("a", RETRY, "differs at x")
    task = board.tasks()["a"]
    assert (task["status"], task["attempts"], task["failure"]) == (PENDING, 1, "differs at x")


def test_parallel_board_runs_independent_tasks_at_once(tmp_path):
    board = make_board(tmp_path)
    for name in "abc":
        board.add(name, [])
    board.add("d", [], ["a", "b", "c"])
    barrier = threading.Barrier(3, timeout=10)
    order = []

    def worker(task_id):
        if task_id != "d":
            barrier.wait()  # fails unless all three really run together
        order.append(task_id)
        return MERGED

    run_board(board, worker, max_agents=3)
    assert order[-1] == "d"
    assert set(order) == set("abcd")


def test_sequential_board_runs_one_at_a_time(tmp_path):
    board = make_board(tmp_path)
    for name in "abc":
        board.add(name, [])
    active = []
    most = []

    def worker(task_id):
        active.append(task_id)
        most.append(len(active))
        active.remove(task_id)
        return MERGED

    run_board(board, worker, max_agents=1)
    assert max(most) == 1


def test_blocked_task_holds_only_its_dependents(tmp_path):
    board = make_board(tmp_path)
    board.add("bad", [])
    board.add("after-bad", [], ["bad"])
    board.add("other", [])
    run_board(board, lambda t: BLOCKED if t == "bad" else MERGED, max_agents=2)
    tasks = board.tasks()
    assert tasks["bad"]["status"] == BLOCKED
    assert tasks["after-bad"]["status"] == PENDING
    assert tasks["other"]["status"] == MERGED


def test_usage_limit_releases_the_task_without_using_an_attempt(tmp_path):
    board = make_board(tmp_path)
    board.add("a", [])

    def worker(_):
        raise LimitReached("limit")

    with pytest.raises(LimitReached):
        run_board(board, worker)
    task = board.tasks()["a"]
    assert (task["status"], task["attempts"]) == (PENDING, 0)


def test_a_crashing_worker_blocks_its_task_and_the_rest_continue(tmp_path):
    board = make_board(tmp_path)
    board.add("crash", [])
    board.add("fine", [])

    def worker(task_id):
        if task_id == "crash":
            raise ValueError("bug")
        return MERGED

    errors = []
    run_board(board, worker, max_agents=2, on_error=lambda t, e: errors.append(t))
    assert errors == ["crash"]
    assert board.tasks()["fine"]["status"] == MERGED


def git(repo, *args):
    return subprocess.run(
        ["git", "-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false", *args],
        cwd=repo, check=True, capture_output=True, text=True,
    ).stdout


@pytest.fixture
def workspaces(tmp_path):
    repo = tmp_path / "repo"
    repo.mkdir()
    git(repo, "init", "-q", "-b", "main")
    (repo / "a.txt").write_text("one\n")
    (repo / "b.txt").write_text("one\n")
    git(repo, "add", "-A")
    git(repo, "commit", "-qm", "start")
    return Workspaces(repo, tmp_path / "worktrees")


def always(path):
    return True, ""


def test_merge_requires_the_gate_to_pass(workspaces):
    path = workspaces.create("t1")
    (path / "a.txt").write_text("changed\n")
    assert workspaces.commit(path, "change")
    result = workspaces.merge("t1", path, lambda _: (False, "gate says no"))
    assert (result.merged, result.reason, result.detail) == (False, "gate", "gate says no")
    assert (workspaces.repo / "a.txt").read_text() == "one\n"
    assert workspaces.merge("t1", path, always).merged
    assert (workspaces.repo / "a.txt").read_text() == "changed\n"


def test_build_leftovers_in_main_dont_block_a_merge(workspaces):
    """Fuzzing builds in the main checkout and leaves an untracked Cargo.lock behind."""
    path = workspaces.create("t1")
    (path / "Cargo.lock").write_text("from the task\n")
    assert workspaces.commit(path, "add lock")
    (workspaces.repo / "Cargo.lock").write_text("left by a gate\n")
    assert workspaces.merge("t1", path, always).merged
    assert (workspaces.repo / "Cargo.lock").read_text() == "from the task\n"


def test_a_rejected_fix_reaches_the_next_debugger(workspaces, tmp_path):
    """The next round must see the earlier work and why it was rejected."""
    prompts, seen = [], []

    def agent(_role, task, prompt, _mounts):
        prompts.append(prompt)
        path = workspaces.worktrees / task
        seen.append((path / "a.txt").read_text())
        (path / "a.txt").write_text((path / "a.txt").read_text() + "fix\n")

    gate_results = iter([(False, "needs a second fix"), (True, "")])
    verdicts = iter([
        SimpleNamespace(passed=False, for_agent=lambda: "main fails"),
        SimpleNamespace(passed=False, for_agent=lambda: "main fails"),
        SimpleNamespace(passed=True),
    ])
    stub = SimpleNamespace(
        config=SimpleNamespace(fix_rounds=3), workspaces=workspaces, _agent=agent,
        _flush_regressions=lambda: None, _port_mounts=lambda *_: {}, _log=lambda _: None,
        _gate_full=lambda *_: next(gate_results), _target=lambda name: tmp_path / "targets" / name,
    )
    Porter._fix_until(stub, "x", lambda: next(verdicts), "Fix it.")

    assert seen == ["one\n", "one\nfix\n"]
    assert "needs a second fix" not in prompts[0]
    assert "needs a second fix" in prompts[1]
    assert (workspaces.repo / "a.txt").read_text() == "one\nfix\nfix\n"


def test_non_conflicting_parallel_work_both_merge_after_rebasing(workspaces):
    first = workspaces.create("t1")
    second = workspaces.create("t2")
    (first / "a.txt").write_text("from t1\n")
    (second / "b.txt").write_text("from t2\n")
    workspaces.commit(first, "t1")
    workspaces.commit(second, "t2")
    seen = []

    def gate(path):
        seen.append((path / "a.txt").read_text())
        return True, ""

    assert workspaces.merge("t1", first, gate).merged
    assert workspaces.merge("t2", second, gate).merged
    assert seen[1] == "from t1\n", "the gate must run on top of work merged earlier"
    assert (workspaces.repo / "b.txt").read_text() == "from t2\n"


def test_forced_merge_conflict_is_reported_and_leaves_main_clean(workspaces):
    first = workspaces.create("t1")
    second = workspaces.create("t2")
    (first / "a.txt").write_text("from t1\n")
    (second / "a.txt").write_text("from t2\n")
    workspaces.commit(first, "t1")
    workspaces.commit(second, "t2")
    assert workspaces.merge("t1", first, always).merged
    result = workspaces.merge("t2", second, always)
    assert (result.merged, result.reason) == (False, "conflict")
    assert (workspaces.repo / "a.txt").read_text() == "from t1\n"
    assert git(second, "status", "--short") == ""
    workspaces.discard("t2")
    again = workspaces.create("t2")
    (again / "a.txt").write_text("redone\n")
    workspaces.commit(again, "redo")
    assert workspaces.merge("t2", again, always).merged


def test_nothing_to_commit_is_reported(workspaces):
    path = workspaces.create("t1")
    assert not workspaces.commit(path, "nothing")
    assert not workspaces.changed_since_branch(path)


def test_resume_keeps_existing_worktree_contents(workspaces):
    path = workspaces.create("t1")
    (path / "work.txt").write_text("partial")
    again = workspaces.create("t1")
    assert (again / "work.txt").read_text() == "partial"


def test_a_resumed_run_frees_claims_left_by_a_dead_run(tmp_path):
    board = TaskBoard(tmp_path / "board.json")
    board.add("a", ["f"])
    assert board.claim("a", "dead-worker")
    assert board.ready() == []  # still held for the whole lease
    board.release_all()
    assert board.ready() == ["a"]


def test_a_merge_conflict_doesnt_use_up_an_attempt(tmp_path):
    from portwright.scheduler import BLOCKED, CONFLICT, MAX_CONFLICTS, PENDING

    board = make_board(tmp_path)
    board.add("a", ["x"])
    for _ in range(MAX_CONFLICTS):
        assert board.claim("a", "w")
        board.finish("a", CONFLICT, "conflict in lib.rs")
        assert board.tasks()["a"]["status"] == PENDING
    assert board.tasks()["a"]["attempts"] == 0
    assert board.claim("a", "w")
    board.finish("a", CONFLICT, "conflict in lib.rs")
    assert board.tasks()["a"]["status"] == BLOCKED, "endless races still stop"
