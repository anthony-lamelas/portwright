"""Starts agent sessions. Harness code decides what each one can see and change."""

from __future__ import annotations

import json
import os
import re
import shutil
import signal
import subprocess
import uuid
from dataclasses import dataclass, field
from pathlib import Path
from typing import Protocol

from portwright.auth import Auth
from portwright.toolchain import IMAGE

REPO_ROOT = Path(__file__).resolve().parents[1]
PROMPTS = REPO_ROOT / "prompts"
SKILLS = REPO_ROOT / "skills"
_LIMIT_TEXT = re.compile(r"usage limit|limit reached|hit your .*limit|rate.?limit|too many requests|overloaded|out of extra usage", re.I)
_AUTH_TEXT = re.compile(r"not logged in|invalid api key|authentication_error|oauth token .*expired|please run /login", re.I)


class LimitReached(RuntimeError):
    """A usage limit was hit. Progress is saved, and the run can resume later."""


@dataclass
class AgentSession:
    """One agent run. ``mounts`` maps a host path to ``container_path[:ro]``."""

    role: str
    prompt: str
    mounts: dict[str, str]
    scratch: Path
    max_turns: int = 150
    timeout: float = 3600.0
    model: str | None = None


@dataclass
class AgentResult:
    status: str  # ok, error, limit, timeout
    text: str = ""
    usage: dict = field(default_factory=dict)


class Backend(Protocol):
    def run(self, session: AgentSession) -> AgentResult: ...


def role_prompt(role: str) -> Path:
    return PROMPTS / f"{role}.md"


def parse_result(stdout: str, returncode: int) -> AgentResult:
    """Interpret ``claude -p --output-format json`` output."""
    data = None
    for line in reversed(stdout.strip().splitlines()):
        try:
            candidate = json.loads(line)
        except ValueError:
            continue
        if isinstance(candidate, dict) and candidate.get("type") == "result":
            data = candidate
            break
    if data is None:
        status = "limit" if _LIMIT_TEXT.search(stdout) else "error"
        return AgentResult(status, stdout[-2000:])
    text = str(data.get("result", ""))
    usage = {
        "input_tokens": data.get("usage", {}).get("input_tokens", 0),
        "cache_creation_input_tokens": data.get("usage", {}).get("cache_creation_input_tokens", 0),
        "cache_read_input_tokens": data.get("usage", {}).get("cache_read_input_tokens", 0),
        "output_tokens": data.get("usage", {}).get("output_tokens", 0),
        "estimated_api_cost_usd": data.get("total_cost_usd", 0.0),
        "turns": data.get("num_turns", 0),
    }
    if data.get("is_error") or returncode != 0:
        if data.get("api_error_status") == 429 or _LIMIT_TEXT.search(text):
            return AgentResult("limit", text, usage)
        if data.get("subtype") == "error_max_turns":
            return AgentResult("error", "the session ran out of turns", usage)
        return AgentResult("error", text or str(data.get("subtype")), usage)
    return AgentResult("ok", text, usage)


def memory_limit(share: float = 0.3, ceiling_gb: int = 16) -> str:
    """Memory for one agent container: a share of this machine's RAM, so two agents
    plus the host's own gate runs can't exhaust it. A fixed 16g did nothing on a 15 GB machine."""
    try:
        with open("/proc/meminfo") as meminfo:
            total_kb = next(int(line.split()[1]) for line in meminfo if line.startswith("MemTotal:"))
    except (OSError, StopIteration, ValueError):
        return f"{ceiling_gb}g"
    return f"{max(2048, min(ceiling_gb * 1024, int(total_kb / 1024 * share)))}m"


def stop_container(name: str) -> None:
    """Stop a container. Snap Docker can deny ``docker kill``, so fall back to its process."""
    if subprocess.run(["docker", "kill", name], capture_output=True, check=False).returncode == 0:
        return
    pid = subprocess.run(
        ["docker", "inspect", "-f", "{{.State.Pid}}", name], capture_output=True, text=True, check=False
    ).stdout.strip()
    if pid.isdigit() and int(pid) > 0:
        try:
            os.kill(int(pid), signal.SIGKILL)
        except OSError:
            pass


class DockerClaude:
    """Runs Claude Code in a container with no network except the allowlist proxy."""

    def __init__(self, proxy_socket: str | Path, auth: Auth, *, image: str = IMAGE, cargo_cache: str | Path | None = None):
        self.proxy_socket = Path(proxy_socket).resolve()
        self.auth = auth
        self.image = image
        self.cargo_cache = Path(cargo_cache).resolve() if cargo_cache else None

    def command(self, session: AgentSession, home: Path, env: dict[str, str], name: str) -> list[str]:
        docker = [
            "docker", "run", "--rm", "-i", "--name", name,
            "--network", "none",
            "--user", f"{os.getuid()}:{os.getgid()}",
            # no-new-privileges would be better, but snap Docker refuses to start with it.
            # The container still runs as a non-root user with no capabilities.
            "--cap-drop", "ALL",
            "--memory", memory_limit(), "--memory-swap", memory_limit(), "--pids-limit", "4096",
            "-v", f"{home.resolve()}:/home/agent",
            "-v", f"{SKILLS.resolve()}:/home/agent/.claude/skills:ro",
            "-v", f"{self.proxy_socket.parent}:/proxy",
            "-v", f"{REPO_ROOT}/portwright:/harness/portwright:ro",
            "-e", "HOME=/home/agent",
            "-e", "HTTPS_PROXY=http://127.0.0.1:3128",
            "-e", "PORTER_IN_SANDBOX=1",
            "-e", f"PORTER_PROXY_SOCKET=/proxy/{self.proxy_socket.name}",
            "-e", "PYTHONPATH=/harness",
            "-e", "CARGO_TARGET_DIR=/work/target",
            "-e", "DISABLE_AUTOUPDATER=1",
            "-e", "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1",
            "-w", "/work",
        ]
        if self.cargo_cache:
            self.cargo_cache.mkdir(parents=True, exist_ok=True)
            docker += ["-v", f"{self.cargo_cache}:/opt/cargo/registry"]
        for key, value in env.items():
            docker += ["-e", f"{key}={value}"]
        for host_path, container_path in session.mounts.items():
            docker += ["-v", f"{Path(host_path).resolve()}:{container_path}"]
        claude = [
            "claude", "-p", "--output-format", "json",
            "--permission-mode", "bypassPermissions",
            # Keeps the account's connectors, such as email, out of agent sessions.
            "--strict-mcp-config",
            "--max-turns", str(session.max_turns),
            "--append-system-prompt-file", "/home/agent/role.md",
        ]
        if session.model:
            claude += ["--model", session.model]
        script = "python3 /harness/portwright/sandbox_forward.py & exec " + " ".join(claude)
        return [*docker, self.image, "bash", "-c", script]

    def run(self, session: AgentSession) -> AgentResult:
        home = (session.scratch / "home").resolve()
        # Docker creates missing mount folders as root, so make them first.
        (home / ".claude" / "skills").mkdir(parents=True, exist_ok=True)
        for host_path in session.mounts:
            Path(host_path).mkdir(parents=True, exist_ok=True)
        env = self.auth.prepare_session(home)
        shared = (PROMPTS / "_shared.md").read_text(encoding="utf-8")
        (home / "role.md").write_text(shared + "\n" + role_prompt(session.role).read_text(encoding="utf-8"), encoding="utf-8")
        name = f"porter-{session.role}-{uuid.uuid4().hex[:8]}"
        try:
            done = subprocess.run(
                self.command(session, home, env, name), input=session.prompt,
                capture_output=True, text=True, timeout=session.timeout, check=False,
            )
        except subprocess.TimeoutExpired:
            stop_container(name)
            return AgentResult("timeout", f"session passed {session.timeout:.0f}s")
        finally:
            credentials = home / ".claude" / ".credentials.json"
            if credentials.exists():
                credentials.unlink()
        (session.scratch / "stdout.json").write_text(done.stdout)
        (session.scratch / "stderr.txt").write_text(done.stderr)
        result = parse_result(done.stdout, done.returncode)
        if result.status == "error" and _AUTH_TEXT.search(result.text + done.stderr):
            return AgentResult("limit", "login needs renewing: " + result.text, result.usage)
        return result
