"""Shared strategies for pc_format checks."""
import struct
from decimal import Decimal

from hypothesis import strategies as st
from portwright.compare.strategies import text, characters

INF = float("inf")
NAN = float("nan")

SPECIAL_FLOATS = [
    0.0, -0.0, 0.5, -0.5, 1.5, 2.5, -2.5, 0.1, 0.2, 0.3, 1 / 3, 2 / 3, 1.005, 2.675, 0.045, 99.995,
    1e16, 1e15, 9999999000000000.0, 1e-4, 1e-5, 1e-7, 1.7976931348623157e308, 5e-324,
    2.2250738585072014e-308, 2.225073858507201e-308, 1e22, 1e23, INF, -INF, NAN, 123456.789,
    9007199254740992.0, 9007199254740993.0, 4.35, 0.285, 1.15, 1e300, -1e-300, 1.2345678901234567e19,
    0.30000000000000004, 1e100, 12345678901234567890.0, 0.000123456, 5e-5, 9.5, 0.95, 0.0095, 1e-323,
]


def float_from_bits():
    return st.integers(0, 2**64 - 1).map(lambda b: struct.unpack("<d", struct.pack("<Q", b))[0])


def floats():
    return st.one_of(
        st.sampled_from(SPECIAL_FLOATS),
        st.floats(allow_nan=True, allow_infinity=True),
        float_from_bits(),
        st.integers(-10**6, 10**6).map(lambda n: n / 8),
        st.integers(-10**6, 10**6).map(lambda n: n / 1000),
        st.integers(-2**60, 2**60).map(float),
        st.integers(-30, 30).map(lambda e: 10.0 ** e),
        st.tuples(st.integers(1, 99999), st.integers(-25, 25)).map(lambda t: float(f"{t[0]}e{t[1]}")),
    )


SPECIAL_INTS = [0, 1, -1, 7, -7, 9, 10, 255, -256, 1000, 2**31, 2**31 - 1, -2**31, 2**32, 2**53, 2**53 + 1,
                2**63, 2**63 - 1, -2**63, 2**64, 2**64 - 1, -2**64, 10**15, 10**20 + 5, 10**40, 3**100, -7**80,
                1114111, 1114112, 55296, 57343, 65536]


def ints(max_bits=300):
    return st.one_of(
        st.sampled_from(SPECIAL_INTS),
        st.integers(-1000, 1000),
        st.integers(-2**64, 2**64),
        st.integers(-2**max_bits, 2**max_bits),
        st.integers(0, max_bits).flatmap(lambda b: st.sampled_from([2**b, 2**b - 1, -2**b, 10**(b // 3), -10**(b // 3) + 1])),
    )


def numbers():
    return st.one_of(ints(), floats(), st.booleans())


def scalars():
    return st.one_of(st.none(), st.booleans(), ints(), floats(), text(max_size=12),
                     st.binary(max_size=8), st.binary(max_size=8).map(bytearray))


def safe_sets():
    """Sets without str/bytes (allowed)."""
    elem = st.one_of(st.none(), st.booleans(), st.integers(-50, 50), st.floats(allow_nan=False),
                     st.tuples(st.integers(-5, 5), st.none()))
    return st.one_of(st.sets(elem, max_size=4), st.frozensets(elem, max_size=4))


def values(depth=2):
    base = scalars()
    if depth == 0:
        return base
    inner = values(depth - 1)
    return st.one_of(
        base, base,
        st.lists(inner, max_size=3),
        st.lists(inner, max_size=3).map(tuple),
        st.dictionaries(st.one_of(text(max_size=4), st.integers(-3, 3), st.none(), st.floats(allow_nan=False)), inner, max_size=3),
        safe_sets(),
    )


# Values that the shim refuses: Decimal, sets of strings, sets holding NaN with other items.
REFUSED = [Decimal("1.5"), {"a"}, frozenset({b"x"}), {("a", 1)}, [Decimal(2)], {"k": Decimal("0")},
           (1, {"s"}), frozenset({frozenset({"a"})}), {1: [frozenset({"z"})]},
           {float("nan"), 1}, frozenset({(float("nan"), 2), None}), [{1.5, frozenset({float("nan")})}]]


def refused():
    return st.sampled_from(REFUSED)


# ---------- format spec grammar ----------
TYPES_INT = list("bcdoxXn") + [""]
TYPES_FLOAT = list("eEfFgGn%") + [""]
TYPES_ALL = list("bcdoxXneEfFgG%s") + [""]


def width_strat():
    return st.one_of(st.just(""), st.integers(0, 40).map(str), st.sampled_from(["00", "007", "100", "9" * 20, "9223372036854775808", "1" + "0" * 25]))


def precision_strat():
    return st.one_of(st.just(""), st.integers(0, 25).map(lambda n: "." + str(n)),
                     st.sampled_from([".", ".50", ".100", ".120", ".00", "." + "9" * 20, ".9223372036854775808"]))


@st.composite
def format_specs(draw, types=TYPES_ALL, valid=False):
    parts = []
    if draw(st.booleans()):
        align = draw(st.sampled_from("<>^="))
        if draw(st.booleans()):
            fill = draw(st.one_of(st.sampled_from(list("*0 x{}日-")), characters()))
            parts.append(fill)
        parts.append(align)
    parts.append(draw(st.sampled_from(["", "", "+", "-", " "])))
    parts.append(draw(st.sampled_from(["", "", "", "z"])))
    parts.append(draw(st.sampled_from(["", "", "#"])))
    parts.append(draw(st.sampled_from(["", "", "0"])))
    parts.append(draw(st.integers(0, 30).map(str) | st.just("")) if valid else draw(width_strat()))
    parts.append(draw(st.sampled_from(["", "", "", ",", "_", ",_", "_,"])))
    if valid:
        parts.append(draw(st.sampled_from(["", ""] + [f".{n}" for n in (0, 1, 2, 3, 6, 10, 17, 20)])))
        parts.append(draw(st.sampled_from(types)))
    else:
        parts.append(draw(precision_strat()))
        parts.append(draw(st.one_of(st.sampled_from(types), st.sampled_from(list("aAyZ") + ["dd", "s ", "%%"]))))
    return "".join(parts)


def any_spec():
    return st.one_of(format_specs(), format_specs(), text(max_size=6),
                     st.text(alphabet="<>^=+- z#0123456789,_.bcdoxXneEfFgG%s", max_size=10))
