"""Shared Hypothesis strategies for the pc_format checks."""

import math
import struct

from hypothesis import strategies as st
from portwright.compare import PythonExpression
from portwright.compare.strategies import text

INF = float("inf")
NAN = float("nan")

SPECIAL_FLOATS = [
    0.0, -0.0, 0.5, -0.5, 1.5, 2.5, -2.5, 3.5, 0.125, 0.1, 0.2, 0.3, 1 / 3, 2 / 3,
    0.30000000000000004, 123456.789, -123456.789, 1e16, 1e15, 9999999000000000.0,
    1e-4, 1e-5, 1e-7, 1.7976931348623157e308, 5e-324, -5e-324, 2.2250738585072014e-308,
    2.225073858507201e-308, 1e22, 1e23, INF, -INF, NAN, -NAN, 0.000123456, 99.995, 1.005,
    2.675, 0.045, 1e100, 1.2345678901234567e19, 9007199254740992.0, 9007199254740993.0,
    4.35, 0.285, 1.15, 1e300, -1e-300, 0.05, 0.15, 0.25, 0.35, 12345.6789e-3, 1e-310,
    999999.5, 9.999999999999999e22, 1e21, 1e-6, 1e-16, 1e17, 123.0, -1.0, 1.0, 100.0,
    2.0 ** 63, 2.0 ** 64, -2.0 ** 63, 2.0 ** 52 + 0.5, 0.49999999999999994, 4503599627370495.5,
    1e308, 8.98846567431158e307, 0.9999999999999999, 9.5, 10.5, 0.95, 0.995, 5e-5, 15.5,
]

SPECIAL_INTS = [
    0, 1, -1, 2, 5, 7, -7, 9, 10, 15, 25, -25, 35, 45, 42, 99, 100, 255, -255, 1000, 65, 97,
    1234567, -1234567, 10 ** 15, 2 ** 31 - 1, 2 ** 31, -2 ** 31, 2 ** 32, 2 ** 53, 2 ** 53 + 1,
    -2 ** 53 - 1, 2 ** 63 - 1, 2 ** 63, -2 ** 63, -2 ** 63 - 1, 2 ** 64 - 1, 2 ** 64, -2 ** 64,
    2 ** 64 + 3, -2 ** 70, 10 ** 20 + 5, 10 ** 30, 10 ** 40, 3 ** 100, -7 ** 80, 2 ** 127,
    2 ** 128, 2 ** 1023, 2 ** 1024, -2 ** 1024, 2 ** 1024 - 2 ** 970, 2 ** 1024 - 2 ** 971 - 1,
    2 ** 54 + 3, 2 ** 53 + 3, 0x10FFFF, 0x110000, 0xD800, 0xDFFF, 0x7F, 0x80, 0xFF, 0x100, 1114111,
]


def bits_float():
    return st.integers(min_value=0, max_value=2 ** 64 - 1).map(lambda b: struct.unpack("<d", struct.pack("<Q", b))[0])


def floats():
    return st.one_of(
        st.sampled_from(SPECIAL_FLOATS),
        st.floats(allow_nan=True, allow_infinity=True),
        bits_float(),
        st.floats(min_value=-1e6, max_value=1e6),
        st.integers(-10 ** 6, 10 ** 6).map(lambda n: n / 1000),
        st.integers(-10 ** 6, 10 ** 6).map(lambda n: n / 8),
        st.tuples(st.integers(1, 9999), st.integers(-30, 30)).map(lambda t: float(f"{t[0]}e{t[1]}")),
        st.tuples(st.integers(-99999, 99999), st.integers(-330, 310)).map(lambda t: t[0] * 10.0 ** t[1] if abs(t[1]) < 300 else float(f"{t[0]}e{t[1]}")),
    )


def ints():
    return st.one_of(
        st.sampled_from(SPECIAL_INTS),
        st.integers(-1000, 1000),
        st.integers(),
        st.integers(-2 ** 70, 2 ** 70),
        st.integers(-10 ** 400, 10 ** 400),
        st.tuples(st.integers(-9, 9), st.integers(0, 1100)).map(lambda t: t[0] * 2 ** t[1] + (t[0] % 3)),
        st.tuples(st.integers(-99, 99), st.integers(0, 60)).map(lambda t: t[0] * 10 ** t[1]),
    )


def numbers():
    return st.one_of(ints(), floats(), st.booleans())


def scalars():
    """Values for format/repr style calls: numbers, bools, strings, None."""
    return st.one_of(ints(), floats(), st.booleans(), text(max_size=12), st.none())


def leaf_values():
    return st.one_of(
        ints(), floats(), st.booleans(), st.none(), text(max_size=8),
        st.binary(max_size=6), st.binary(max_size=6).map(bytearray),
    )


def containers():
    hashable = st.one_of(ints(), floats(), st.booleans(), st.none(), text(max_size=6), st.binary(max_size=4))
    set_members = st.one_of(ints(), floats(), st.booleans(), st.none())
    return st.recursive(
        leaf_values(),
        lambda inner: st.one_of(
            st.lists(inner, max_size=4),
            st.lists(inner, max_size=4).map(tuple),
            st.dictionaries(hashable, inner, max_size=3),
            # Sets hold no str/bytes: their order follows per-process hash randomization.
            st.frozensets(set_members, max_size=3),
            st.sets(set_members, max_size=3),
        ),
        max_leaves=8,
    )


def any_value():
    return st.one_of(leaf_values(), containers())


# Values outside the built-in whitelist: the shim must raise TypeError.
FOREIGN = [
    PythonExpression("__import__('decimal').Decimal('1.5')"),
    PythonExpression("__import__('fractions').Fraction(1, 3)"),
    PythonExpression("1+2j"),
    PythonExpression("range(3)"),
    PythonExpression("[1, __import__('decimal').Decimal('2')]"),
    PythonExpression("{'a': 1j}"),
    PythonExpression("{1j: 1}"),
    PythonExpression("((1, (2, 1j)),)"),
    PythonExpression("frozenset({1j})"),
    PythonExpression("object()"),
]


def foreign():
    return st.sampled_from(FOREIGN)


# ----- format spec -------------------------------------------------------------

FILL_CHARS = ["", "", "", "x", "0", " ", "*", "{", "}", "日", "\U0001f600", "\x00", "\ud800", "<", "=", "%", ".", ","]


def format_specs(kinds="bcdeEfFgGnosxX%"):
    """Structured specs: [[fill]align][sign][z][#][0][width][grouping][.precision][type]."""
    @st.composite
    def build(draw):
        parts = []
        align = draw(st.sampled_from(["", "", "<", ">", "^", "="]))
        if align:
            parts.append(draw(st.sampled_from(FILL_CHARS)))
            parts.append(align)
        parts.append(draw(st.sampled_from(["", "", "+", "-", " "])))
        parts.append(draw(st.sampled_from(["", "", "", "z"])))
        parts.append(draw(st.sampled_from(["", "", "#"])))
        parts.append(draw(st.sampled_from(["", "", "0"])))
        parts.append(draw(st.sampled_from(["", "", "", "1", "5", "8", "12", "20", "0", "30", "007"])))
        parts.append(draw(st.sampled_from(["", "", "", ",", "_"])))
        if draw(st.booleans()):
            parts.append("." + draw(st.sampled_from(["0", "1", "2", "3", "5", "6", "10", "17", "20", "50"])))
        parts.append(draw(st.sampled_from(["", ""] + list(kinds))))
        return "".join(parts)
    return build()


def near_miss_specs():
    return st.one_of(
        format_specs(),
        format_specs("cdsrayz"),
        st.lists(st.sampled_from(list("<>^=+- z#0123456789,_.bcdeEfFgGnosxX%%sz{}x日")), max_size=8).map("".join),
        text(max_size=5),
        st.sampled_from(["~", "..", ".f.", "1.2.3", "<<<", ",,", ",_", "_,", "99999999999999999999",
                         ".99999999999999999999d", "y", "dd", "#s", "+c", ",c", "zd", "=s", "0s", ".2c",
                         "#c", "_s", ",s", ".", "10.", ".x", "2147483648", ".2147483648f", "+", "z,"]),
    )
