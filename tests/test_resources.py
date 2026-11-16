"""Memory, disk and liveness limits a run shares with the rest of the machine (open items D, E, H, T)."""

import json
import os
import subprocess
import sys
import time
from pathlib import Path

import pytest

from portwright import isolation, resources, toolchain
from portwright.agents import AgentSession, DockerClaude
from portwright.auth import SUBSCRIPTION, Auth
from portwright.limits import LimitReached
from portwright.sandbox_watchdog import stale
from portwright.toolchain import image_available

REPO = Path(__file__).resolve().parents[1]
REPO_TARGET = REPO / "target" / "sandbox-tests" / "resources"
docker = pytest.mark.skipif(not image_available(), reason="needs the toolchain image")


# ---------------------------------------------------------------- D: heartbeat and watchdog


def test_watchdog_reads_a_stopped_heartbeat_as_a_dead_orchestrator(tmp_path):
    beat = tmp_path / "beat"
    assert stale(str(beat), 60, seen=False) == (False, False)  # no orchestrator to watch
    beat.touch()
    assert stale(str(beat), 60, seen=False) == (False, True)
    assert stale(str(beat), 60, seen=True, now=time.time() + 61) == (True, True)
    beat.unlink()
    assert stale(str(beat), 60, seen=True) == (True, True)  # deleted once seen: the run ended


def test_heartbeat_keeps_its_file_fresh_and_deletes_it_on_stop(tmp_path, monkeypatch):
    monkeypatch.delenv(resources.HEARTBEAT_ENV, raising=False)
    heartbeat = resources.Heartbeat(tmp_path / "hb" / "beat", interval=0.05)
    heartbeat.start()
    try:
        assert os.environ[resources.HEARTBEAT_ENV] == str(heartbeat.path)
        first = heartbeat.path.stat().st_mtime_ns
        time.sleep(0.3)
        assert heartbeat.path.stat().st_mtime_ns > first
    finally:
        heartbeat.stop()
    assert not heartbeat.path.exists()
    assert resources.HEARTBEAT_ENV not in os.environ


def test_containers_watch_the_heartbeat_only_when_there_is_one(tmp_path, monkeypatch):
    monkeypatch.delenv(resources.HEARTBEAT_ENV, raising=False)
    session = AgentSession("porter", "", {}, tmp_path / "session")
    backend = DockerClaude(tmp_path / "p.sock", Auth(SUBSCRIPTION))
    assert "sandbox_watchdog" not in backend.command(session, tmp_path / "home", {}, "n")[-1]
    assert resources.watched(["true"], None) == ["true"]
    monkeypatch.setenv(resources.HEARTBEAT_ENV, str(tmp_path / "hb" / "beat"))
    command = backend.command(session, tmp_path / "home", {}, "n")
    assert f"{tmp_path / 'hb'}:/heartbeat:ro" in command
    assert "sandbox_watchdog.py /heartbeat/beat" in command[-1]
    assert command[-1].index("sandbox_watchdog") < command[-1].index("exec timeout")


@docker
def test_a_container_ends_itself_once_the_heartbeat_stops(monkeypatch):
    """SIGKILL on the orchestrator used to leave agents running for up to two hours."""
    REPO_TARGET.mkdir(parents=True, exist_ok=True)
    monkeypatch.setenv("PORTWRIGHT_HEARTBEAT_MAX_AGE", "3")
    heartbeat = resources.Heartbeat(REPO_TARGET / "hb" / "beat", interval=0.5)
    heartbeat.start()
    try:
        started = time.time()
        process = subprocess.Popen(
            ["docker", "run", "--rm", "--network", "none", "--user", f"{os.getuid()}:{os.getgid()}",
             *resources.watchdog_mounts()[0], "-v", f"{REPO}:{REPO}:ro", toolchain.IMAGE,
             *resources.watched(["sleep", "300"], resources.watchdog_mounts()[1])],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        )
        time.sleep(8)
        assert process.poll() is None, process.communicate()  # alive while the heartbeat beats
        heartbeat._stop.set()  # the orchestrator "dies": no more beats, file left behind
        heartbeat._thread.join()
        _, stderr = process.communicate(timeout=60)
        assert time.time() - started < 60
        assert "the orchestrator is gone" in stderr
    finally:
        heartbeat.stop()
        if process.poll() is None:
            process.kill()


# ---------------------------------------------------------------- E: gate memory


def test_gate_memory_is_part_of_the_machine_unless_set(monkeypatch):
    monkeypatch.delenv("PORTWRIGHT_GATE_MEMORY", raising=False)
    limit = resources.megabytes(resources.gate_memory_limit())
    assert resources.GATE_MEMORY_FLOOR_MB <= limit <= resources.GATE_MEMORY_CEILING_MB
    monkeypatch.setenv("PORTWRIGHT_GATE_MEMORY", "3g")
    assert resources.gate_memory_limit() == "3g"
    assert resources.megabytes("3g") == 3072 and resources.megabytes("512m") == 512
    assert resources.megabytes(str(2 * 1024**3)) == 2048


def _captured(monkeypatch, module):
    seen = []

    def fake_run(command, **kwargs):
        seen.append(command)
        return subprocess.CompletedProcess(command, 0, "", "")
    monkeypatch.setattr(module.subprocess, "run", fake_run)
    return seen


def test_rust_builds_run_under_the_gate_memory_limit(monkeypatch):
    """Builds run agent code and had no memory limit at all."""
    monkeypatch.setenv("PORTWRIGHT_GATE_MEMORY", "5g")
    monkeypatch.delenv("PORTER_IN_SANDBOX", raising=False)
    REPO_TARGET.mkdir(parents=True, exist_ok=True)
    seen = _captured(monkeypatch, toolchain)
    toolchain.run_in_toolchain(["cargo", "build"], REPO_TARGET, fetch=False)
    command = seen[-1]
    assert command[command.index("--memory") + 1] == "5g"
    assert command[command.index("--memory-swap") + 1] == "5g"


def test_contained_gate_runs_have_memory_and_swap_limits(monkeypatch):
    monkeypatch.setenv("PORTWRIGHT_GATE_MEMORY", "4g")
    monkeypatch.setattr(isolation, "enabled", lambda: True)
    REPO_TARGET.mkdir(parents=True, exist_ok=True)
    seen = _captured(monkeypatch, isolation)
    isolation.run(["python3", "-c", "pass"], cwd=REPO_TARGET)
    command = seen[-1]
    assert command[command.index("--memory") + 1] == "4g"
    assert command[command.index("--memory-swap") + 1] == "4g"


# ---------------------------------------------------------------- H: machine-wide agent budget


def test_agent_memory_is_shared_by_every_run_on_the_machine(tmp_path):
    """Two runs at once each gave agents 60% of RAM."""
    first = resources.AgentBudget(tmp_path, total_mb=1000)
    other_run = resources.AgentBudget(tmp_path, total_mb=1000)
    a = first.try_reserve(600, "run a")
    assert a is not None
    assert other_run.try_reserve(600, "run b") is None  # would pass the machine's budget
    b = other_run.try_reserve(400, "run b")
    assert b is not None and other_run.in_use_mb() == 1000
    first.release(a)
    assert other_run.try_reserve(600, "run b") is not None


def test_a_request_bigger_than_the_budget_runs_alone(tmp_path):
    budget = resources.AgentBudget(tmp_path, total_mb=1000)
    key = budget.try_reserve(4000)
    assert key is not None
    assert budget.try_reserve(1) is None
    budget.release(key)


def test_a_dead_runs_reservations_are_dropped(tmp_path):
    """A run killed with SIGKILL can't release what it holds."""
    dead = subprocess.Popen([sys.executable, "-c", "pass"])
    dead.wait()
    (tmp_path / "agents.json").write_text(json.dumps([{"key": "x", "pid": dead.pid, "started": "1", "mb": 900}]))
    budget = resources.AgentBudget(tmp_path, total_mb=1000)
    assert budget.in_use_mb() == 0
    assert budget.try_reserve(900) is not None


def test_a_session_waits_for_memory_held_elsewhere(tmp_path):
    budget = resources.AgentBudget(tmp_path, total_mb=1000, poll_seconds=0.05)
    held = budget.try_reserve(800)
    waits = []
    import threading

    def release_later():
        time.sleep(0.3)
        budget.release(held)
    threading.Thread(target=release_later).start()
    with budget.reserve(500, on_wait=waits.append):
        assert budget.in_use_mb() == 500
    assert waits and "800 of 1000 MB" in waits[0]
    assert budget.in_use_mb() == 0


# ---------------------------------------------------------------- T: disk


def test_low_disk_space_pauses_instead_of_failing_midway(tmp_path, monkeypatch):
    monkeypatch.setenv("PORTWRIGHT_MIN_FREE_GB", "1000000")
    with pytest.raises(resources.LowDiskSpace) as caught:
        resources.require_free_space(tmp_path / "not" / "made" / "yet", "a build")
    assert isinstance(caught.value, LimitReached)  # the run pauses and can resume
    assert "PORTWRIGHT_MIN_FREE_GB" in str(caught.value) and "a build" in str(caught.value)
    monkeypatch.setenv("PORTWRIGHT_MIN_FREE_GB", "0")
    resources.require_free_space(tmp_path, "a build")


def test_builds_and_gate_runs_check_free_space_first(tmp_path, monkeypatch):
    monkeypatch.setenv("PORTWRIGHT_MIN_FREE_GB", "1000000")
    REPO_TARGET.mkdir(parents=True, exist_ok=True)
    started = _captured(monkeypatch, toolchain)
    with pytest.raises(resources.LowDiskSpace):
        toolchain.run_in_toolchain(["cargo", "build"], REPO_TARGET, fetch=False)
    with pytest.raises(resources.LowDiskSpace):
        isolation.run(["python3", "-c", "pass"], cwd=tmp_path)
    assert started == []
