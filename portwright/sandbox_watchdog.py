"""Ends a container whose orchestrator has died.

Started in the background inside agent and gate containers as
``python3 sandbox_watchdog.py HEARTBEAT_FILE MAX_AGE_SECONDS``. The orchestrator
touches the heartbeat file every few seconds (``portwright.resources.Heartbeat``).
If the file stops changing, or disappears after it was seen, the orchestrator is
gone: nobody will merge the work or stop the session, so this kills every process
in the container it can signal, and the container ends.

An orchestrator killed with SIGKILL can't stop its containers, and an agent
container used to run until its own time limit, up to about two hours (open item D).

Standalone and standard library only, like ``sandbox_forward.py``: it runs with the
container's Python before anything else is set up. If the file never existed, there
is no orchestrator to watch (a manual run), so the watchdog does nothing.
"""

from __future__ import annotations

import os
import signal
import sys
import time

POLL_SECONDS = 5


def stale(path: str, max_age: float, seen: bool, now: float | None = None) -> tuple[bool, bool]:
    """Return (stale, seen): whether the heartbeat says the orchestrator is gone."""
    now = time.time() if now is None else now
    try:
        modified = os.stat(path).st_mtime
    except OSError:
        return seen, seen  # gone after it was seen means the run ended without cleaning up
    return now - modified > max_age, True


def end_container() -> None:
    # kill(-1) reaches every process this user may signal, except this one and PID 1.
    # PID 1 is the session's ``timeout`` or shell, which exits once its child dies.
    for signum in (signal.SIGTERM, signal.SIGKILL):
        try:
            os.kill(-1, signum)
        except OSError:
            pass
        time.sleep(5)
    os._exit(0)


def main(argv: list[str]) -> int:
    path, max_age = argv[0], float(argv[1])
    seen = False
    while True:
        is_stale, seen = stale(path, max_age, seen)
        if is_stale:
            print(f"portwright watchdog: {path} stopped changing; the orchestrator is gone", file=sys.stderr, flush=True)
            end_container()
        time.sleep(POLL_SECONDS)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
