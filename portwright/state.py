"""Where a run keeps its files, and how state survives crashes."""

from __future__ import annotations

import fcntl
import json
import os
import tempfile
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path


def atomic_write(path: str | Path, text: str) -> None:
    """Replace a file in one step, so a crash can't leave half of it behind."""
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    handle, temporary = tempfile.mkstemp(dir=path.parent, prefix=f".{path.name}.")
    with os.fdopen(handle, "w", encoding="utf-8") as stream:
        stream.write(text)
    os.replace(temporary, path)


@contextmanager
def file_lock(path: str | Path):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, "a") as stream:
        fcntl.flock(stream, fcntl.LOCK_EX)
        try:
            yield
        finally:
            fcntl.flock(stream, fcntl.LOCK_UN)


class JsonStore:
    """A JSON file that many threads can safely read and update."""

    def __init__(self, path: str | Path, default: dict | None = None):
        self.path = Path(path)
        self.default = default if default is not None else {}
        self._lock_path = self.path.with_name(f".{self.path.name}.lock")

    def read(self) -> dict:
        with file_lock(self._lock_path):
            return self._read()

    def _read(self) -> dict:
        if not self.path.exists():
            return json.loads(json.dumps(self.default))
        return json.loads(self.path.read_text(encoding="utf-8"))

    def update(self, change) -> dict:
        """Apply ``change(data)`` and save. Returns the saved data."""
        with file_lock(self._lock_path):
            data = self._read()
            change(data)
            atomic_write(self.path, json.dumps(data, indent=1, sort_keys=True))
            return data


def append_capped(path: str | Path, text: str, max_lines: int = 200) -> None:
    """Add notes, keeping only the newest lines, so notes can't flood agent context."""
    path = Path(path)
    if path.exists() and text.strip() and text.strip() in path.read_text(encoding="utf-8"):
        return  # each retry repeats its notes; once is enough
    existing = path.read_text(encoding="utf-8").splitlines() if path.exists() else []
    lines = existing + text.strip().splitlines()
    atomic_write(path, "\n".join(lines[-max_lines:]) + "\n")


@dataclass(frozen=True)
class RunPaths:
    """A run's folders. ``control`` and ``tests_full`` never enter a sandbox."""

    root: Path

    @property
    def upstream_src(self) -> Path:
        return self.root / "upstream" / "src"

    @property
    def tests_full(self) -> Path:
        return self.root / "upstream" / "tests_full"

    @property
    def tests_visible(self) -> Path:
        return self.root / "upstream" / "tests_visible"

    @property
    def checks(self) -> Path:
        return self.root / "checks"

    @property
    def port(self) -> Path:
        return self.root / "port"

    @property
    def worktrees(self) -> Path:
        return self.root / "worktrees"

    @property
    def public(self) -> Path:
        return self.root / "state"

    @property
    def control(self) -> Path:
        return self.root / "control"

    @property
    def sessions(self) -> Path:
        return self.root / "sessions"

    @property
    def build(self) -> Path:
        return self.root / "build"

    def create(self) -> None:
        for path in (self.checks, self.worktrees, self.public, self.control, self.sessions, self.build):
            path.mkdir(parents=True, exist_ok=True)
