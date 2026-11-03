"""An agent that runs away can only hurt its own container, and can't outlive its run."""

import os
import subprocess
import time
from pathlib import Path

import pytest

from portwright.agents import AgentSession, DockerClaude, memory_limit
from portwright.auth import SUBSCRIPTION, Auth
from portwright.toolchain import image_available

REPO_TARGET = Path(__file__).resolve().parents[1] / "target" / "containment-test"
docker = pytest.mark.skipif(not image_available(), reason="needs the toolchain image")


def _megabytes(limit: str) -> int:
    return int(limit.rstrip("m"))


def test_memory_for_all_agents_fits_in_part_of_the_machine():
    total_mb = int(next(line.split()[1] for line in open("/proc/meminfo") if line.startswith("MemTotal:"))) // 1024
    for agents in (1, 2, 4):
        backend = DockerClaude(REPO_TARGET / "p.sock", Auth(SUBSCRIPTION), max_agents=agents)
        per_agent = _megabytes(backend.memory)
        assert per_agent * agents <= max(0.6 * total_mb, 2048 * agents)
    assert _megabytes(memory_limit(share=0.3)) <= 16 * 1024


def _container(backend: DockerClaude, name: str, script: str) -> list[str]:
    REPO_TARGET.mkdir(parents=True, exist_ok=True)
    (REPO_TARGET / "home").mkdir(exist_ok=True)
    command = backend.command(AgentSession("porter", "", {}, REPO_TARGET / "session"), REPO_TARGET / "home", {}, name)
    return [*command[:-1], script]


@docker
def test_a_runaway_allocation_is_killed_inside_its_container():
    backend = DockerClaude(REPO_TARGET / "p.sock", Auth(SUBSCRIPTION), run_label=str(REPO_TARGET))
    backend.memory = "256m"
    done = subprocess.run(
        _container(backend, f"porter-oom-{os.getpid()}", "python3 -c 'b = bytearray(1024 * 1024 * 1024); print(len(b))'; echo exit=$?"),
        capture_output=True, text=True, timeout=120, check=False,
    )
    assert "exit=137" in done.stdout, done.stdout + done.stderr  # killed by the container's own limit


@docker
def test_containers_left_by_a_dead_run_are_stopped(tmp_path):
    label = str(tmp_path)
    backend = DockerClaude(REPO_TARGET / "p.sock", Auth(SUBSCRIPTION), run_label=label)
    name = f"porter-orphan-{os.getpid()}"
    orphan = subprocess.Popen(_container(backend, name, "sleep 300"), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        for _ in range(60):
            if name in subprocess.run(["docker", "ps", "--format", "{{.Names}}"], capture_output=True, text=True).stdout:
                break
            time.sleep(0.5)
        token = tmp_path / "sessions" / "001-porter" / "home" / ".claude" / ".credentials.json"
        token.parent.mkdir(parents=True)
        token.write_text("{}")
        assert backend.stop_leftovers(tmp_path / "sessions") == 1
        assert not token.exists()
        for _ in range(60):
            if name not in subprocess.run(["docker", "ps", "--format", "{{.Names}}"], capture_output=True, text=True).stdout:
                break
            time.sleep(0.5)
        else:
            pytest.fail("the orphaned container is still running")
    finally:
        orphan.kill()
