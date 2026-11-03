from hypothesis import given, strategies as st
from portwright.compare import same, PythonExpression
from portwright.compare.strategies import text

from pcstrats import ints, floats, numbers, any_value, foreign, scalars


# ----- float_repr ----------------------------------------------------------------

@given(floats())
def test_float_repr(x):
    same("float_repr", x)


@given(st.one_of(ints(), st.booleans(), st.none(), text(max_size=8), any_value(), foreign()))
def test_float_repr_other(x):
    same("float_repr", x)


# ----- int_str --------------------------------------------------------------------

@given(ints())
def test_int_str(n):
    same("int_str", n)


@given(st.integers(4290, 4310), st.integers(1, 9), st.booleans())
def test_int_str_digit_limit(digits, lead, negative):
    # Ints over 4300 digits can't cross the wire as text, so each side builds them.
    sign = "-" if negative else ""
    same("int_str", PythonExpression(f"{sign}{lead} * 10 ** {digits - 1}"))
    same("int_str", PythonExpression(f"{sign}{lead} * 10 ** {digits - 1} - 1"))
    same("int_to_float", PythonExpression(f"{sign}{lead} * 10 ** {digits - 1}"))
    same("format_value", PythonExpression(f"{sign}{lead} * 10 ** {digits - 1}"), "x")
    same("float_repr", PythonExpression(f"{sign}{lead} * 10 ** {digits - 1}"))


@given(st.one_of(floats(), st.booleans(), st.none(), text(max_size=8), any_value(), foreign()))
def test_int_str_other(x):
    same("int_str", x)


# ----- round_number ---------------------------------------------------------------

NDIGITS = st.one_of(st.none(), st.integers(-30, 30), st.integers(-400, 400),
                    st.sampled_from([-2 ** 63, 2 ** 63, -2 ** 70, 2 ** 70, 2 ** 31, -2 ** 31 - 1, 323, 324, -308, -309]))


@given(floats())
def test_round_float_default(x):
    same("round_number", x)


@given(floats(), NDIGITS)
def test_round_float(x, nd):
    same("round_number", x, nd)
    same("round_number", x, ndigits=nd)


@given(st.integers(-10 ** 6, 10 ** 6), st.integers(-3, 6))
def test_round_halfway_decimals(n, nd):
    same("round_number", n / 1000 + 0.0005, nd)
    same("round_number", (2 * n + 1) / 2)


@given(st.one_of(ints(), st.booleans()), NDIGITS)
def test_round_int(n, nd):
    same("round_number", n)
    same("round_number", n, nd)


@given(st.integers(-10 ** 25, 10 ** 25), st.integers(-26, 2))
def test_round_int_halfway(n, nd):
    same("round_number", n * 10 + 5, nd)
    same("round_number", n, nd)


@given(st.one_of(numbers(), text(max_size=3), st.none(), any_value(), foreign()),
       st.one_of(st.none(), floats(), st.booleans(), text(max_size=3), st.integers(-5, 5), foreign()))
def test_round_bad(x, nd):
    same("round_number", x, nd)


# ----- parse_int ------------------------------------------------------------------

DIGIT_CHARS = list("0123456789abcdefghijklmnopqrstuvwxyzABCDEFXYZ") + [
    "_", "_", "٣", "١", "০", "１", "２", "Ⅻ", "²", "𝟘", "߁", "๓", "Ａ", "\ud800"]
WS = ["", "", " ", "\t", "\n", "\xa0", " ", "　", "\x1c", "\x85", "​", "\x0b"]


@st.composite
def int_texts(draw):
    s = draw(st.sampled_from(WS))
    s += draw(st.sampled_from(["", "", "-", "+", "--", "+-"]))
    s += draw(st.sampled_from(["", "", "", "0x", "0X", "0o", "0O", "0b", "0B", "0", "0_", "00"]))
    s += "".join(draw(st.lists(st.sampled_from(DIGIT_CHARS), max_size=8)))
    s += draw(st.sampled_from(WS))
    return s


BASES = st.one_of(st.sampled_from([0, 2, 8, 10, 16, 36, 10, 10, 0]), st.integers(-2, 40),
                  st.sampled_from([2 ** 64, -2 ** 64, 37]))


@given(int_texts())
def test_parse_int_default(t):
    same("parse_int", t)


@given(int_texts(), BASES)
def test_parse_int_base(t, base):
    same("parse_int", t, base)
    same("parse_int", t, base=base)


@given(st.integers(), st.integers(2, 36))
def test_parse_int_roundtrip(n, base):
    digits = "0123456789abcdefghijklmnopqrstuvwxyz"
    m, out = abs(n), ""
    while True:
        m, r = divmod(m, base)
        out = digits[r] + out
        if not m:
            break
    t = ("-" if n < 0 else "") + out
    same("parse_int", t, base)
    same("parse_int", t.upper(), base)
    same("parse_int", "_".join(t), base)


@given(st.integers(4290, 4310), st.sampled_from([10, 0, 16, 2, 8, 36]))
def test_parse_int_digit_limit(n, base):
    same("parse_int", "7" * n, base)
    same("parse_int", "-" + "1" * n, base)


@given(text(max_size=10), st.one_of(st.just(10), BASES))
def test_parse_int_any_text(t, base):
    same("parse_int", t, base)


@given(st.one_of(st.binary(max_size=8), int_texts().map(lambda s: s.encode("utf-8", "surrogatepass"))).flatmap(
    lambda b: st.sampled_from([b, bytearray(b)])), BASES)
def test_parse_int_bytes(b, base):
    same("parse_int", b)
    same("parse_int", b, base)


@given(st.one_of(floats(), ints(), st.booleans(), st.none(), any_value(), foreign()),
       st.one_of(st.just(10), BASES, floats(), st.none(), text(max_size=2), st.booleans()))
def test_parse_int_bad(x, base):
    same("parse_int", x)
    same("parse_int", x, base)


# ----- parse_float ----------------------------------------------------------------

@st.composite
def float_texts(draw):
    s = draw(st.sampled_from(WS))
    s += draw(st.sampled_from(["", "", "-", "+", "--"]))
    kind = draw(st.integers(0, 5))
    if kind == 0:
        s += draw(st.sampled_from(["inf", "INF", "Infinity", "iNfInItY", "nan", "NaN", "infinit", "infinityy",
                                   "nan(1)", "in", "na", "Inf_"]))
    else:
        digs = st.lists(st.sampled_from(list("0123456789") + ["_", "٣", "１", "๓"]), max_size=6).map("".join)
        s += draw(digs)
        if draw(st.booleans()):
            s += draw(st.sampled_from([".", ".", "..", "._"])) + draw(digs)
        if draw(st.booleans()):
            s += draw(st.sampled_from(["e", "E", "e+", "e-", "E_", "e--"])) + draw(digs)
    s += draw(st.sampled_from(WS))
    return s


@given(float_texts())
def test_parse_float_structured(t):
    same("parse_float", t)


@given(floats())
def test_parse_float_roundtrip(x):
    same("parse_float", repr(x))
    same("parse_float", "%.17e" % x)
    same("parse_float", "%.20g" % x)


@given(st.integers(1, 10 ** 30), st.integers(-400, 400), st.sampled_from(["", "-", " +"]))
def test_parse_float_decimal(mant, exp, sign):
    same("parse_float", f"{sign}{mant}e{exp}")
    same("parse_float", f"{sign}0.{mant}e{exp}")


@given(st.integers(1, 200), st.integers(1, 9))
def test_parse_float_long(n, d):
    same("parse_float", str(d) * n + "." + "5" * n)
    same("parse_float", "0." + "0" * (n + 300) + str(d))
    same("parse_float", "2.47032822920623272088" + "0" * n + str(d) + "e-324")


@given(text(max_size=10))
def test_parse_float_any_text(t):
    same("parse_float", t)


@given(st.one_of(st.binary(max_size=8), float_texts().map(lambda s: s.encode("utf-8", "surrogatepass"))).flatmap(
    lambda b: st.sampled_from([b, bytearray(b)])))
def test_parse_float_bytes(b):
    same("parse_float", b)


@given(st.one_of(floats(), ints(), st.booleans(), st.none(), any_value(), foreign()))
def test_parse_float_other(x):
    same("parse_float", x)


# ----- int_to_float ---------------------------------------------------------------

@given(ints())
def test_int_to_float(n):
    same("int_to_float", n)


@given(st.integers(0, 1100), st.integers(-3, 3), st.booleans())
def test_int_to_float_rounding(e, delta, neg):
    n = 2 ** e + delta * 2 ** max(0, e - 53) + (2 ** max(0, e - 54) if delta == 0 else 0)
    same("int_to_float", -n if neg else n)


@given(st.one_of(floats(), st.booleans(), st.none(), text(max_size=6), float_texts(), any_value(), foreign()))
def test_int_to_float_other(x):
    same("int_to_float", x)
