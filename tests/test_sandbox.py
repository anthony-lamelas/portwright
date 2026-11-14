"""The sandbox must block everything except its allowlist, and handle login safely."""

import json
import os
import socket
import subprocess
import time
from pathlib import Path

import pytest

from portwright.agents import AgentSession, DockerClaude, parse_result
from portwright.auth import API_KEY, SUBSCRIPTION, Auth, AuthError
from portwright.sandbox import AllowlistProxy, is_allowed
from portwright.toolchain import IMAGE, image_available

REPO_TARGET = Path(__file__).parent.parent / "target" / "sandbox-tests"
docker = pytest.mark.skipif(not image_available(), reason="toolchain image not built")


@pytest.mark.parametrize(
    ("target", "allowed"),
    [
        ("api.anthropic.com:443", True),
        ("index.crates.io:443", True),
        ("static.crates.io:443", True),
        ("github.com:443", False),
        ("pypi.org:443", False),
        ("api.anthropic.com:80", False),
        ("evil.api.anthropic.com:443", False),
        ("api.anthropic.com.evil.com:443", False),
        ("127.0.0.1:443", False),
        ("api.anthropic.com", False),
    ],
)
def test_allowlist_is_exact(target, allowed):
    assert is_allowed(target) is allowed


def ask_proxy(path, line):
    with socket.socket(socket.AF_UNIX) as client:
        client.connect(str(path))
        client.sendall(line.encode() + b"\r\n\r\n")
        return client.recv(200).decode()


def test_proxy_denies_and_records_other_hosts(tmp_path):
    proxy = AllowlistProxy(tmp_path / "proxy.sock")
    proxy.start()
    try:
        assert "403" in ask_proxy(tmp_path / "proxy.sock", "CONNECT github.com:443 HTTP/1.1")
        assert "403" in ask_proxy(tmp_path / "proxy.sock", "GET http://example.com/ HTTP/1.1")
        assert "github.com:443" in proxy.denied
    finally:
        proxy.stop()
    assert not (tmp_path / "proxy.sock").exists()


def test_subscription_mode_refuses_a_stray_api_key(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "not-a-real-key")
    with pytest.raises(AuthError, match="bill"):
        Auth(SUBSCRIPTION).check_environment()


def test_api_key_mode_needs_a_key(monkeypatch):
    monkeypatch.delenv("ANTHROPIC_API_KEY", raising=False)
    with pytest.raises(AuthError):
        Auth(API_KEY).check_environment()


def test_session_never_receives_the_refresh_token(tmp_path, monkeypatch):
    monkeypatch.delenv("ANTHROPIC_API_KEY", raising=False)
    source = tmp_path / "source.json"
    source.write_text(json.dumps({"claudeAiOauth": {
        "accessToken": "access-example", "refreshToken": "refresh-example",
        "expiresAt": int((time.time() + 86400) * 1000), "scopes": ["a"],
    }}))
    env = Auth(SUBSCRIPTION, source).prepare_session(tmp_path / "home")
    written = tmp_path / "home/.claude/.credentials.json"
    assert env == {}
    assert "access-example" in written.read_text()
    assert "refresh" not in written.read_text().lower()
    assert oct(written.stat().st_mode)[-3:] == "600"


@pytest.mark.parametrize(
    ("result", "status"),
    [
        ({"type": "result", "is_error": False, "result": "done"}, "ok"),
        ({"type": "result", "is_error": True, "result": "Claude usage limit reached"}, "limit"),
        ({"type": "result", "is_error": True, "api_error_status": 429, "result": "x"}, "limit"),
        ({"type": "result", "is_error": True, "subtype": "error_max_turns", "result": ""}, "error"),
        ({"type": "result", "is_error": True, "result": "something else"}, "error"),
    ],
)
def test_results_are_classified(result, status):
    assert parse_result(json.dumps(result), 0).status == status


def test_garbage_output_is_an_error_not_success():
    assert parse_result("not json", 1).status == "error"


@docker
def test_container_has_no_network_except_the_proxy():
    """Runs real commands in the real sandbox setup."""
    REPO_TARGET.mkdir(parents=True, exist_ok=True)
    socket_path = REPO_TARGET / "proxy" / "p.sock"
    proxy = AllowlistProxy(socket_path)
    proxy.start()
    try:
        backend = DockerClaude(socket_path, Auth(SUBSCRIPTION))
        session = AgentSession("porter", "", {}, REPO_TARGET / "session")
        command = backend.command(session, (REPO_TARGET / "home"), {}, f"porter-test-{os.getpid()}")
        (REPO_TARGET / "home").mkdir(exist_ok=True)
        probe = (
            "python3 /harness/portwright/sandbox_forward.py & sleep 1; "
            "curl -sS -m 5 -o /dev/null -w 'direct:%{http_code}\\n' --noproxy '*' https://1.1.1.1 2>&1 | tail -1; "
            "curl -sS -m 8 -o /dev/null -w 'blocked:%{http_code}\\n' https://github.com 2>&1 | tail -1; "
            "curl -sS -m 15 -o /dev/null -w 'crates:%{http_code}\\n' https://index.crates.io/config.json 2>&1 | tail -1; "
            "touch /harness/portwright/x 2>&1 | tail -1; "
            "ls -a /home/agent/.claude | grep -c credentials"
        )
        done = subprocess.run([*command[:-1], probe], capture_output=True, text=True, timeout=90, check=False)
        denied = list(proxy.denied)
    finally:
        proxy.stop()
    output = done.stdout + done.stderr
    assert "direct:000" in output or "Could not resolve" in output or "Network is unreachable" in output, output
    assert "crates:200" in output, output
    assert "blocked:200" not in output, output
    assert "Read-only" in output, output
    assert "github.com:443" in denied, (output, denied)


@pytest.mark.parametrize("text", [
    "Failed to authenticate: OAuth token revoked. Please log in again or contact your administrator.",
    "OAuth token has expired",
    "Not logged in. Please run /login",
])
def test_login_failures_are_recognized(text):
    """A revoked token cost a design attempt instead of pausing for a fresh login."""
    from portwright.agents import _AUTH_TEXT

    assert _AUTH_TEXT.search(text)


def test_prompts_with_lone_surrogates_can_be_sent():
    """A difference example quoting '\udc80' crashed the orchestrator."""
    from portwright.agents import encodable

    text = encodable("reference: 'x\udc80y' differs")
    text.encode("utf-8")
    assert "\\udc80" in text
