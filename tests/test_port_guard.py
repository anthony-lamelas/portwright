"""A port's worker can't borrow the original's answers or run Python code (red-team, #88).

The red-team's port read ``PORTER_CONFIG``, loaded the original from the path it names,
and answered every call with the original's results: no differences, while its own
code returned 0 for everything. Others ran the original's source with ``exec`` or
patched the worker. Each toy port here is a package with a native-module marker, so
the harness treats it as a built port, and code in its ``__init__.py`` stands in for
what Rust bindings could do. Patching the worker itself only lets a port report
answers it computed, so it gains nothing without one of these.
"""

import json
import shutil
import sys
import textwrap
from pathlib import Path

import pytest

from portwright import isolation
from portwright.gates.differential import run_checks
from portwright.toolchain import image_available

REPO = Path(__file__).resolve().parents[1]
ROOT = REPO / "target" / "sandbox-tests" / "port-guard"

ORIGINAL = "def f(x):\n    return x * 2 + 1\n"
CHECKS = '''
from hypothesis import given, strategies as st
from portwright.compare import PythonExpression, same

@given(st.integers(-50, 50))
def test_f(x):
    same("f", x)
'''

HONEST_BUT_WRONG = "def f(x):\n    return 0\n"

# Each attack makes f return the original's answer, if the attack works.
ATTACKS = {
    "config": '''
        import importlib.util, json, os
        config = json.load(open(os.environ["PORTER_CONFIG"]))
        where = config["reference"]["path"] + "/toy/__init__.py"
        spec = importlib.util.spec_from_file_location("stolen", where)
        stolen = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(stolen)
        f = stolen.f
    ''',
    "embedded": '''
        namespace = {}
        exec("def f(x):\\n    return x * 2 + 1\\n", namespace)
        f = namespace["f"]
    ''',
    "compile": '''
        f = eval(compile("lambda x: x * 2 + 1", "/usr/local/lib/python3.12/os.py", "eval"))
    ''',
    "ctypes": '''
        import ctypes
        def f(x):
            return 0
    ''',
}


def _setup(tmp: Path, name: str, port_code: str) -> tuple[dict, dict, Path]:
    shutil.rmtree(tmp / name, ignore_errors=True)
    reference = tmp / name / "ref"
    (reference / "toy").mkdir(parents=True)
    (reference / "toy" / "__init__.py").write_text(ORIGINAL)
    port = tmp / name / "port"
    (port / "toy").mkdir(parents=True)
    (port / "toy" / "__init__.py").write_text(textwrap.dedent(port_code))
    (port / "toy" / "_native.cpython-312-x86_64-linux-gnu.so").write_bytes(b"")  # marks a built port
    checks = tmp / name / "checks"
    checks.mkdir()
    (checks / "test_toy.py").write_text(CHECKS)
    side = lambda path: {"path": str(path), "module": "toy", "python": sys.executable}  # noqa: E731
    return side(reference), side(port), checks


@pytest.fixture
def where():
    ROOT.mkdir(parents=True, exist_ok=True)
    return ROOT


def _run(where, name, code):
    reference, port, checks = _setup(where, name, code)
    return run_checks(checks, reference, port, max_examples=20, seed=1)


def test_a_wrong_port_is_still_caught(where):
    run = _run(where, "control", HONEST_BUT_WRONG)
    assert run.divergences, run.output[-2000:]


@pytest.mark.parametrize("attack", sorted(ATTACKS))
def test_a_port_cant_borrow_the_originals_answers(where, attack):
    run = _run(where, attack, ATTACKS[attack])
    # Refused where it happens: as a difference, or, at import, a port that can't start.
    # Either way the gate fails; it never sees the original's answers.
    assert run.divergences or run.exit_status not in (0, 1), f"{attack} hid every difference:\n{run.output[-2000:]}"
    text = json.dumps(run.divergences) + run.output
    assert "refused" in text or "a port's worker" in text or "PORTER_CONFIG" in text, text[-2000:]


def test_the_ports_worker_gets_no_run_settings(where, monkeypatch):
    monkeypatch.setenv("PORTER_SECRET_FOR_TEST", "x")
    run = _run(where, "env", '''
        import os
        def f(x):
            # Matches only if the worker sees nothing of the run.
            leaked = [k for k in os.environ if k.startswith(("PORTER_", "PYTHONPATH"))]
            return x * 2 + 1 if not leaked else -1
    ''')
    assert not run.divergences, json.dumps(run.divergences)[:2000]


def test_a_port_may_still_use_the_standard_library_and_check_callbacks(where):
    """namedtuple and dataclasses compile source; lazy stdlib imports unmarshal it."""
    run = _run(where, "stdlib", '''
        def f(x):
            import collections, dataclasses, fractions, decimal
            Point = collections.namedtuple("Point", "a b")
            @dataclasses.dataclass
            class Box:
                v: int
            return Point(Box(x).v * 2, 1).a + fractions.Fraction(1) .numerator
    ''')
    assert not run.divergences, json.dumps(run.divergences)[:2000]


@pytest.mark.skipif(not image_available() or not isolation.enabled(), reason="needs the toolchain image")
def test_the_ports_container_holds_neither_the_original_nor_the_checks(where):
    reference, port, checks = _setup(where, "container", '''
        import os
        def f(x):
            visible = [p for p in (%r, %r, %r) if os.path.exists(p)]
            return x * 2 + 1 if not visible else -len(visible)
    ''' % (str(where / "container" / "ref"), str(where / "container" / "checks"), str(REPO / "portwright" / "orchestrator.py")))
    run = run_checks(checks, reference, port, max_examples=10, seed=1)
    assert not run.divergences, json.dumps(run.divergences)[:2000] + run.output[-2000:]


def test_check_expressions_may_run_their_own_code(where):
    """isodate's checks probe with exec inside an expression; that's the checks' code."""
    reference, port, checks = _setup(where, "check-exec", ORIGINAL)
    (checks / "test_toy.py").write_text(CHECKS + '''

def test_probe():
    same("f", PythonExpression("(lambda ns: (exec('r = 3', ns), ns['r'])[1])({})"))
'''.replace("from portwright.compare import same", "from portwright.compare import PythonExpression, same"))
    run = run_checks(checks, reference, port, max_examples=5, seed=1)
    assert not run.divergences and run.exit_status == 0, json.dumps(run.divergences)[:1500] + run.output[-1500:]
