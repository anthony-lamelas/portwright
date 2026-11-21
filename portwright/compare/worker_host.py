"""Starts a port's comparison workers in a container of their own.

The comparison runs in one container with the original, the checks and the run's
files. A port's workers used to run there too, so a port could read the original's
path from its environment, load it, and answer with the original's results; the
red-team showed a port that returns 0 for everything passing that way. Now the
harness starts a second container holding only the port's built package and this
folder, with no network, a read-only filesystem and an environment with nothing in
it. This host listens on a Unix socket the two containers share, and for each
connection starts ``worker.py --guard`` with the connection as its stdin and stdout.
When the comparison hangs up, as it does to stop a worker that timed out, the worker
is killed.

Standalone and standard library only, like ``worker.py``.
"""

from __future__ import annotations

import argparse
import os
import select
import socket
import subprocess
import sys
import threading

WORKER = os.path.join(os.path.dirname(os.path.abspath(__file__)), "worker.py")


def environment() -> dict[str, str]:
    keep = {key: value for key, value in os.environ.items() if key.startswith("PORTWRIGHT_WORKER_")}
    return {**keep, "PATH": "/usr/local/bin:/usr/bin:/bin", "HOME": "/nonexistent", "LC_ALL": "C.UTF-8"}


def serve(connection: socket.socket, module: str, path: str) -> None:
    with connection:
        process = subprocess.Popen(
            [sys.executable, "-I", WORKER, "--module", module, "--path", path, "--guard"],
            stdin=connection.fileno(), stdout=connection.fileno(), stderr=subprocess.DEVNULL,
            cwd=path, env=environment(),
        )
        watch = select.poll()
        # POLLRDHUP reports the other end hanging up without taking the worker's input.
        watch.register(connection.fileno(), select.POLLRDHUP | select.POLLHUP | select.POLLERR)
        while process.poll() is None:
            if watch.poll(200):
                process.kill()
                break
        process.wait()


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--socket", required=True)
    parser.add_argument("--module", required=True)
    parser.add_argument("--path", required=True)
    options = parser.parse_args(argv)
    if os.path.exists(options.socket):
        os.unlink(options.socket)
    server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    server.bind(options.socket)
    server.listen(16)
    while True:
        connection, _ = server.accept()
        threading.Thread(target=serve, args=(connection, options.module, options.path), daemon=True).start()


if __name__ == "__main__":
    raise SystemExit(main())
