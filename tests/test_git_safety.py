"""Nothing an agent writes in its worktree may run when the harness uses git on the host."""

import os
import shutil
import subprocess

import pytest

from portwright.scheduler import Workspaces


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
    git(repo, "add", "-A")
    git(repo, "commit", "-qm", "start")
    return Workspaces(repo, tmp_path / "worktrees")


def plant(path, marker):
    """What a porter could do from inside its sandbox: replace the .git file with a
    repository of its own whose hook and fsmonitor write a marker file."""
    script = f"#!/bin/sh\necho ran >> {marker}\nexit 0\n"
    (path / ".git").unlink()
    fake = path / ".git"
    subprocess.run(["git", "init", "-q", str(path)], check=True, capture_output=True)
    hook = fake / "hooks" / "pre-commit"
    hook.parent.mkdir(exist_ok=True)
    hook.write_text(script)
    hook.chmod(0o755)
    monitor = fake / "monitor.sh"
    monitor.write_text(script)
    monitor.chmod(0o755)
    with (fake / "config").open("a") as config:
        config.write(f"[core]\n\tfsmonitor = {monitor}\n\thooksPath = {fake / 'hooks'}\n")


def test_a_planted_git_folder_runs_nothing_and_the_work_still_lands(workspaces, tmp_path):
    marker = tmp_path / "ran.txt"
    path = workspaces.create("t1")
    (path / "a.txt").write_text("ported\n")
    plant(path, marker)

    assert workspaces.commit(path, "porter: t1, attempt 1")
    assert workspaces.changed_since_branch(path)
    assert workspaces.merge("t1", path, lambda _p: (True, "")).merged

    assert not marker.exists(), "the agent's hook or fsmonitor ran on the host"
    assert (workspaces.repo / "a.txt").read_text() == "ported\n"
    tracked = git(workspaces.repo, "ls-files")
    assert ".git" not in tracked.split()


def test_git_never_trusts_the_worktrees_own_pointer(workspaces, tmp_path):
    """Even a .git file pointing somewhere else is ignored."""
    elsewhere = tmp_path / "elsewhere"
    git(tmp_path, "init", "-q", str(elsewhere))
    path = workspaces.create("t1")
    (path / ".git").write_text(f"gitdir: {elsewhere / '.git'}\n")
    (path / "a.txt").write_text("ported\n")
    assert workspaces.commit(path, "porter: t1, attempt 1")
    assert workspaces.merge("t1", path, lambda _p: (True, "")).merged
    assert (workspaces.repo / "a.txt").read_text() == "ported\n"
    assert git(elsewhere, "rev-list", "--all") == "", "nothing was committed to the agent's repository"


@pytest.mark.skipif(shutil.which("git") is None or os.name != "posix", reason="needs git and POSIX hooks")
def test_the_main_checkouts_own_hooks_are_off_too(workspaces, tmp_path):
    marker = tmp_path / "ran.txt"
    hook = workspaces.repo / ".git" / "hooks" / "post-merge"
    hook.write_text(f"#!/bin/sh\necho ran >> {marker}\n")
    hook.chmod(0o755)
    path = workspaces.create("t1")
    (path / "a.txt").write_text("ported\n")
    assert workspaces.commit(path, "work")
    assert workspaces.merge("t1", path, lambda _p: (True, "")).merged
    assert not marker.exists()
