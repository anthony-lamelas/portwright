"""Rules for the Rust review findings.

#1 round_int: the `k > bits(n)+1 -> 0` shortcut and the 10**k path right behind it.
#2 memory budget: huge widths/precisions. CPython under the worker's 1 GiB limit refuses at once
   (MemoryError), or returns short text when nothing big is built (inf/nan/0.0 with 'g', str
   precision, the '.2147483647e' wrap). Widths are kept well above the limit for each fill's byte
   width (1-byte >= 1.1e9, 2-byte >= 600M, 4-byte >= 300M); mid-range values make CPython really
   allocate and are avoided.
#3 deep nesting: the worker can't carry nesting past ~330, so depths stay <= 300 (see tried.md).
"""
import sys

from hypothesis import example, given, settings, strategies as st
from portwright.compare import same, PythonExpression
from portwright.compare.strategies import text

from pcstrats import floats

# ----- #1 round_number on ints around the 10**k / 2 boundary ---------------------------------


@given(st.integers(1, 1200), st.integers(-3, 3), st.integers(-3, 3), st.booleans())
@example(1, 0, 0, False)
@example(1, 0, 1, False)
@example(4, 0, 0, True)
@example(64, 0, 0, False)
@example(1000, 2, -1, True)
def test_round_int_bitlength_boundary(bits, delta, k_shift, neg):
    # n has exactly `bits` bits (or close), ndigits sits around -(bits+1), the shortcut's edge.
    n = (1 << (bits - 1)) + delta if bits > 1 else 1 + max(delta, 0)
    if neg:
        n = -n
    same("round_number", n, -(bits + 1 + k_shift))
    same("round_number", n, -(bits + k_shift))


@given(st.integers(1, 1300), st.integers(-3, 3), st.booleans(), st.integers(-2, 2))
@example(1, 0, False, 0)
@example(1, 0, True, 0)
@example(2, 0, False, 0)
@example(1, 10, False, 0)
@example(300, 0, True, 0)
def test_round_int_half_boundary(k, delta, neg, k_shift):
    # n = 10**k/2 + delta: exact half (ties-to-even), just under and just over, at ndigits=-k.
    n = 5 * 10 ** (k - 1) + delta
    m = 15 * 10 ** (k - 1) + delta
    if neg:
        n, m = -n, -m
    same("round_number", n, -k)
    same("round_number", m, -k)
    same("round_number", n, -(k + k_shift) if k + k_shift > 0 else -k)
    same("round_number", 10 ** k - 1 if not neg else 1 - 10 ** k, -k)


@settings(deadline=None)
@given(st.integers(1300, 4299), st.integers(-2, 2), st.booleans())
@example(4299, 0, False)
@example(4299, -1, True)
@example(4299, 1, False)
@example(4000, 0, True)
def test_round_int_large_half_boundary(k, delta, neg):
    # Inputs and results must stay under the 4300-digit str limit: the worker str()s both.
    n = 5 * 10 ** (k - 1) + delta
    if neg:
        n = -n
    same("round_number", n, -k)
    same("round_number", n, -k - 1)
    same("round_number", n, 1 - k)
    bits = k * 3
    p = -(1 << bits) if neg else (1 << bits)
    same("round_number", p, -(bits + 2))
    same("round_number", p, -(bits + 1))
    same("round_number", p + delta, -(len(str(abs(p))) - 1))


@settings(deadline=None)
@given(st.integers(-10 ** 40, 10 ** 40), st.one_of(
    st.sampled_from([-4300, -4301, -10 ** 4, -2 ** 16, -10 ** 5, -2 ** 17, -500_000, -10 ** 6]),
    st.integers(-10 ** 6, -4000)))
@example(0, -10 ** 6)
@example(-12345, -10 ** 6)
@example(10 ** 40, -2 ** 17)
@example(5, -10 ** 5)
def test_round_int_huge_negative_ndigits(n, nd):
    # Person's decision: abs(n) < 10**k/2 -> 0 at once, never MemoryError from an estimate.
    # k stays <= 10**6, where CPython still answers (it builds 10**k; 10**7 takes ~5 s, 2**63 hangs).
    same("round_number", n, nd)
    same("round_number", n, ndigits=nd)


# ----- #2 huge widths and precisions ---------------------------------------------------------

ONE_BYTE_WIDTHS = [1_100_000_000, 1_500_000_000, 2_000_000_000, 2_147_483_647, 2_147_483_648,
                   2 ** 32, 2 ** 40, 2 ** 62, sys.maxsize, sys.maxsize + 1, 10 ** 20]
WIDE_FILLS = [("Ā", [600_000_000, 1_000_000_000]), ("\U0001f600", [300_000_000, 700_000_000]),
              ("\ud800", [600_000_000]), ("*", ONE_BYTE_WIDTHS), (" ", ONE_BYTE_WIDTHS), ("0", ONE_BYTE_WIDTHS)]
HUGE_PRECISIONS = [1_100_000_000, 2_000_000_000, 2_147_483_646, 2_147_483_647, 2_147_483_648,
                   2 ** 40, sys.maxsize, sys.maxsize + 1]


@st.composite
def huge_width_specs(draw):
    fill, widths = draw(st.sampled_from(WIDE_FILLS))
    align = draw(st.sampled_from(["", "<", ">", "^", "="]))
    # Without an align the fill is unused and padding is 1-byte: only 1-byte-safe widths then.
    width = draw(st.sampled_from(widths if align else ONE_BYTE_WIDTHS))
    head = (fill + align) if align else draw(st.sampled_from(["", "0"]))
    sign = draw(st.sampled_from(["", "+", "-", " "]))
    alt = draw(st.sampled_from(["", "#"]))
    group = draw(st.sampled_from(["", "", ",", "_"]))
    prec = draw(st.sampled_from(["", "", ".2", ".0"]))
    kind = draw(st.sampled_from(["", "", "d", "x", "b", "f", "e", "g", "%", "s", "n", "c", "o"]))
    return f"{head}{sign}{alt}{width}{group}{prec}{kind}"


VALUES = st.one_of(st.sampled_from([0, 1, -5, 255, 10 ** 30, True, False, 0.0, -0.0, 1.5, -2.5,
                                    float("inf"), float("-inf"), float("nan"), "", "ab", "\ud800",
                                    "\U0001f600x"]), floats(), st.integers(-10 ** 6, 10 ** 6),
                   text(max_size=4))


@given(VALUES, huge_width_specs())
@example(1, "2000000000")
@example(1, "1200000000")
@example("x", "\U0001f600<300000000")
@example(1.5, "1500000000.3f")
@example(1, "99999999999999999999")
@example(1, "9223372036854775807")
@example(1, "9223372036854775808")
@example("ab", "=1100000000")
@example(float("nan"), ",1100000000")
def test_format_huge_width(value, spec):
    same("format_value", value, spec)


@given(VALUES, st.sampled_from(HUGE_PRECISIONS), st.sampled_from(["", "f", "F", "e", "E", "g", "G", "%", "n", "s", "d"]),
       st.sampled_from(["", ",", "_", "+", "#", "010", "<5"]))
@example(1.5, 2_147_483_647, "e", "")
@example(0.0, 1_100_000_000, "g", "")
@example(-0.0, 1_100_000_000, "", "")
@example(float("inf"), 2_000_000_000, "f", "")
@example(float("nan"), 1_100_000_000, "%", ",")
@example("ab", 3_000_000_000, "", "")
@example("\ud800", 2_147_483_648, "s", "")
@example(1.5, 2_147_483_648, "f", "")
@example(1.5, 1_100_000_000, "g", "")
@example(7, 1_100_000_000, "f", "")
def test_format_huge_precision(value, prec, kind, prefix):
    same("format_value", value, f"{prefix}.{prec}{kind}")


@given(VALUES, st.sampled_from(ONE_BYTE_WIDTHS[:5]), st.sampled_from(["s", "r", "d", "f", "e", "x", "c", "a"]),
       st.sampled_from(["", "-", "0", "+", "#"]))
@example(1, 2_000_000_000, "d", "")
@example("x", 1_100_000_000, "s", "-")
@example("x", 1_100_000_000, "c", "")
def test_percent_huge_width(value, width, kind, flag):
    same("percent_format", f"%{flag}{width}{kind}", (value,))
    same("percent_format", f"%{flag}*{kind}", (width, value))
    same("percent_format", f"%.{width}s", (value,))


@given(VALUES, st.sampled_from(ONE_BYTE_WIDTHS), st.sampled_from(["", "s", "d", "f", ">", "^", "*<"]))
@example(1, 2_000_000_000, "")
@example("a", 1_100_000_000, ">")
@example(1, 9_223_372_036_854_775_808, "")
def test_str_format_huge_width(value, width, kind):
    if kind in ("", "s", "d", "f"):
        spec = f"{width}{kind}"
    else:
        spec = f"{kind}{width}"
    same("str_format", "{:" + spec + "}", (value,))
    same("str_format", "{0:{1}}", (value, width))
    same("str_format", "{x:.{p}}", (), {"x": value, "p": width})


# ----- #3 nesting --------------------------------------------------------------------------

LEAVES = st.one_of(st.none(), st.integers(-10 ** 20, 10 ** 20), floats(), text(max_size=3),
                   st.binary(max_size=3), st.booleans())


@st.composite
def nested(draw, max_depth=300):
    depth = draw(st.one_of(st.integers(0, 30), st.integers(30, max_depth), st.sampled_from([max_depth])))
    value = draw(LEAVES)
    for _ in range(depth):
        shape = draw(st.sampled_from(["list", "tuple", "dict", "dictkey", "pair"]))
        if shape == "list":
            value = [value]
        elif shape == "tuple":
            value = (value,)
        elif shape == "dict":
            value = {draw(st.one_of(st.integers(-3, 3), text(max_size=2))): value}
        elif shape == "dictkey" and isinstance(value, (int, str, tuple, type(None), float, bytes)):
            try:
                hash(value)
                value = {value: draw(LEAVES)}
            except TypeError:
                value = [value]
        else:
            value = [draw(LEAVES), value]
    return value


def _nest(depth, leaf):
    value = leaf
    for i in range(depth):
        value = [value] if i % 3 else (value,)
    return value


@settings(deadline=None)
@given(nested(), st.sampled_from(["", "s", "x", "10", "<5"]))
@example(_nest(300, 1.5), "")
@example(_nest(300, "\ud800"), "")
@example(_nest(250, None), "x")
def test_nested_format_value(value, spec):
    same("format_value", value, spec)


@settings(deadline=None)
@given(nested(), st.sampled_from(["{}", "{!r}", "{!a}", "{0!s:>5}", "{0[0]}", "{:x}"]),
       st.sampled_from(["%s", "%r", "%a", "%d"]))
@example(_nest(300, 2.5), "{!r}", "%r")
@example({1: _nest(290, "\U0001f600")}, "{!a}", "%a")
def test_nested_templates(value, template, ptemplate):
    same("str_format", template, (value,))
    same("str_format", "{k}", (), {"k": value})
    same("percent_format", ptemplate, (value,))
    same("percent_format", "%(k)s", {"k": value})


@settings(deadline=None)
@given(nested())
@example(_nest(300, 0))
@example([_nest(299, PythonExpression("__import__('decimal').Decimal(1)"))])
def test_nested_other_apis(value):
    same("float_repr", value)
    same("int_str", value)
    same("round_number", value)
    same("true_div", value, 1)
    same("parse_int", "7", value)
