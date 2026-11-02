"""The comparison layer must spot real differences and ignore harmless ones."""

import textwrap

import pytest

from portwright.compare import PythonExpression
from portwright.compare.session import Divergence, Session, Side, WorkerStartError


def make_library(directory, body):
    package = directory / "lib"
    package.mkdir(parents=True)
    (package / "__init__.py").write_text(textwrap.dedent(body))
    return Side(path=str(directory), module="lib")


@pytest.fixture
def compare(tmp_path):
    sessions = []

    def build(reference_body, candidate_body, timeout=2.0):
        session = Session(
            make_library(tmp_path / f"ref{len(sessions)}", reference_body),
            make_library(tmp_path / f"cand{len(sessions)}", candidate_body),
            timeout=timeout,
        )
        session.start()
        sessions.append(session)
        return session

    yield build
    for session in sessions:
        session.close()


@pytest.mark.parametrize(
    ("left", "right"),
    [
        ("1", "1.0"),                                # int and float are equal in Python
        ("True", "1"),
        ("0.0", "-0.0"),
        ("'a'", "b'a'"),
        ("[1]", "(1,)"),
        ("{'a': 1, 'b': 2}", "{'b': 2, 'a': 1}"),    # order matters in dicts
        ("0.1 + 0.2", "0.3"),
        ("None", "0"),
    ],
)
def test_values_that_look_equal_are_different(compare, left, right):
    session = compare(f"def f(): return {left}", f"def f(): return {right}")
    with pytest.raises(Divergence):
        session.same("f")


def test_set_order_is_ignored_and_nan_matches_itself(compare):
    body = "def f(): return ({'b', 'a', 'c'}, float('nan'))"
    compare(body, body).same("f")


def test_different_object_addresses_are_ignored(compare):
    body = "class C: pass\ndef f(): return C()"
    compare(body, "x = [object() for _ in range(99)]\n" + body).same("f")


@pytest.mark.parametrize(
    ("left", "right", "where"),
    [
        ("raise ValueError('a')", "raise TypeError('a')", "type"),
        ("raise ValueError('a')", "raise ValueError('b')", "args"),
        ("raise ValueError('a', 1)", "raise ValueError('a')", "args"),
        ("raise ValueError('a')", "return 1", "kind"),
    ],
)
def test_error_differences(compare, left, right, where):
    session = compare(f"def f():\n    {left}", f"def f():\n    {right}")
    with pytest.raises(Divergence, match=where):
        session.same("f")


def test_exception_base_classes_are_compared(compare):
    session = compare(
        "class E(ValueError): pass\ndef f(): raise E('a')",
        "class E(KeyError): pass\ndef f(): raise E('a')",
    )
    with pytest.raises(Divergence, match="mro"):
        session.same("f")


def test_error_timing_in_iterators_is_compared(compare):
    session = compare(
        "def f():\n    yield 1\n    raise ValueError('late')",
        "def f():\n    raise ValueError('late')\n    yield 1",
    )
    with pytest.raises(Divergence):
        session.same("f")


def test_eager_error_differs_from_lazy_error(compare):
    session = compare(
        "def f():\n    raise ValueError('x')\n    yield",
        "def f():\n    raise ValueError('x')",
    )
    with pytest.raises(Divergence):
        session.same("f")


def test_changed_arguments_are_compared(compare):
    session = compare("def f(x): x.append(1)", "def f(x): pass")
    with pytest.raises(Divergence, match="args_after"):
        session.same("f", [])


def test_printed_output_is_compared(compare):
    session = compare("def f(): print('a')", "def f(): pass")
    with pytest.raises(Divergence, match="stdout"):
        session.same("f")


def test_crash_is_a_difference_and_the_worker_recovers(compare):
    session = compare(
        "def f(): return 1\ndef g(): return 2",
        "import os\ndef f(): os._exit(7)\ndef g(): return 2",
    )
    with pytest.raises(Divergence, match="crashed"):
        session.same("f")
    session.same("g")


def test_hang_is_a_difference_and_the_worker_recovers(compare):
    session = compare(
        "def f(): return 1\ndef g(): return 2",
        "def f():\n    while True: pass\ndef g(): return 2",
        timeout=0.5,
    )
    with pytest.raises(Divergence, match="timeout"):
        session.same("f")
    session.same("g")


def test_lone_surrogates_and_huge_ints_survive_the_trip(compare):
    session = compare("def f(x): return x", "def f(x): return x")
    session.same("f", "\ud800")
    session.same("f", 10**400)


def test_library_stdout_cannot_corrupt_the_protocol(compare):
    body = "import os\ndef f():\n    os.write(1, b'noise\\n')\n    return 1"
    compare(body, body).same("f")


def test_library_named_like_stdlib_cannot_break_the_worker(tmp_path):
    (tmp_path / "json.py").write_text("raise RuntimeError('planted')")
    library = make_library(tmp_path, "def f(): return 1")
    session = Session(library, library)
    session.start()
    session.same("f")
    session.close()


def test_unimportable_library_is_reported(tmp_path):
    library = make_library(tmp_path, "raise RuntimeError('broken')")
    session = Session(library, library)
    with pytest.raises(WorkerStartError):
        session.start()


def test_callbacks_and_returned_functions_work_on_both_sides(compare):
    body = "def make(n):\n    def inner(f):\n        return f(n)\n    return inner"
    session = compare(body, body.replace("return f(n)", "return f(n + 1)"))
    returned = session.same("make", 1)
    with pytest.raises(Divergence, match="outcome.value"):
        returned.call("__call__", PythonExpression("lambda value: value * 2"))


def test_methods_on_objects_are_compared_on_both_sides(compare):
    reference = "class C:\n    def m(self): return 1"
    candidate = "class C:\n    def m(self): return 2"
    session = compare(reference, candidate)
    pair = session.same("C")
    with pytest.raises(Divergence):
        pair.call("m")


def test_text_strategy_produces_lone_surrogates():
    from hypothesis import given, settings

    from portwright.compare.strategies import text

    found = []

    @settings(max_examples=200, database=None)
    @given(text())
    def collect(value):
        found.extend(c for c in value if 0xD800 <= ord(c) < 0xE000)

    collect()
    assert found, "st.text() hides lone surrogates, so this strategy must produce them"
