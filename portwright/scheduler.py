"""Decides what runs when. Plain code does this, not a manager agent.

Runs one task at a time by default. With ``max_agents`` above 1, tasks that don't
depend on each other run at once, each in its own git worktree. Finished work goes
through a one-at-a-time merge queue, because two changes that pass separately can
still break when combined.
"""

from __future__ import annotations

import subprocess
import threading
import time
import uuid
from concurrent.futures import FIRST_COMPLETED, ThreadPoolExecutor, wait
from dataclasses import dataclass
from pathlib import Path

from portwright.agents import LimitReached
from portwright.state import JsonStore

PENDING = "pending"
CLAIMED = "claimed"
MERGED = "merged"
BLOCKED = "blocked"
RETRY = "retry"


class TaskBoard:
    """Tracks every task, and survives crashes because it lives on disk."""

    def __init__(self, path: str | Path, lease_seconds: float = 3 * 60 * 60, clock=time.time):
        self.store = JsonStore(path, {"tasks": {}})
        self.lease_seconds = lease_seconds
        self.clock = clock

    def add(self, task_id: str, apis: list[str], depends_on: list[str] | None = None, notes: str = "") -> None:
        def change(data):
            data["tasks"].setdefault(
                task_id,
                {"apis": apis, "depends_on": depends_on or [], "notes": notes,
                 "status": PENDING, "attempts": 0, "failure": "", "claim": None},
            )
        self.store.update(change)

    def tasks(self) -> dict:
        return self.store.read()["tasks"]

    def _expire(self, data) -> None:
        for task in data["tasks"].values():
            if task["status"] == CLAIMED and task["claim"]["expires"] <= self.clock():
                task["status"] = PENDING
                task["claim"] = None

    def ready(self) -> list[str]:
        """List tasks whose dependencies have merged and that nobody holds."""
        found = []

        def change(data):
            self._expire(data)
            for task_id, task in sorted(data["tasks"].items()):
                dependencies = (data["tasks"].get(dep, {}).get("status") for dep in task["depends_on"])
                if task["status"] == PENDING and all(status == MERGED for status in dependencies):
                    found.append(task_id)
        self.store.update(change)
        return found

    def claim(self, task_id: str, owner: str) -> bool:
        """Take a task. Exactly one caller wins, and the claim expires if its owner dies."""
        won = []

        def change(data):
            self._expire(data)
            task = data["tasks"][task_id]
            if task["status"] == PENDING:
                task["status"] = CLAIMED
                task["claim"] = {"owner": owner, "expires": self.clock() + self.lease_seconds}
                won.append(True)
        self.store.update(change)
        return bool(won)

    def finish(self, task_id: str, status: str, failure: str = "") -> None:
        def change(data):
            task = data["tasks"][task_id]
            task["claim"] = None
            if status == RETRY:
                task["status"] = PENDING
                task["attempts"] += 1
                task["failure"] = failure
            else:
                task["status"] = status
                if failure:
                    task["failure"] = failure
        self.store.update(change)

    def release(self, task_id: str) -> None:
        """Give a task back without using up an attempt, as when a usage limit hits."""
        def change(data):
            task = data["tasks"][task_id]
            if task["status"] == CLAIMED:
                task["status"] = PENDING
                task["claim"] = None
        self.store.update(change)

    def release_all(self) -> None:
        """Free every claim. Only one orchestrator runs per run, so at startup any claim
        belongs to a run that died, and waiting out its lease would stall the resume."""
        def change(data):
            for task in data["tasks"].values():
                if task["status"] == CLAIMED:
                    task["status"] = PENDING
                    task["claim"] = None
        self.store.update(change)

    def unfinished(self) -> dict:
        return {k: v for k, v in self.tasks().items() if v["status"] != MERGED}


def run_board(board: TaskBoard, worker, *, max_agents: int = 1, on_error=None) -> None:
    """Run tasks until none can start. ``worker(task_id)`` returns merged, retry, or blocked.

    If a usage limit hits, running tasks finish, nothing new starts, and the limit is
    raised so the run can pause and resume later.
    """
    limit: LimitReached | None = None
    with ThreadPoolExecutor(max_workers=max_agents) as pool:
        running = {}
        while True:
            if limit is None:
                for task_id in board.ready():
                    if len(running) >= max_agents:
                        break
                    if board.claim(task_id, f"worker-{uuid.uuid4().hex[:6]}"):
                        running[pool.submit(worker, task_id)] = task_id
            if not running:
                break
            finished, _ = wait(running, return_when=FIRST_COMPLETED)
            for future in finished:
                task_id = running.pop(future)
                try:
                    result = future.result()
                    if isinstance(result, tuple):
                        board.finish(task_id, result[0], result[1])
                    else:
                        board.finish(task_id, result)
                except LimitReached as error:
                    board.release(task_id)
                    limit = error
                except Exception as error:  # a harness error shouldn't kill other tasks
                    if on_error:
                        on_error(task_id, error)
                    board.finish(task_id, BLOCKED, f"harness error: {error}")
    if limit is not None:
        raise limit


@dataclass
class MergeResult:
    merged: bool
    reason: str = ""  # conflict or gate
    detail: str = ""


class Workspaces:
    """Gives each task its own git worktree, and merges finished work in a queue."""

    def __init__(self, repo: str | Path, worktrees: str | Path, branch: str = "main"):
        self.repo = Path(repo).resolve()
        self.worktrees = Path(worktrees).resolve()
        self.branch = branch
        self._merge_lock = threading.Lock()

    def _git(self, *args, cwd=None, check=True) -> subprocess.CompletedProcess:
        return subprocess.run(
            ["git", "-c", "user.name=portwright", "-c", "user.email=portwright@users.noreply.github.com",
             "-c", "commit.gpgsign=false", *args],
            cwd=cwd or self.repo, capture_output=True, text=True, check=check,
        )

    def create(self, name: str, start: str | None = None) -> Path:
        """Make a worktree from the main branch, or from ``start`` to continue earlier work."""
        path = self.worktrees / name
        if path.exists():  # resuming after a crash: keep any work already there
            return path
        self._git("worktree", "prune")
        self._git("worktree", "add", "--force", "-B", f"task/{name}", str(path), start or self.branch)
        return path

    def revision(self, path: str | Path) -> str:
        return self._git("rev-parse", "HEAD", cwd=path).stdout.strip()

    def keep(self, revision: str) -> None:
        """Stop git from garbage-collecting a rejected commit that later work builds on."""
        self._git("update-ref", "refs/portwright/last-rejected", revision)

    def commit(self, path: str | Path, message: str) -> bool:
        """Commit everything in a worktree. Returns False if nothing changed."""
        self._git("add", "-A", cwd=path)
        if self._git("diff", "--cached", "--quiet", cwd=path, check=False).returncode == 0:
            return False
        self._git("commit", "-qm", message, cwd=path)
        return True

    def changed_since_branch(self, path: str | Path) -> bool:
        ahead = self._git("rev-list", "--count", f"{self.branch}..HEAD", cwd=path).stdout.strip()
        return ahead != "0"

    def merge(self, name: str, path: str | Path, gate) -> MergeResult:
        """Rebase on the latest work, rerun the gate there, and only then merge."""
        with self._merge_lock:
            rebased = self._git("rebase", self.branch, cwd=path, check=False)
            if rebased.returncode != 0:
                detail = (rebased.stdout + rebased.stderr)[-2000:]
                self._git("rebase", "--abort", cwd=path, check=False)
                return MergeResult(False, "conflict", detail)
            ok, detail = gate(Path(path))
            if not ok:
                return MergeResult(False, "gate", detail)
            self._git("merge", "--ff-only", "-q", f"task/{name}")
            return MergeResult(True)

    def discard(self, name: str) -> None:
        path = self.worktrees / name
        self._git("worktree", "remove", "--force", str(path), check=False)
        self._git("worktree", "prune", check=False)
        self._git("branch", "-D", f"task/{name}", check=False)

    def head(self) -> str:
        return self._git("rev-parse", self.branch).stdout.strip()
