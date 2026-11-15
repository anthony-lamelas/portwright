"""Targeted checks for narrow behaviours that random generation rarely reaches."""
import math
import struct

from hypothesis import given, strategies as st
from portwright.compare import same
from portwright.compare.strategies import text

from pcf_strategies import ints, floats

NEG_NAN = -float("nan")


def neg_nans():
    # NaNs with the sign bit set, including different payloads.
    return st.one_of(
        st.just(NEG_NAN),
        st.integers(1, 2**52 - 1).map(lambda m: struct.unpack("<d", struct.pack("<Q", (0xFFF << 52) | m))[0]),
    )


def signed_zero_floats():
    return st.one_of(st.sampled_from([-0.0, 0.0, -1e-300, -5e-324, -0.4, -0.5, -0.04, -1e-20]),
                     st.floats(-1.0, 0.0), neg_nans())


# ---------- negative zero ----------

@given(st.sampled_from([-0.0, 0.0]))
def test_neg_zero_repr(z):
    same("float_repr", z)
    same("int_to_float", 0)
    same("round_number", z)
    same("round_number", z, 0)
    same("round_number", z, 2)
    same("round_number", z, -2)


@given(signed_zero_floats(), st.sampled_from(["", "f", ".0f", ".1f", "e", ".2e", "g", ".3g", "G", "%", ".0%",
                                              "+", "+.1f", " ", "z", "z.1f", "z.0f", "zg", "08.2f", "<8", "^9.1e", "r", "n"]))
def test_neg_zero_format(x, spec):
    same("format_value", x, spec)
    same("float_repr", x)
    same("str_format", "{}|{!r}|{:" + spec.replace("r", "") + "}", [x, x, x])


@given(signed_zero_floats(), st.sampled_from([None, 0, 1, 2, 5, -1, -3]))
def test_neg_zero_round(x, nd):
    same("round_number", x, nd)


@given(st.sampled_from(["-0", "-0.0", " -0.0 ", "-0e10", "-0.000", "-1e-400", "+0.0", "0.0", "-0x0", "-nan", "-NaN", "-inf"]))
def test_neg_zero_parse(t):
    same("parse_float", t)
    same("parse_float", t.encode())


@given(st.sampled_from(["floor_div", "modulo", "div_mod", "true_div"]),
       st.sampled_from([-0.0, 0.0, -1.0, 1.0, -3.5, 3.5, 0, -1, 1, -5, 5, float("inf"), float("-inf"), -1e-300, 1e-300]),
       st.sampled_from([-0.0, 0.0, -1.0, 1.0, -3.5, 3.5, -1, 1, -5, 5, float("inf"), float("-inf"), 1e300, -1e300]))
def test_neg_zero_arith(op, a, b):
    same(op, a, b)


@given(st.sampled_from(["%f", "%.0f", "%e", "%g", "%r", "%s", "%+.1f", "% .2e", "%d", "%i", "%5.1f", "%G"]), signed_zero_floats())
def test_neg_zero_percent(t, x):
    same("percent_format", t, x)
    same("percent_format", t, (x,))


# ---------- NaN sign ----------

@given(neg_nans(), st.sampled_from(["", "f", "e", "g", "%", "+", "-", " ", "z", "10", "<6", "F", "E", "G", "n", ".3"]))
def test_neg_nan(x, spec):
    same("float_repr", x)
    same("format_value", x, spec)
    same("str_format", "{!r} {} {!s}", [x, x, x])
    same("percent_format", "%r %s %f %e %g %+F", (x,) * 6)
    same("round_number", x, 2)
    same("int_str", [x])


# ---------- round(int, -n) half-to-even ----------

@st.composite
def int_halves(draw):
    m = draw(st.integers(1, 40))
    k = draw(st.one_of(st.integers(-20, 20), st.integers(-10**30, 10**30)))
    off = draw(st.sampled_from([0, 0, 0, 1, -1]))
    return k * 10**m + 5 * 10**(m - 1) + off, -m


@given(int_halves())
def test_round_int_halves(xn):
    x, nd = xn
    same("round_number", x, nd)
    same("round_number", -x, nd)


@given(st.integers(-200, 200), st.integers(-3, 0))
def test_round_small_int(x, nd):
    same("round_number", x, nd)


# ---------- %c ----------

@given(st.one_of(st.integers(0, 0x10FFFF), st.integers(32, 0x7F), st.sampled_from([0, 10, 65, 255, 256, 0xD800, 0xDFFF, 0x10FFFF, 0x110000, -1])),
       st.sampled_from(["%c", "%3c", "%-4c", "[%c]", "%(k)c", "%.2c", "%*c"]))
def test_percent_c_int(n, t):
    if "(k)" in t:
        same("percent_format", t, {"k": n})
    elif "*" in t:
        same("percent_format", t, (4, n))
    else:
        same("percent_format", t, n)
        same("percent_format", t, (n,))
        same("percent_format", t, [n])


@given(st.one_of(st.integers(0, 255), st.integers(-2, 300)), st.booleans())
def test_percent_c_bytes(n, arr):
    t = b"<%c>"
    same("percent_format", bytearray(t) if arr else t, n)
    same("percent_format", t, (n,))
    same("percent_format", t, (bytes([n % 256]),))


@given(st.one_of(text(max_size=3), st.booleans(), st.floats(allow_nan=False, allow_infinity=False)))
def test_percent_c_other(v):
    same("percent_format", "%c", v)
    same("percent_format", "%c", (v,))


# ---------- nested format spec ----------

@given(st.one_of(st.integers(-999, 999), text(max_size=3), st.floats(-100, 100)),
       st.integers(0, 30), st.integers(0, 8),
       st.sampled_from(["{:{}}", "{:>{}}", "{:*^{}}", "{:<{}}", "{0:{1}}", "{a:{w}}", "{:{}.{}}", "{:{}{}}", "{:0{}}", "{:{}.{}f}", "{:{}s}", "{:{}d}", "{:{}x}", "{!r:{}}", "{:{}}{:{}}"]))
def test_nested_width(v, w, p, t):
    if t == "{a:{w}}":
        same("str_format", t, [], {"a": v, "w": w})
    elif t == "{:{}{}}":
        same("str_format", t, [v, w, "s" if isinstance(v, str) else ""])
    elif t == "{:{}}{:{}}":
        same("str_format", t, [v, w, v, p])
    else:
        same("str_format", t, [v, w, p])


@given(st.integers(0, 30), st.sampled_from(["<", ">", "^", "="]), st.sampled_from(["*", "-", "0", "日"]), st.integers(-50, 50))
def test_nested_align_fill(w, align, fill, v):
    same("str_format", "{:{}{}{}}", [v, fill, align, w])
    same("str_format", "{0:{1}{2}{3}d}", [v, fill, align, w])


# ---------- auto/manual numbering ----------

@given(st.sampled_from(["{0}{}", "{}{0}", "{}{1}", "{1}{}", "{0:{}}", "{:{0}}", "{a}{}{0}", "{}{a}{1}", "{0}{:{}}", "{:{1}}",
                        "x{}y{0}z", "{0}{1}{}", "{!r}{0!s}", "{0[0]}{}", "{}{0[0]}"]),
       st.lists(st.one_of(st.integers(0, 9), text(max_size=2), st.lists(st.integers(0, 3), min_size=1, max_size=2)), min_size=0, max_size=3))
def test_numbering_switch(t, args):
    same("str_format", t, args, {"a": 1})


@given(st.integers(0, 4), st.sampled_from(["{}", "{0}", "{1}", "{2}", "{:{}}", "{}{}", "{}{}{}"]))
def test_index_errors(n, t):
    same("str_format", t, list(range(n)))


# ---------- big int true division ----------

@st.composite
def big_div_pairs(draw):
    qbits = draw(st.integers(-60, 1100))
    bbits = draw(st.integers(0, 1500))
    b = draw(st.integers(2**bbits, 2**(bbits + 1)))
    if draw(st.booleans()):
        b |= 1
    a = draw(st.integers(2**max(0, bbits + qbits), 2**max(1, bbits + qbits + 1)))
    if draw(st.booleans()):
        a = -a
    if draw(st.booleans()):
        b = -b
    return a, b


@given(big_div_pairs())
def test_true_div_big(ab):
    a, b = ab
    same("true_div", a, b)


@given(st.integers(54, 1023), st.integers(1, 2**20), st.integers(1, 2**60))
def test_true_div_beyond_53_bits(bits, low, d):
    a = 2**bits + low
    same("true_div", a, d)
    same("true_div", a, 3)
    same("true_div", -a, 7)
    same("true_div", d, a)
    same("true_div", a, a - 1)


@given(st.integers(1, 2**20), st.integers(1024, 3000))
def test_true_div_huge_operands(k, bits):
    same("true_div", 2**bits + k, 2**(bits - 3) + 1)
    same("true_div", k, 2**bits)
    same("true_div", 2**bits, k)


# ---------- ascii() escape widths (\xhh / \uhhhh / \Uhhhhhhhh) ----------

def escape_width_text():
    cps = st.one_of(
        st.integers(0x20, 0x7E),
        st.integers(0x80, 0xFF),
        st.integers(0x100, 0xFFFF),        # \uhhhh range, incl. lone surrogates
        st.sampled_from([0xFF, 0x100, 0xFFFF, 0x10000, 0xD800, 0xDFFF, 0x2028]),
        st.integers(0x10000, 0x10FFFF),
    )
    return st.one_of(st.lists(cps, max_size=8).map(lambda l: "".join(map(chr, l))), text())


@given(escape_width_text())
def test_ascii_conversion_escape_widths(s):
    same("str_format", "{!a}", s)
    same("str_format", "{0!a:>30}|{1!a}", s, [s, (s,)])
    same("percent_format", "%a", s)
    same("percent_format", "%a|%.5a", (s, {s: s}))
    same("percent_format", b"%a", s)
