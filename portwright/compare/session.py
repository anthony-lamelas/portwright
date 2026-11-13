"""Calls the reference and the candidate with the same input and compares the results."""

from __future__ import annotations

import json
import os
import select
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

from portwright.compare import worker as wire

WORKER_PATH = Path(wire.__file__).resolve()
DEFAULT_TIMEOUT = 10.0
STARTUP_TIMEOUT = 60.0
SUMMARY_LIMIT = 500


class WorkerStartError(RuntimeError):
    """A library couldn't be imported, so nothing can be compared."""


class Divergence(AssertionError):
    """The two versions behaved differently."""


@dataclass(frozen=True)
class Side:
    """One version of the library and the interpreter that runs it."""

    path: str
    module: str
    python: str = sys.executable
    coverage_file: str | None = None


class Worker:
    """A library in its own process, so a crash or hang can't take down the run."""

    def __init__(self, side: Side, timeout: float = DEFAULT_TIMEOUT):
        self.side = side
        self.timeout = timeout
        self.process: subprocess.Popen | None = None
        self.generation = 0
        self._buffer = b""
        self._closed = False

    def start(self) -> None:
        command = [
            self.side.python, "-I", str(WORKER_PATH),
            "--module", self.side.module, "--path", self.side.path,
        ]
        if self.side.coverage_file:
            command += ["--coverage", self.side.coverage_file]
        self.process = subprocess.Popen(
            command,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            cwd=self.side.path,
        )
        self.generation += 1
        self._buffer = b""
        self._closed = False
        ready = self._read_line(STARTUP_TIMEOUT)
        if ready is None or not json.loads(ready).get("ready"):
            detail = json.loads(ready).get("error") if ready else "no response"
            self.close()
            raise WorkerStartError(f"can't import {self.side.module} from {self.side.path}: {detail}")

    def _read_line(self, timeout: float) -> bytes | None:
        """Return one line, or None on a timeout or when the process has exited."""
        stream = self.process.stdout
        fd = stream.fileno()
        remaining = timeout
        while b"\n" not in self._buffer:
            ready, _, _ = select.select([fd], [], [], remaining)
            if not ready:
                return None
            chunk = os.read(fd, 65536)
            if not chunk:
                self._closed = True
                return None
            self._buffer += chunk
        line, self._buffer = self._buffer.split(b"\n", 1)
        return line

    def request(self, message: dict) -> dict:
        if self.process is None or self.process.poll() is not None:
            self.start()
        try:
            self.process.stdin.write(json.dumps(message).encode() + b"\n")
            self.process.stdin.flush()
        except (BrokenPipeError, OSError):
            return self._dead()
        line = self._read_line(self.timeout)
        if line is not None:
            return json.loads(line)
        # A closed pipe means the process died, even if the OS hasn't marked it as
        # exited yet. Checking the process state here would misreport crashes as hangs.
        if self._closed:
            return self._dead()
        self._kill()
        return {"kind": "timeout"}

    def _dead(self) -> dict:
        code = self.process.wait()
        self.process = None
        return {"kind": "crashed", "returncode": code}

    def _kill(self) -> None:
        self.process.kill()
        self.process.wait()
        self.process = None

    def close(self) -> None:
        if self.process is None:
            return
        if self.process.poll() is None:
            try:
                self.process.stdin.write(b'{"op":"exit"}\n')
                self.process.stdin.close()
                self.process.wait(timeout=30)
            except (BrokenPipeError, OSError, subprocess.TimeoutExpired):
                self.process.kill()
                self.process.wait()
        self.process = None


def strip_handles(value):
    """Remove handle numbers, which only say how many objects were made before this one."""
    if isinstance(value, dict):
        return {k: strip_handles(v) for k, v in value.items() if k != "handle"}
    if isinstance(value, list):
        return [strip_handles(item) for item in value]
    return value


def find_difference(reference, candidate, path="outcome"):
    """Return where two outcomes first differ, or None if they match exactly."""
    if type(reference) is not type(candidate):
        return path, reference, candidate
    if isinstance(reference, dict):
        for key in dict.fromkeys([*reference, *candidate]):
            if key not in reference or key not in candidate:
                return f"{path}.{key}", reference.get(key, "<missing>"), candidate.get(key, "<missing>")
            found = find_difference(reference[key], candidate[key], f"{path}.{key}")
            if found:
                return found
        return None
    if isinstance(reference, list):
        if len(reference) != len(candidate):
            return f"{path}.length", len(reference), len(candidate)
        for index, (left, right) in enumerate(zip(reference, candidate)):
            found = find_difference(left, right, f"{path}[{index}]")
            if found:
                return found
        return None
    if reference != candidate:
        return path, reference, candidate
    return None


def _short(value) -> str:
    text = json.dumps(value, ensure_ascii=True)
    return text if len(text) <= SUMMARY_LIMIT else text[:SUMMARY_LIMIT] + "..."


class Pair:
    """The same object on both sides, so tests can call methods on it together."""

    def __init__(self, session, api, reference, candidate):
        self._session = session
        self._api = api
        self._reference = reference
        self._candidate = candidate
        self.porter_handle = None  # set per side while encoding; see Session._encode

    def __repr__(self) -> str:
        # Shown in difference examples. The default "<Pair object at 0x...>" told the
        # agent nothing about what was passed.
        return f"<{self._api} object returned by an earlier call>"

    def call(self, name, /, *args, **kwargs):
        return self._session._call(f"{self._api}.{name}", {"op": "call", "path": name}, (self._reference, self._candidate), args, kwargs)

    def get(self, name):
        return self._session._call(f"{self._api}.{name}", {"op": "get", "path": name}, (self._reference, self._candidate), (), {})


class Session:
    """Compares two versions call by call, and counts what each API really did."""

    def __init__(self, reference: Side, candidate: Side, timeout: float = DEFAULT_TIMEOUT, approved=()):
        self.reference = Worker(reference, timeout)
        self.candidate = Worker(candidate, timeout)
        self.stats: dict[str, dict] = {}
        self.divergences: dict[str, dict] = {}
        # Differences a person approved are still recorded, but don't stop the check.
        # Raising on them ended each test at its first approved difference, so every
        # call after it went unchecked.
        self.approved = frozenset(approved)

    def start(self) -> None:
        self.reference.start()
        self.candidate.start()

    def close(self) -> None:
        self.reference.close()
        self.candidate.close()

    def same(self, target: str, /, *args, **kwargs):
        """Call ``target`` on both sides and raise Divergence if they differ."""
        return self._call(target, {"op": "call", "path": target}, (None, None), args, kwargs)

    def get(self, target: str):
        """Read an attribute on both sides and raise Divergence if they differ."""
        return self._call(target, {"op": "get", "path": target}, (None, None), (), {})

    def _encode(self, values, side_index):
        """Encode inputs for one side, swapping any Pair for that side's handle."""
        def convert(value):
            if isinstance(value, Pair):
                handle = value._reference if side_index == 0 else value._candidate
                return {"t": "handle", "v": handle}
            if type(value) in (list, tuple, set, frozenset):
                return {"t": type(value).__name__, "v": [convert(item) for item in value]}
            if type(value) is dict:
                return {"t": "dict", "v": [[convert(k), convert(v)] for k, v in value.items()]}
            return wire.encode(value)
        return [convert(item) for item in values]

    def _build(self, base, handles, index, args, kwargs):
        message = dict(base)
        if handles[index] is not None:
            message["handle"] = handles[index]
        message["args"] = self._encode(args, index)
        message["kwargs"] = dict(zip(kwargs, self._encode(kwargs.values(), index)))
        return message

    def _call(self, api, base, handles, args, kwargs):
        reference_generation = self.reference.generation
        candidate_generation = self.candidate.generation
        left = self.reference.request(self._build(base, handles, 0, args, kwargs))
        right = self.candidate.request(self._build(base, handles, 1, args, kwargs))
        self._count(api, left)
        if left.get("kind") == "crashed" or (left.get("kind") == "timeout" and right.get("kind") != "crashed"):
            # The original gave no answer, so there is nothing to match. Requiring the
            # port to time out too would reject it for being faster (round(115,
            # -99999999) takes CPython minutes), and an original that died, say at the
            # worker's memory cap, has no outcome to copy. A port crash after an original
            # timeout still counts.
            self.stats[api]["not_compared"] = self.stats[api].get("not_compared", 0) + 1  # reported in gate summaries
            difference = None
        else:
            difference = find_difference(strip_handles(left), strip_handles(right))
        if difference:
            self._record_divergence(api, args, kwargs, left, right, difference)
        if left.get("kind") == "raised":
            return None
        if self.reference.generation != reference_generation or self.candidate.generation != candidate_generation:
            return None
        value = left.get("value")
        if isinstance(value, dict) and value.get("t") in ("object", "callable"):
            name = value.get("class") or api
            return Pair(self, name, value["handle"], right["value"]["handle"])
        return value

    def _count(self, api, outcome) -> None:
        counts = self.stats.setdefault(api, {"calls": 0, "returned": 0, "raised": {}, "timeout": 0, "crashed": 0})
        counts["calls"] += 1
        kind = outcome.get("kind")
        if kind == "returned":
            counts["returned"] += 1
        elif kind == "raised":
            counts["raised"][outcome["type"]] = counts["raised"].get(outcome["type"], 0) + 1
        elif kind in ("timeout", "crashed"):
            counts[kind] += 1

    def _record_divergence(self, api, args, kwargs, left, right, difference) -> None:
        where, reference_value, candidate_value = difference
        shown_args = ", ".join([*map(ascii, args), *(f"{k}={v!a}" for k, v in kwargs.items())])
        signature = f"{api} {where}"
        entry = self.divergences.setdefault(signature, {"count": 0})
        entry["count"] += 1
        message = (
            f"{api}({shown_args}) differs at {where}\n"
            f"  reference: {_short(reference_value)}\n"
            f"  candidate: {_short(candidate_value)}"
        )
        entry["example"] = message  # Hypothesis tries the simplest failing input last
        try:
            entry["input"] = {
                "target": api,
                "args": [wire.encode(item) for item in args],
                "kwargs": {key: wire.encode(item) for key, item in kwargs.items()},
            }
        except TypeError:
            entry.pop("input", None)  # calls on stored objects can't be replayed alone
        if signature in self.approved:
            return
        raise Divergence(message)


_current: Session | None = None


def set_current(session: Session | None) -> None:
    global _current
    _current = session


def current() -> Session:
    if _current is None:
        raise RuntimeError("no comparison session; run checks through the differential gate")
    return _current


def same(target: str, /, *args, **kwargs):
    """Used by check files: call ``target`` on both versions and require a match."""
    return current().same(target, *args, **kwargs)


def get(target: str):
    """Used by check files: read an attribute on both versions and require a match."""
    return current().get(target)
