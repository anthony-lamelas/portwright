"""Strengthen stage: float binades, fixed-point precisions past 22 digits,
bytes reprs with a lone quote kind, and %-format precisions near INT_MAX."""
import math

from hypothesis import given, settings, strategies as st
from portwright.compare import same
from portwright.compare.strategies import text

from pcf_strategies import floats, ints


# ---------- every binade of float repr ----------
# Shortest-digit algorithms branch on the binary exponent (e.g. Ryu's
# `e2 > 3` table offset), so each exponent must be visited, not sampled.

@settings(max_examples=4)
@given(st.integers(0, 2**52 - 1), st.booleans())
def test_float_repr_every_binade(mantissa, neg):
    sign = -1.0 if neg else 1.0
    for e in range(-1074, 1024):
        if e < -1022:
            x = math.ldexp(mantissa >> min(52, -1022 - e), -1022 - 52)
        else:
            x = math.ldexp(1.0 + mantissa / 2**52, e)
        same("float_repr", sign * x)


@given(st.integers(-1074, 1023), st.sampled_from([0, 1, 2**51, 2**52 - 1, 3, 2**50 + 1]),
       st.sampled_from(["", "r", "e", ".17g", ".3f"]))
def test_float_binade_edges(e, frac, spec):
    x = math.ldexp(1.0 + frac / 2**52, e) if e >= -1022 else math.ldexp(frac or 1, e)
    if math.isfinite(x):
        same("float_repr", x)
        same("format_value", x, spec)


# ---------- exact decimal ties for shortest repr ----------

@st.composite
def tie_floats(draw):
    kind = draw(st.integers(0, 2))
    if kind == 0:
        # 2**E plus a few ulps: exact expansions one digit longer than the shortest.
        e = draw(st.integers(30, 60))
        j = draw(st.integers(-300, 300))
        return math.ldexp(1.0, e) + j * math.ldexp(1.0, e - 52)
    if kind == 1:
        return math.ldexp(draw(st.integers(1, 2**53 - 1)) | 1, -draw(st.integers(1, 30)))
    return float(draw(st.integers(1, 2**53 - 1)) | 1) * 2.0 ** draw(st.integers(0, 22))


@given(tie_floats(), st.sampled_from(["", "r", "g", ".16g", ".17g", "e"]))
def test_float_ties(x, spec):
    same("float_repr", x)
    same("format_value", x, spec)
    same("percent_format", "%r %s", (x, x))


@given(st.integers(1, 1074), st.booleans())
def test_powers_of_two_repr(k, neg):
    x = 2.0 ** -k
    same("float_repr", -x if neg else x)
    if k <= 1023:
        same("float_repr", 2.0 ** k)


# ---------- fixed precisions above 22 ----------

PRECS = st.one_of(st.integers(20, 45), st.sampled_from([22, 23, 24, 38, 39, 40, 60, 100]))


@given(floats(), PRECS, st.sampled_from(list("fFeEgG%")), st.sampled_from(["", ",", "_"]))
def test_format_large_precision(x, prec, kind, group):
    sep = group if kind in "fF%" else ""
    same("format_value", x, f"{sep}.{prec}{kind}")


@given(st.one_of(floats(), st.floats(1e-3, 1e6)), PRECS, st.sampled_from(list("fFeEgG")))
def test_percent_large_precision(x, prec, kind):
    same("percent_format", f"%.{prec}{kind}", x)
    same("str_format", f"{{:.{prec}{kind}}}", x)


@given(st.floats(1e-3, 1e6), PRECS)
def test_round_large_ndigits(x, nd):
    same("round_number", x, nd)


# ---------- bytes / bytearray repr quoting ----------

def quoty_bytes():
    return st.one_of(
        st.binary(max_size=8),
        st.lists(st.sampled_from([b"'", b'"', b"a", b"\\", b"\n", b"\x00", b"\xff", b" "]), max_size=8).map(b"".join),
        st.sampled_from([b"'", b"a'b", b"''", b'"', b"'\"", b"it's"]),
    )


@given(quoty_bytes(), st.booleans(), st.sampled_from(["%r", "%a", "%s", "%5r", "%-12r", "%.4r"]))
def test_bytes_repr_quotes(b, arr, tpl):
    v = bytearray(b) if arr else b
    same("percent_format", tpl, v)
    same("percent_format", tpl, (v,))
    same("format_value", v, "")
    same("str_format", "{!r} {!a} {}", v, v, v)


# ---------- %-format precision near INT_MAX ----------

@given(st.integers(2**31 - 8, 2**31 + 2), st.sampled_from(list("diuxXo")),
       st.one_of(st.sampled_from([0, 1, -1, 255]), ints(80)), st.sampled_from(["", "#", "-", "0"]))
def test_percent_int_precision_near_int_max(prec, kind, n, flag):
    same("percent_format", f"%{flag}.{prec}{kind}", n)
    same("percent_format", b"%" + flag.encode() + b"." + str(prec).encode() + kind.encode(), n)
