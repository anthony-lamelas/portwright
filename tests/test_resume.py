"""A stopped or ruled-on run must pick up where it was, without paying twice for work."""

import subprocess
from types import SimpleNamespace

import pytest

import portwright.orchestrator as orchestrator
from portwright.orchestrator import Config, Porter
from portwright.scheduler import BLOCKED, MERGED, PENDING, RETRY, TaskBoard, Workspaces, recover_landed


def git(repo, *args):
    return subprocess.run(
        ["git", "-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false", *args],
        cwd=repo, check=True, capture_output=True, text=True,
    ).stdout


def init_repo(repo):
    repo.mkdir(parents=True, exist_ok=True)
    git(repo, "init", "-q", "-b", "main")
    (repo / "a.txt").write_text("one\n")
    git(repo, "add", "-A")
    git(repo, "commit", "-qm", "start")


@pytest.fixture
def workspaces(tmp_path):
    init_repo(tmp_path / "repo")
    return Workspaces(tmp_path / "repo", tmp_path / "worktrees")


def always(_path):
    return True, ""


def finish_work(workspaces, name, text="changed\n"):
    path = workspaces.create(name)
    (path / "a.txt").write_text(text)
    assert workspaces.commit(path, f"porter: {name}, attempt 1")
    return path


# ----------------------------------------------------------------- open item K


def test_a_merge_the_board_never_recorded_is_found_on_resume(workspaces, tmp_path):
    board = TaskBoard(tmp_path / "board.json")
    board.add("t1", ["f"])
    assert board.claim("t1", "worker")
    path = finish_work(workspaces, "t1")
    assert workspaces.merge("t1", path, always).merged
    # The process stops here, before the board hears about the merge.
    board.release_all()
    assert board.tasks()["t1"]["status"] == PENDING

    assert recover_landed(board, workspaces) == ["t1"]
    assert board.tasks()["t1"]["status"] == MERGED
    assert recover_landed(board, workspaces) == [], "the marker is cleaned up once recorded"


def test_untouched_or_rejected_work_is_not_taken_for_a_merge(workspaces, tmp_path):
    board = TaskBoard(tmp_path / "board.json")
    for name in ("empty", "rejected", "stopped_mid_merge"):
        board.add(name, ["f"])
    # A session that changed nothing leaves a worktree at main's own commit.
    workspaces.create("empty")
    # A rejected attempt never reaches main.
    rejected = finish_work(workspaces, "rejected")
    assert not workspaces.merge("rejected", rejected, lambda _: (False, "no")).merged

    # A stop after the gate passed but before main moved: the marker exists, main doesn't have it.
    stopped = finish_work(workspaces, "stopped_mid_merge", "other\n")
    git(workspaces.repo, "update-ref", "refs/portwright/landing/stopped_mid_merge", workspaces.revision(stopped))

    assert recover_landed(board, workspaces) == []
    assert all(task["status"] == PENDING for task in board.tasks().values())


def test_a_merge_that_failed_as_a_harness_error_is_still_recovered(workspaces, tmp_path):
    """Cleanup after a merge can throw; the task is then blocked though its work is on main."""
    board = TaskBoard(tmp_path / "board.json")
    board.add("t1", ["f"])
    path = finish_work(workspaces, "t1")
    assert workspaces.merge("t1", path, always).merged
    board.finish("t1", BLOCKED, "harness error: worktree remove failed")
    assert recover_landed(board, workspaces) == ["t1"]
    assert board.tasks()["t1"]["status"] == MERGED


# ----------------------------------------------------------------- open item N


@pytest.fixture
def porter(tmp_path, monkeypatch):
    config = Config(
        lib="textkit", module="textkit", source=tmp_path / "src", package_path="textkit",
        tests_path="tests", url="https://example.invalid", run_dir=tmp_path / "run",
    )
    porter = Porter(config, backend=None)
    init_repo(porter.paths.port)
    porter.board.add("t1", ["textkit.f"])
    porter.project = lambda: None
    porter.sessions = []

    def agent(role, label, prompt, mounts, apis=None):
        porter.sessions.append((role, label, prompt))
        (porter.workspaces.worktrees / "t1" / "a.txt").write_text("fixed\n")
        return SimpleNamespace(status="ok", text="")

    porter._agent = agent
    porter._port_mounts = lambda *_args, **_kwargs: {}

    def fake_verify(project, path, site, **_kwargs):
        approved = "Diff:float" in porter.run_state.read()["approved_differences"]
        text = (path / "a.txt").read_text()
        passed = text == "fixed\n" or (text == "close\n" and approved)
        porter.gate_runs = getattr(porter, "gate_runs", 0) + 1
        return SimpleNamespace(passed=passed, gates=[], for_agent=lambda: "" if passed else "Diff:float differs")

    monkeypatch.setattr(orchestrator, "verify", fake_verify)
    return porter


def reject_first_attempt(porter):
    """The first session writes work that differs only by a difference a person may approve."""
    def agent(role, label, prompt, mounts, apis=None):
        porter.sessions.append((role, label, prompt))
        (porter.workspaces.worktrees / "t1" / "a.txt").write_text("close\n")
        return SimpleNamespace(status="ok", text="")

    real, porter._agent = porter._agent, agent
    assert porter.board.claim("t1", "w")
    outcome = porter._work_task("t1")
    assert outcome[0] == RETRY
    porter.board.finish("t1", *outcome)
    porter._agent = real


def test_an_approval_lets_the_rejected_work_merge_without_a_session(porter):
    reject_first_attempt(porter)
    porter.run_state.update(lambda d: d["approved_differences"].append("Diff:float"))

    assert porter.board.claim("t1", "w")
    assert porter._work_task("t1") == MERGED
    assert len(porter.sessions) == 1, "no second session after the ruling"
    assert (porter.paths.port / "a.txt").read_text() == "close\n"


def test_without_a_ruling_the_next_attempt_runs_a_session_as_before(porter):
    reject_first_attempt(porter)
    runs_before = porter.gate_runs

    assert porter.board.claim("t1", "w")
    assert porter._work_task("t1") == MERGED
    assert len(porter.sessions) == 2
    assert porter.gate_runs == runs_before + 1, "no extra gate run when nothing changed"


def test_a_ruling_that_isnt_enough_still_gets_a_session_with_the_current_failure(porter):
    reject_first_attempt(porter)
    porter.run_state.update(lambda d: d["approved_differences"].append("Diff:unrelated"))

    assert porter.board.claim("t1", "w")
    assert porter._work_task("t1") == MERGED
    assert len(porter.sessions) == 2
    assert "Diff:float differs" in porter.sessions[1][2]
