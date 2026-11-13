"""Every detail of an outcome is compared, not just the parts that are easy to see.

A review found four gaps: exception classes matched by name alone, exception
attributes such as ``re.error.pos`` never compared, every NaN looking alike, and any
``0x`` token in a repr masked as if it were an address.
"""

import struct
import sys

import pytest

from portwright.compare import worker
from portwright.evaluation import Project, differential_for

REFERENCE = '''
import re

class Pat:
    def __init__(self, text):
        self.text = text
    def __repr__(self):
        return f"<Pat {self.text!r}>"

class Positioned(Exception):
    def __init__(self, message, pos):
        super().__init__(message)
        self.pos = pos

class Unreadable(Exception):
    @property
    def broken(self):
        raise RuntimeError("no")

def homemade_error():
    raise ValueError("bad")

def error_position():
    re.compile("(")

def stored_vs_getter():
    raise Positioned("bad", 3)

def unreadable_attribute():
    error = Unreadable("bad")
    error.me = error
    raise error

def attribute_lookup():
    return (1).nope

def nan_sign():
    return float("nan")

def nan_payload():
    return struct_nan(0x7FF8000000000000)

def struct_nan(bits):
    import struct
    return struct.unpack("<d", struct.pack("<Q", bits))[0]

def hex_in_repr():
    return Pat("0x1f")

def real_address():
    return object()
'''

CANDIDATE = REFERENCE.replace(
    'def homemade_error():\n', 'class ValueError(Exception):\n    pass\n\ndef homemade_error():\n'
).replace(
    '    re.compile("(")\n',
    '    try:\n        re.compile("(")\n    except re.error as error:\n        error.pos = 1\n        raise\n',
).replace(
    'class Positioned(Exception):\n    def __init__(self, message, pos):\n        super().__init__(message)\n'
    '        self.pos = pos\n',
    # The same attribute, read through a getter, as a Rust class would provide it.
    'class Positioned(Exception):\n    def __init__(self, message, pos):\n        super().__init__(message)\n'
    '        self._pos = pos\n    @property\n    def pos(self):\n        return self._pos\n',
).replace(
    '    return float("nan")\n', '    return -float("nan")\n'
).replace(
    '    return (1).nope\n', '    raise AttributeError("\'int\' object has no attribute \'nope\'")\n'
).replace(
    'return struct_nan(0x7FF8000000000000)', 'return struct_nan(0x7FF8000000000001)'
).replace(
    'Pat("0x1f")', 'Pat("0xDEAD")'
)

CHECKS = '''
from portwright.compare import same

def test_outcomes():
    for name in ["homemade_error", "error_position", "stored_vs_getter", "unreadable_attribute",
                 "attribute_lookup", "nan_sign", "nan_payload", "hex_in_repr", "real_address"]:
        same(name)
'''


@pytest.fixture(scope="module")
def divergences(tmp_path_factory):
    root = tmp_path_factory.mktemp("outcomes")
    sides = {}
    for name, text in (("ref", REFERENCE), ("port", CANDIDATE)):
        (root / name).mkdir()
        (root / name / "toy.py").write_text(text)
        sides[name] = {"path": str(root / name), "module": "toy", "python": sys.executable}
    (root / "checks").mkdir()
    (root / "checks" / "test_toy.py").write_text(CHECKS)
    # The expected differences are approved, so every call runs and is recorded,
    # and any other difference blocks.
    approved = [f"{api} {where}" for api, where in EXPECTED.items()]
    project = Project(
        module="toy", lib="toy", core_crate="c", py_crate="p", reference=sides["ref"],
        checks=str(root / "checks"), tests_visible="", approved_differences=approved,
    )
    result = differential_for(project, sides["port"]["path"], max_examples=1)
    assert not result.details["divergences"], result.details["divergences"]
    assert result.details["calls"] == 9, result.summary
    return set(result.details["approved_seen"])


EXPECTED = {
    "homemade_error": "outcome.mro[0]",
    "error_position": "outcome.attributes.pos.v",
    "nan_sign": "outcome.value.v",
    "nan_payload": "outcome.value.v",
    "hex_in_repr": "outcome.value.repr",
}


@pytest.mark.parametrize("api", sorted(EXPECTED))
def test_a_hidden_difference_is_found(divergences, api):
    assert f"{api} {EXPECTED[api]}" in divergences


def test_equal_outcomes_still_match(divergences):
    """A getter matches a stored attribute, unreadable and cyclic attributes don't
    crash the worker, an explicit AttributeError matches a failed lookup, and real
    addresses are still masked."""
    assert {signature.split()[0] for signature in divergences} == set(EXPECTED)


def test_floats_round_trip_bit_for_bit():
    for bits in (0x7FF8000000000000, 0xFFF8000000000000, 0x7FF0000000000001, 0x8000000000000000):
        value = struct.unpack("<d", struct.pack("<Q", bits))[0]
        back = worker.decode(worker.encode(value), worker.Handles())
        assert struct.pack("<d", back) == struct.pack("<d", value)
    assert worker.encode(float("-nan"))["v"] == "-nan:0xfff8000000000000"
