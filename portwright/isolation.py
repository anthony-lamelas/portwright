"""Runs code that agents wrote or built inside the toolchain image, never on the host.

Gates import the port's compiled module, run the test writer's check files, and run
upstream tests and benchmarks against the port. All of that is agent code. Run on the
host, it could rewrite the run's state (approvals, locked checks, the reference)
before the gate reports. Here each such process runs in a throwaway container with no
network, the harness and its inputs mounted read-only at their host paths, and one
writable scratch folder for results.

Inside that container (or an agent's sandbox) ``PORTER_IN_SANDBOX`` is set, so the
same code runs directly instead of starting another container.

``PORTWRIGHT_GATES_ON_HOST=1`` runs everything on the host as before. The test suite
uses it when the toolchain image isn't built. Real runs must not.
"""

from __future__ import annotations

import os
import shutil
import subprocess
import tempfile
import uuid
from pathlib import Path

from portwright import resources
from portwright.toolchain import IMAGE, in_sandbox

REPO_ROOT = Path(__file__).resolve().parents[1]
CONTAINER_PYTHON = "python3"
# Settings that reach the processes inside. Everything else in the host environment stays out.
PASSED_ENV_PREFIXES = ("PORTWRIGHT_WORKER_",)
KILL_GRACE_SECONDS = 10
TIMEOUT_EXIT = 124  # coreutils timeout


# The port's container: what its workers may use, and where the comparison finds them.
PORT_MEMORY_MB = 2048
PORT_SOCKET_DIR = "/portwright-port"
PORT_SOCKET = f"{PORT_SOCKET_DIR}/workers.sock"
COMPARE_DIR = REPO_ROOT / "portwright" / "compare"


class PortHost:
    """A port's comparison workers, in a container of their own.

    It holds only the port's built package (read-only, at its host path) and the two
    standalone worker files: no original, no checks, no harness state, no network, a
    read-only filesystem and an empty environment. A red-team port that loaded the
    original from the path in its environment showed no differences while returning 0
    for everything; here there is nothing to load.
    """

    def __init__(self, site: str | Path, module: str):
        self.site = Path(site).resolve()
        self.module = module

    def docker(self, name, sockets, memory, heartbeat_mount, heartbeat, timeout) -> list[str]:
        command = [
            "timeout", "-k", str(KILL_GRACE_SECONDS), str(int(timeout)),
            "python3", "-I", "/portwright-compare/worker_host.py",
            "--socket", PORT_SOCKET, "--module", self.module, "--path", str(self.site),
        ]
        env = []
        for key, value in os.environ.items():
            if key.startswith(PASSED_ENV_PREFIXES):
                env += ["-e", f"{key}={value}"]
        return [
            "docker", "run", "-d", "--rm", "--name", name, "--label", "portwright.gate=1",
            "--network", "none", "--read-only", "--memory", memory, "--memory-swap", memory,
            "--pids-limit", "512", "--cap-drop", "ALL",
            "--user", f"{os.getuid()}:{os.getgid()}", "-w", "/",
            *env,
            *_mount(self.site, "ro"),
            "-v", f"{COMPARE_DIR / 'worker.py'}:/portwright-compare/worker.py:ro",
            "-v", f"{COMPARE_DIR / 'worker_host.py'}:/portwright-compare/worker_host.py:ro",
            "-v", f"{sockets}:{PORT_SOCKET_DIR}:rw",
            "-v", f"{resources.WATCHDOG_SCRIPT}:/portwright-compare/sandbox_watchdog.py:ro",
            *heartbeat_mount,
            IMAGE, *resources.watched(command, heartbeat, "/portwright-compare/sandbox_watchdog.py"),
        ]


def enabled() -> bool:
    return not in_sandbox() and os.environ.get("PORTWRIGHT_GATES_ON_HOST") != "1"


def python(host_python: str) -> str:
    """The interpreter a side runs with, where gate processes run."""
    return CONTAINER_PYTHON if enabled() else host_python


def scratch_root() -> Path:
    """Where gates keep files the container must see.

    Some Docker installs (snap) can't see the host's temporary directory and mount an
    empty folder there without any error, so contained runs keep scratch in the repo.
    """
    if not enabled():
        return Path(tempfile.gettempdir())
    root = Path(os.environ.get("PORTWRIGHT_SCRATCH", REPO_ROOT / "target" / "gate-scratch")).resolve()
    root.mkdir(parents=True, exist_ok=True)
    return root


def scratch(prefix: str) -> tempfile.TemporaryDirectory:
    return tempfile.TemporaryDirectory(prefix=prefix, dir=scratch_root())


def _mount(path, mode: str) -> list[str]:
    path = Path(path).resolve()
    if path.is_relative_to(tempfile.gettempdir()):
        raise ValueError(
            f"{path} is in the host's temporary directory, which some Docker installs can't see; "
            "use portwright.isolation.scratch() for files a contained gate needs"
        )
    return ["-v", f"{path}:{path}:{mode}"]


def run(
    command: list[str],
    *,
    cwd,
    read=(),
    write=(),
    env: dict[str, str] | None = None,
    timeout: float = 1800.0,
    port: PortHost | None = None,
) -> subprocess.CompletedProcess:
    """Run ``command`` like ``subprocess.run(..., capture_output=True, text=True)``.

    Contained, only ``read`` (read-only), ``write`` and ``cwd`` (read-write) and the
    harness itself are visible, at their host paths. ``env`` is the whole environment
    the command gets, apart from a few worker settings.

    With ``port``, a second container runs the port's worker host (see ``PortHost``),
    and ``command`` reaches it through the socket at ``PORT_SOCKET``.
    """
    env = dict(env or {})
    resources.require_free_space(cwd, "a gate run")
    if not enabled():
        return subprocess.run(
            command, cwd=cwd, env={**os.environ, **env}, capture_output=True, text=True,
            timeout=timeout, check=False,
        )
    name = f"portwright-gate-{uuid.uuid4().hex[:12]}"
    memory = resources.gate_memory_limit()
    total_mb = resources.megabytes(memory)
    if port is not None:
        # One reservation covers both containers: two held separately could each wait
        # on the other in a full pool.
        port_mb = min(PORT_MEMORY_MB, total_mb // 2)
        memory = f"{total_mb - port_mb}m"
    docker = [
        "docker", "run", "--rm", "--name", name, "--label", "portwright.gate=1",
        "--network", "none", "--memory", memory, "--memory-swap", memory,
        "--user", f"{os.getuid()}:{os.getgid()}",
        "-e", "HOME=/tmp", "-e", "PORTER_IN_SANDBOX=1",
        "-w", str(Path(cwd).resolve()),
    ]
    for key, value in os.environ.items():
        if key.startswith(PASSED_ENV_PREFIXES) and key not in env:
            env[key] = value
    for key, value in env.items():
        docker += ["-e", f"{key}={value}"]
    mounts = {}
    for path in read:
        mounts[Path(path).resolve()] = "ro"
    for path in (*write, cwd):
        mounts[Path(path).resolve()] = "rw"
    mounts.setdefault(REPO_ROOT, "ro")
    for path, mode in mounts.items():
        docker += _mount(path, mode)
    sockets = None
    if port is not None:
        sockets = Path(scratch_root()) / f"sock-{uuid.uuid4().hex[:8]}"
        sockets.mkdir()
        docker += ["-v", f"{sockets}:{PORT_SOCKET_DIR}:rw"]
    heartbeat_mount, heartbeat = resources.watchdog_mounts()
    docker += heartbeat_mount
    # The timeout runs inside the container, so a killed orchestrator can't leave one
    # behind for long; the watchdog ends it sooner, once the heartbeat stops.
    inner = resources.watched(["timeout", "-k", str(KILL_GRACE_SECONDS), str(int(timeout)), *command], heartbeat)
    port_name = f"{name}-port"
    try:
        # The whole machine's gate containers share one pool (open item V).
        with resources.gate_budget().reserve(total_mb, f"gate {name}"):
            if port is not None:
                started = subprocess.run(
                    port.docker(port_name, sockets, f"{port_mb}m", heartbeat_mount, heartbeat, timeout),
                    capture_output=True, text=True, check=False,
                )
                if started.returncode != 0:
                    return subprocess.CompletedProcess(command, 1, "", f"the port's container didn't start: {started.stderr[-1000:]}")
            done = resources.run_bounded(
                [*docker, IMAGE, *inner], timeout=timeout + 2 * KILL_GRACE_SECONDS, container=name, disk=cwd,
            )
    except subprocess.TimeoutExpired:
        subprocess.run(["docker", "kill", name], capture_output=True, check=False)
        raise
    finally:
        if port is not None:
            subprocess.run(["docker", "kill", port_name], capture_output=True, check=False)
            shutil.rmtree(sockets, ignore_errors=True)
    if done.returncode == TIMEOUT_EXIT:  # callers expect the same error as on the host
        raise subprocess.TimeoutExpired(command, timeout, done.stdout, done.stderr)
    return done
