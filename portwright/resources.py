"""Machine resources a run shares with everything else on the host: memory, disk, and
the orchestrator's own liveness.

- ``Heartbeat``: the orchestrator touches a file every few seconds, and every agent and
  gate container watches it (``sandbox_watchdog.py``). A run killed with SIGKILL can't
  stop its containers, which used to run until their own time limit, up to about two
  hours (open item D).
- ``gate_memory_limit``: the memory each gate container gets, Rust builds included.
  Builds used to have no limit at all (open item E).
- ``AgentBudget``: the memory every agent container on the machine may use together,
  shared by all runs through a ledger file. Each run used to assume it had the machine
  to itself, so two runs at once could each give agents 60% of RAM (open item H).
- ``require_free_space``: agent sessions and gates refuse to start when the disk is
  nearly full, with a message that says so, instead of failing halfway through a build
  with an unrelated-looking error (open item T).
"""

from __future__ import annotations

import fcntl
import json
import os
import shutil
import threading
import time
import uuid
from contextlib import contextmanager
from pathlib import Path

from portwright.limits import LimitReached

# ---------------------------------------------------------------- memory


def total_memory_mb() -> int | None:
    try:
        with open("/proc/meminfo") as meminfo:
            return next(int(line.split()[1]) for line in meminfo if line.startswith("MemTotal:")) // 1024
    except (OSError, StopIteration, ValueError):
        return None


AGENTS_MEMORY_SHARE = 0.6  # all agent containers on the machine together
GATES_MEMORY_SHARE = 0.4  # one gate container: what the agents leave free
GATE_MEMORY_CEILING_MB = 6144
GATE_MEMORY_FLOOR_MB = 2048


def gate_memory_limit() -> str:
    """Memory for one gate container (contained check runs and Rust builds alike).

    ``PORTWRIGHT_GATE_MEMORY`` (Docker syntax, such as ``8g``) overrides it. Otherwise
    it is the share of RAM the agents leave free, between 2 GB and 6 GB.
    """
    configured = os.environ.get("PORTWRIGHT_GATE_MEMORY")
    if configured:
        return configured
    total = total_memory_mb()
    if total is None:
        return f"{GATE_MEMORY_CEILING_MB}m"
    return f"{max(GATE_MEMORY_FLOOR_MB, min(GATE_MEMORY_CEILING_MB, int(total * GATES_MEMORY_SHARE)))}m"


def megabytes(limit: str) -> int:
    """A Docker memory size (``512m``, ``6g``, ``1048576k`` or bytes) in megabytes."""
    text = limit.strip().lower()
    factors = {"k": 1 / 1024, "m": 1, "g": 1024, "t": 1024 * 1024}
    if text and text[-1] in factors:
        return int(float(text[:-1]) * factors[text[-1]])
    if text.endswith("b"):
        return megabytes(text[:-1])
    return int(text) // (1024 * 1024)


def budget_dir() -> Path:
    """Where runs on this machine keep their shared agent ledger. ``PORTWRIGHT_BUDGET_DIR`` moves it."""
    configured = os.environ.get("PORTWRIGHT_BUDGET_DIR")
    return Path(configured) if configured else Path.home() / ".cache" / "portwright"


def _process_start(pid: int) -> str | None:
    """The process's start time, so a reused process ID isn't taken for a live run."""
    try:
        with open(f"/proc/{pid}/stat") as stat:
            return stat.read().rsplit(")", 1)[1].split()[19]
    except (OSError, IndexError):
        return None


def _alive(entry: dict) -> bool:
    pid = entry.get("pid")
    if not isinstance(pid, int):
        return False
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        pass  # someone else's process, still running
    started = entry.get("started")
    return started is None or _process_start(pid) in (None, started)


class AgentBudget:
    """The memory all agent containers on this machine may hold at once, across runs.

    Each session reserves its container's memory limit in a ledger file before it
    starts and gives it back when it ends. A session that doesn't fit waits. Entries
    belong to a process: if that process dies, even by SIGKILL, its entries are
    ignored and dropped, so a killed run can't hold memory forever.
    """

    def __init__(self, directory: str | Path | None = None, total_mb: int | None = None, poll_seconds: float = 15.0):
        self.directory = Path(directory) if directory else budget_dir()
        machine = total_memory_mb()
        self.total_mb = total_mb if total_mb is not None else int((machine or 16384) * AGENTS_MEMORY_SHARE)
        self.poll_seconds = poll_seconds
        self._pid = os.getpid()
        self._started = _process_start(self._pid)

    @property
    def ledger(self) -> Path:
        return self.directory / "agents.json"

    @contextmanager
    def _locked(self):
        self.directory.mkdir(parents=True, exist_ok=True)
        with open(self.directory / "agents.lock", "a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            try:
                try:
                    entries = json.loads(self.ledger.read_text())
                except (OSError, ValueError):
                    entries = []
                entries = [entry for entry in entries if isinstance(entry, dict) and _alive(entry)]
                yield entries
                temporary = self.ledger.with_suffix(f".{uuid.uuid4().hex}.tmp")
                temporary.write_text(json.dumps(entries, indent=1))
                os.replace(temporary, self.ledger)
            finally:
                fcntl.flock(lock, fcntl.LOCK_UN)

    def in_use_mb(self) -> int:
        with self._locked() as entries:
            return sum(entry["mb"] for entry in entries)

    def try_reserve(self, mb: int, label: str = "") -> str | None:
        """Reserve ``mb`` if it fits, and return the reservation's key. A request larger
        than the whole budget is granted only when nothing else holds any."""
        key = uuid.uuid4().hex
        with self._locked() as entries:
            used = sum(entry["mb"] for entry in entries)
            if entries and used + mb > self.total_mb:
                return None
            entries.append({"key": key, "pid": self._pid, "started": self._started, "mb": mb, "label": label, "since": time.time()})
        return key

    def release(self, key: str) -> None:
        with self._locked() as entries:
            entries[:] = [entry for entry in entries if entry.get("key") != key]

    @contextmanager
    def reserve(self, mb: int, label: str = "", on_wait=None):
        """Hold ``mb`` of the machine's agent memory for the duration of the block."""
        key = self.try_reserve(mb, label)
        waited = False
        while key is None:
            if on_wait and not waited:
                on_wait(f"waiting for agent memory: {self.in_use_mb()} of {self.total_mb} MB is held by other sessions on this machine")
            waited = True
            time.sleep(self.poll_seconds)
            key = self.try_reserve(mb, label)
        try:
            yield key
        finally:
            self.release(key)


# ---------------------------------------------------------------- liveness

HEARTBEAT_ENV = "PORTWRIGHT_HEARTBEAT"
HEARTBEAT_SECONDS = 10.0
WATCHDOG_SCRIPT = Path(__file__).resolve().parent / "sandbox_watchdog.py"


def heartbeat_max_age() -> int:
    """How long a heartbeat may go unchanged before containers end themselves."""
    return int(os.environ.get("PORTWRIGHT_HEARTBEAT_MAX_AGE", "180"))


class Heartbeat:
    """Touches a file every few seconds for as long as the orchestrator lives.

    The file is touched in place, never replaced, because containers see it through
    a bind mount of its folder. ``stop`` deletes it, which ends any container still
    watching it.
    """

    def __init__(self, path: str | Path, interval: float = HEARTBEAT_SECONDS):
        self.path = Path(path).resolve()
        self.interval = interval
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None

    def beat(self) -> None:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.path.touch()
        os.utime(self.path)

    def start(self) -> None:
        self.beat()
        os.environ[HEARTBEAT_ENV] = str(self.path)

        def loop():
            while not self._stop.wait(self.interval):
                try:
                    self.beat()
                except OSError:
                    pass  # a full disk mustn't kill the thread; the next beat may work
        self._thread = threading.Thread(target=loop, name="portwright-heartbeat", daemon=True)
        self._thread.start()

    def stop(self) -> None:
        self._stop.set()
        if self._thread:
            self._thread.join(timeout=5)
        if os.environ.get(HEARTBEAT_ENV) == str(self.path):
            del os.environ[HEARTBEAT_ENV]
        self.path.unlink(missing_ok=True)


def heartbeat_path() -> Path | None:
    configured = os.environ.get(HEARTBEAT_ENV)
    return Path(configured) if configured else None


def watchdog_mounts(container_dir: str | None = None) -> tuple[list[str], str | None]:
    """Docker arguments that make the heartbeat visible in a container, and the path it
    has there. Without a heartbeat (tests, manual runs) there is nothing to watch."""
    path = heartbeat_path()
    if path is None:
        return [], None
    inside = f"{container_dir}/{path.name}" if container_dir else str(path)
    return ["-v", f"{path.parent}:{container_dir or path.parent}:ro"], inside


def watched(command: list[str], heartbeat_inside: str | None, watchdog_inside: str | None = None) -> list[str]:
    """``command``, with the watchdog started in the background first.

    The command runs as a child of ``sh``, not in its place: PID 1 in a container
    ignores signals sent from inside it, so a command exec'd as PID 1 would outlive
    the watchdog's kill. ``sh`` exits with the command's status.
    """
    if heartbeat_inside is None:
        return command
    script = watchdog_inside or str(WATCHDOG_SCRIPT)
    return [
        "sh", "-c", 'python3 "$1" "$2" "$3" & shift 3; "$@"',
        "portwright-watchdog", script, heartbeat_inside, str(heartbeat_max_age()), *command,
    ]


# ---------------------------------------------------------------- disk


class LowDiskSpace(LimitReached):
    """The disk is nearly full. The run pauses with progress saved; free space and resume.

    A subclass of ``LimitReached``, so every place that pauses a run for a usage limit
    pauses it for this too, and a task in flight goes back in the queue unharmed.
    """


def minimum_free_gb() -> float:
    """``PORTWRIGHT_MIN_FREE_GB``, default 10: a full test run leaves about 9 GB of
    builds, and the isodate run failed to start with 3.3 GB left."""
    return float(os.environ.get("PORTWRIGHT_MIN_FREE_GB", "10"))


def require_free_space(path: str | Path, what: str) -> None:
    """Raise ``LowDiskSpace`` if the filesystem holding ``path`` is below the floor."""
    floor = minimum_free_gb()
    if floor <= 0:
        return
    probe = Path(path).resolve()
    while not probe.exists() and probe != probe.parent:
        probe = probe.parent
    free_gb = shutil.disk_usage(probe).free / 1024**3
    if free_gb < floor:
        raise LowDiskSpace(
            f"not starting {what}: only {free_gb:.1f} GB free on the disk holding {probe} "
            f"(minimum {floor:g} GB, PORTWRIGHT_MIN_FREE_GB). Free some space, for example old "
            "runs' build/ folders or target/, and run the same command again to resume."
        )


def remove_tree(path: str | Path) -> None:
    """Delete a build folder. Missing folders and files Docker left owned by root are skipped."""
    shutil.rmtree(path, ignore_errors=True)
