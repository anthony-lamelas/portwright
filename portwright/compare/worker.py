"""Runs one version of a library in its own process and records what each call did.

Started with ``python -I worker.py``. ``-I`` keeps the library's directory off the
front of ``sys.path``, so a file like ``json.py`` in the library can't replace the
standard library module this worker needs. For that reason this file must stay
self-contained and use only the standard library. ``coverage`` is imported only
when asked for.

Protocol: one JSON request per line on stdin, one JSON response per line on the
original stdout. The library's own prints are captured and recorded instead.
"""

from __future__ import annotations

import argparse
import contextlib
import datetime as dt
import decimal
import importlib
import io
import json
import os
import re
import struct
import sys
import time

ITER_LIMIT = 1000
DEPTH_LIMIT = 40
# Only memory addresses, as in "<Foo object at 0x7f...>". Masking every 0x token
# made re.compile('0x1f') equal re.compile('0xDEAD').
_ADDRESS = re.compile(r"(?<=\bat )0x[0-9a-fA-F]+")


class PythonExpression:
    """A Python expression both sides evaluate, for inputs like callback functions."""

    def __init__(self, source: str):
        self.source = source

    def __repr__(self):
        return f"PythonExpression({self.source!r})"



def _int_text(value: int) -> str:
    """Decimal for readable reports, hex past ~3,900 digits.

    Python 3.12 refuses to turn ints over 4,300 digits into decimal text, so sending
    them as decimal crashed the worker, and both sides "crashing" passed as a match.
    Hex has no such limit. ``int(text, 0)`` reads both forms.
    """
    return str(value) if value.bit_length() < 13000 else hex(value)


def _float_text(value: float) -> str:
    """Exact text for a float: hex, or the raw bits for a NaN.

    ``float.hex()`` is exact for every other float, but gives "nan" for every NaN,
    so a NaN's sign and payload went uncompared.
    """
    if value == value:
        return value.hex()
    bits = struct.unpack("<Q", struct.pack("<d", value))[0]
    return f"{'-' if bits >> 63 else ''}nan:{bits:#018x}"


def _float_value(text: str) -> float:
    if "nan:" in text:
        return struct.unpack("<d", struct.pack("<Q", int(text.split(":")[1], 16)))[0]
    return float.fromhex(text)


def _class_name(cls) -> str:
    """Module and qualified name, so a homemade ``ValueError`` isn't the real one."""
    return f"{getattr(cls, '__module__', None)}.{cls.__qualname__}"


def encode(value):
    """Turn an input value into JSON the other side can rebuild exactly."""
    if isinstance(value, PythonExpression):
        return {"t": "python", "v": value.source}
    if value is None:
        return {"t": "none"}
    kind = type(value)
    if kind is bool:
        return {"t": "bool", "v": value}
    if kind is int:
        return {"t": "int", "v": _int_text(value)}
    if kind is float:
        return {"t": "float", "v": _float_text(value)}
    if kind is str:
        return {"t": "str", "v": value}
    if kind in (bytes, bytearray):
        return {"t": kind.__name__, "v": bytes(value).hex()}
    if kind in (list, tuple, set, frozenset):
        return {"t": kind.__name__, "v": [encode(item) for item in value]}
    if kind is dict:
        return {"t": "dict", "v": [[encode(k), encode(v)] for k, v in value.items()]}
    if kind is decimal.Decimal:
        return {"t": "decimal", "v": str(value)}
    handle = getattr(value, "porter_handle", None)
    if handle is not None:
        return {"t": "handle", "v": handle}
    raise TypeError(f"can't send {kind.__name__} to a worker")


def decode(data, handles, evaluate=True):
    """Rebuild an input value from ``encode``.

    With ``evaluate=False``, Python expressions come back as ``PythonExpression``
    so they can be sent on to the workers. Check files use that, because the
    library being tested can only be imported inside a worker.
    """
    tag, value = data["t"], data.get("v")
    if tag in ("list", "tuple", "set", "frozenset", "dict"):
        inner = lambda item: decode(item, handles, evaluate)  # noqa: E731
        if tag == "dict":
            return {inner(k): inner(v) for k, v in value}
        return {"list": list, "tuple": tuple, "set": set, "frozenset": frozenset}[tag](inner(item) for item in value)
    if tag == "python" and not evaluate:
        return PythonExpression(value)
    if tag == "none":
        return None
    if tag == "bool":
        return value
    if tag == "int":
        return int(value, 0)
    if tag == "float":
        return _float_value(value)
    if tag == "str":
        return value
    if tag == "bytes":
        return bytes.fromhex(value)
    if tag == "bytearray":
        return bytearray.fromhex(value)
    if tag in ("list", "tuple", "set", "frozenset"):
        return {"list": list, "tuple": tuple, "set": set, "frozenset": frozenset}[tag](
            decode(item, handles) for item in value
        )
    if tag == "dict":
        return {decode(k, handles): decode(v, handles) for k, v in value}
    if tag == "decimal":
        return decimal.Decimal(value)
    if tag == "handle":
        return handles.objects[value]
    if tag == "python":
        # Comes only from check files, which run outside the library being tested.
        return eval(value, {"__builtins__": __builtins__})  # noqa: S307
    raise ValueError(f"unknown input tag {tag}")


class Handles:
    """Objects kept alive between calls so tests can call their methods later."""

    def __init__(self):
        self.objects = {}
        self._ids = {}

    def add(self, obj) -> int:
        key = id(obj)
        if key not in self._ids:
            handle = len(self.objects) + 1
            self._ids[key] = handle
            self.objects[handle] = obj
        return self._ids[key]


def _describe_exception(exc, depth=0, seen=()):
    seen = (*seen, id(exc))
    return {
        "kind": "raised",
        "type": type(exc).__qualname__,
        "mro": [_class_name(cls) for cls in type(exc).__mro__],
        "args": canon(exc.args, Handles(), depth + 1, seen),
        "message": _safe(str, exc),
        "attributes": _exception_attributes(exc, depth, seen),
    }


_BASE_EXCEPTION_NAMES = frozenset(dir(BaseException))


def _exception_attributes(exc, depth, seen):
    """Public attributes beyond args, like ``re.error.pos`` or ``UnicodeError.start``.

    Read with ``dir`` rather than ``vars``: built-in exceptions and Rust classes keep
    them in slots or getters, not in ``__dict__``. Methods, private names and the
    traceback and context dunders are left out. A value that can't be read or
    described is recorded as such instead of crashing the worker.
    """
    names = _safe(dir, exc)
    found = {}
    for name in sorted(names) if isinstance(names, list) else ():  # not a list: dir raised
        if name.startswith("_") or name in _BASE_EXCEPTION_NAMES:
            continue
        try:
            value = getattr(exc, name)
            if callable(value) and not isinstance(value, (type, BaseException)):
                continue
            found[name] = canon(value, Handles(), depth + 1, seen)
        except BaseException as error:
            found[name] = {"t": "unreadable", "v": type(error).__qualname__}
    return found


def _safe(func, value):
    try:
        return func(value)
    except BaseException as exc:  # a broken __str__ or __repr__ is behavior too
        return f"<{func.__name__} raised {type(exc).__qualname__}>"


def _is_iterator(value) -> bool:
    return hasattr(type(value), "__next__") and hasattr(type(value), "__iter__")


def canon(value, handles, depth=0, seen=()):
    """Describe a result so that two versions can be compared exactly.

    Types are part of the description, so ``1`` and ``1.0`` differ. Floats use
    their exact hex form, or their bits for a NaN. Dicts keep their order and sets
    don't have one.
    """
    if depth > DEPTH_LIMIT:
        return {"t": "too_deep"}
    if id(value) in seen:
        return {"t": "cycle"}
    kind = type(value)
    if value is None:
        return {"t": "none"}
    if kind is bool:
        return {"t": "bool", "v": value}
    if kind is int:
        return {"t": "int", "v": _int_text(value)}
    if kind is float:
        return {"t": "float", "v": _float_text(value)}
    if kind is str:
        return {"t": "str", "v": value}
    if kind in (bytes, bytearray):
        return {"t": kind.__name__, "v": bytes(value).hex()}
    seen = (*seen, id(value))
    next_level = lambda item: canon(item, handles, depth + 1, seen)  # noqa: E731
    if kind in (list, tuple):
        return {"t": kind.__name__, "v": [next_level(item) for item in value]}
    if kind in (set, frozenset):
        items = [next_level(item) for item in value]
        return {"t": kind.__name__, "v": sorted(items, key=lambda i: json.dumps(i, sort_keys=True))}
    if kind is dict:
        return {"t": "dict", "v": [[next_level(k), next_level(v)] for k, v in value.items()]}
    if kind is decimal.Decimal:
        return {"t": "decimal", "v": str(value)}
    if kind in (dt.datetime, dt.time):
        return {"t": kind.__name__, "v": value.isoformat(), "tz": next_level(value.tzinfo)}
    if kind is dt.date:
        return {"t": "date", "v": value.isoformat()}
    if kind is dt.timedelta:
        return {"t": "timedelta", "v": [value.days, value.seconds, value.microseconds]}
    if kind is type:
        return {"t": "class", "v": _class_name(value)}
    if isinstance(value, BaseException):
        return {"t": "exception", "v": _describe_exception(value, depth, seen)}
    if _is_iterator(value):
        return _drain(value, next_level)
    if callable(value) and hasattr(value, "__qualname__"):
        return {"t": "callable", "v": value.__qualname__, "handle": handles.add(value)}
    return {
        "t": "object",
        "class": kind.__qualname__,
        "module": getattr(kind, "__module__", None),
        "repr": _ADDRESS.sub("0x_", _safe(repr, value)),
        "handle": handles.add(value),
    }


def _drain(iterator, next_level):
    """Read an iterator, recording where it fails, so lazy errors can be compared."""
    items = []
    result = {"t": "iterator", "v": items}
    try:
        for item in iterator:
            items.append(next_level(item))
            if len(items) >= ITER_LIMIT:
                result["truncated"] = True
                break
    except BaseException as exc:
        result["raised_after"] = len(items)
        result["error"] = _describe_exception(exc)
    return result


def _resolve(request, module, handles):
    if "handle" in request:
        target = handles.objects[request["handle"]]
        path = request["path"].split(".") if request.get("path") else []
    else:
        target = module
        path = request["path"].split(".")
    for name in path:
        target = getattr(target, name)
    return target


def run_request(request, module, handles):
    """Run one call, recording the result or exception, and any changed arguments."""
    args = [decode(item, handles) for item in request.get("args", [])]
    kwargs = {key: decode(item, handles) for key, item in request.get("kwargs", {}).items()}
    captured = io.StringIO()
    outcome = {}
    with contextlib.redirect_stdout(captured):
        try:
            target = _resolve(request, module, handles)
            value = target if request["op"] == "get" else target(*args, **kwargs)
            outcome = {"kind": "returned", "value": canon(value, handles)}
        except BaseException as exc:
            outcome = _describe_exception(exc)
    outcome["args_after"] = [canon(item, handles) for item in args]
    outcome["kwargs_after"] = {key: canon(item, handles) for key, item in kwargs.items()}
    outcome["stdout"] = captured.getvalue()
    return outcome


def run_benchmark(request, module, handles):
    """Time one call repeatedly and return the median nanoseconds per call."""
    args = [decode(item, handles) for item in request.get("args", [])]
    kwargs = {key: decode(item, handles) for key, item in request.get("kwargs", {}).items()}
    target = _resolve(request, module, handles)
    clock = time.perf_counter_ns
    loops = 1
    try:
        while True:
            start = clock()
            for _ in range(loops):
                target(*args, **kwargs)
            elapsed = clock() - start
            if elapsed >= 20_000_000 or loops >= 1 << 24:
                break
            loops *= 2
        samples = []
        for _ in range(request.get("samples", 15)):
            start = clock()
            for _ in range(loops):
                target(*args, **kwargs)
            samples.append((clock() - start) / loops)
    except BaseException as exc:
        return _describe_exception(exc)
    samples.sort()
    return {"kind": "benchmark", "median_ns": samples[len(samples) // 2], "fastest_ns": samples[0], "loops": loops}


def _limit_memory():
    """Cap this worker's address space, so an input that asks for a huge allocation,
    such as format(1, "2000000000"), raises MemoryError here instead of starving the
    machine. Both versions run under the same cap, so the outcome is still compared."""
    import resource

    megabytes = int(os.environ.get("PORTWRIGHT_WORKER_MEMORY_MB", "1024"))
    if megabytes > 0:
        limit = megabytes * 1024 * 1024
        resource.setrlimit(resource.RLIMIT_AS, (limit, limit))


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--module", required=True)
    parser.add_argument("--path", required=True)
    parser.add_argument("--coverage")
    options = parser.parse_args()

    # Keep the real stdout for protocol messages. Anything writing to file
    # descriptor 1 afterward, such as native code, can't corrupt the protocol.
    protocol = os.fdopen(os.dup(1), "w", encoding="utf-8", buffering=1)
    os.dup2(2, 1)
    sys.path.insert(0, options.path)
    _limit_memory()

    collector = None
    if options.coverage:
        import coverage

        collector = coverage.Coverage(
            data_file=options.coverage, data_suffix=True, branch=True, source=[options.path]
        )
        collector.start()

    def reply(message):
        protocol.write(json.dumps(message) + "\n")
        protocol.flush()

    try:
        module = importlib.import_module(options.module)
    except BaseException as exc:
        reply({"ready": False, "error": _describe_exception(exc)})
        return 1
    reply({"ready": True, "location": getattr(module, "__file__", None)})

    handles = Handles()
    for line in sys.stdin:
        request = json.loads(line)
        if request["op"] == "exit":
            break
        if request["op"] == "bench":
            reply(run_benchmark(request, module, handles))
        else:
            reply(run_request(request, module, handles))
    if collector:
        collector.stop()
        collector.save()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
