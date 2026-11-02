"""The only way out of an agent sandbox: a proxy that allows a short list of hosts.

Agent containers run with no network at all. Their single connection to the outside
is a Unix socket mounted into the container, so nothing here listens on a network
port. Inside, ``sandbox_forward.py`` makes that socket look like a normal HTTP proxy.
"""

from __future__ import annotations

import os
import select
import socket
import socketserver
import threading
from pathlib import Path

# Claude's API and login, and the crate registry. Nothing else.
ALLOWED_HOSTS = frozenset(
    {
        "api.anthropic.com",
        "claude.ai",
        "platform.claude.com",
        "console.anthropic.com",
        "index.crates.io",
        "static.crates.io",
        "crates.io",
    }
)
ALLOWED_PORT = 443
CONNECT_TIMEOUT = 20
HEADER_LIMIT = 16384


def is_allowed(target: str, allowed_hosts=ALLOWED_HOSTS) -> bool:
    """Only an exact allowed host on the HTTPS port passes."""
    host, separator, port = target.rpartition(":")
    return bool(separator) and port == str(ALLOWED_PORT) and host.lower() in allowed_hosts


class _Handler(socketserver.BaseRequestHandler):
    def handle(self) -> None:
        client = self.request
        header = b""
        while b"\r\n\r\n" not in header and len(header) < HEADER_LIMIT:
            chunk = client.recv(4096)
            if not chunk:
                return
            header += chunk
        parts = header.split(b"\r\n", 1)[0].decode("latin-1").split()
        if len(parts) < 2 or parts[0] != "CONNECT" or not is_allowed(parts[1], self.server.allowed_hosts):
            self.server.denied.append(parts[1] if len(parts) > 1 else "<bad request>")
            client.sendall(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n")
            return
        host, _, port = parts[1].rpartition(":")
        try:
            upstream = socket.create_connection((host, int(port)), timeout=CONNECT_TIMEOUT)
        except OSError:
            client.sendall(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n")
            return
        client.sendall(b"HTTP/1.1 200 Connection established\r\n\r\n")
        _tunnel(client, upstream)


def _tunnel(left: socket.socket, right: socket.socket) -> None:
    sockets = [left, right]
    try:
        while True:
            readable, _, _ = select.select(sockets, [], [], 600)
            if not readable:
                return
            for source in readable:
                data = source.recv(65536)
                if not data:
                    return
                (right if source is left else left).sendall(data)
    except OSError:
        return
    finally:
        right.close()


class _Server(socketserver.ThreadingMixIn, socketserver.UnixStreamServer):
    daemon_threads = True


class AllowlistProxy:
    """Runs on the host for as long as agents need it."""

    def __init__(self, socket_path: str | Path, allowed_hosts=ALLOWED_HOSTS):
        self.socket_path = Path(socket_path)
        self.allowed_hosts = allowed_hosts
        self._server: _Server | None = None

    @property
    def denied(self) -> list[str]:
        return self._server.denied if self._server else []

    def start(self) -> None:
        self.socket_path.parent.mkdir(parents=True, exist_ok=True)
        if self.socket_path.exists():
            self.socket_path.unlink()
        self._server = _Server(str(self.socket_path), _Handler)
        self._server.allowed_hosts = self.allowed_hosts
        self._server.denied = []
        os.chmod(self.socket_path, 0o600)
        threading.Thread(target=self._server.serve_forever, daemon=True).start()

    def stop(self) -> None:
        if self._server:
            self._server.shutdown()
            self._server.server_close()
            self._server = None
        if self.socket_path.exists():
            self.socket_path.unlink()
