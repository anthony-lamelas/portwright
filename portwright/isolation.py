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
import subprocess
import tempfile
import uuid
from pathlib import Path

from portwright.toolchain import IMAGE, in_sandbox

REPO_ROOT = Path(__file__).resolve().parents[1]
CONTAINER_PYTHON = "python3"
# Settings that reach the processes inside. Everything else in the host environment stays out.
PASSED_ENV_PREFIXES = ("PORTWRIGHT_WORKER_",)
MEMORY_LIMIT = os.environ.get("PORTWRIGHT_GATE_MEMORY", "6g")
KILL_GRACE_SECONDS = 10
TIMEOUT_EXIT = 124  # coreutils timeout


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
) -> subprocess.CompletedProcess:
    """Run ``command`` like ``subprocess.run(..., capture_output=True, text=True)``.

    Contained, only ``read`` (read-only), ``write`` and ``cwd`` (read-write) and the
    harness itself are visible, at their host paths. ``env`` is the whole environment
    the command gets, apart from a few worker settings.
    """
    env = dict(env or {})
    if not enabled():
        return subprocess.run(
            command, cwd=cwd, env={**os.environ, **env}, capture_output=True, text=True,
            timeout=timeout, check=False,
        )
    name = f"portwright-gate-{uuid.uuid4().hex[:12]}"
    docker = [
        "docker", "run", "--rm", "--name", name, "--label", "portwright.gate=1",
        "--network", "none", "--memory", MEMORY_LIMIT,
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
    # The timeout runs inside the container, so a killed orchestrator can't leave one behind.
    inner = ["timeout", "-k", str(KILL_GRACE_SECONDS), str(int(timeout)), *command]
    try:
        done = subprocess.run(
            [*docker, IMAGE, *inner], capture_output=True, text=True,
            timeout=timeout + 2 * KILL_GRACE_SECONDS, check=False,
        )
    except subprocess.TimeoutExpired:
        subprocess.run(["docker", "kill", name], capture_output=True, check=False)
        raise
    if done.returncode == TIMEOUT_EXIT:  # callers expect the same error as on the host
        raise subprocess.TimeoutExpired(command, timeout, done.stdout, done.stderr)
    return done
