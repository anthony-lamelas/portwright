from hypothesis import given, strategies as st
from portwright.compare import same
from portwright.compare.strategies import text

from pcf_strategies import ints, floats, numbers, values, refused, scalars


@given(st.one_of(floats(), floats(), ints(), values()))
def test_float_repr(x):
    same("float_repr", x)


@given(st.one_of(ints(1000), ints(), st.booleans(), floats(), text(max_size=5), values()))
def test_int_str(n):
    same("int_str", n)


@given(st.integers(4290, 4310), st.integers(1, 9), st.booleans())
def test_int_str_digit_limit(ndigits, lead, neg):
    n = lead * 10 ** (ndigits - 1)
    same("int_str", -n if neg else n)


@given(st.one_of(floats(), ints(), st.booleans()))
def test_round_default(x):
    same("round_number", x)


@given(st.one_of(floats(), ints(), st.booleans()),
       st.one_of(st.integers(-30, 30), st.integers(-400, 400), st.sampled_from([2**31 - 1, 2**31, 2**63, 2**70, True, False, None])))
def test_round_ndigits(x, nd):
    same("round_number", x, nd)


# Huge negative ndigits on an int makes CPython compute 10**-ndigits (hangs), so floats only there.
@given(floats(), st.sampled_from([-2**31, -2**31 - 1, -2**63, -2**70, 2**63, 2**70, -1000, 1000]))
def test_round_float_huge_ndigits(x, nd):
    same("round_number", x, nd)


@given(st.one_of(text(max_size=3), st.none(), st.lists(st.integers(), max_size=1), refused()), st.one_of(st.none(), st.integers(-3, 3), floats(), text(max_size=2)))
def test_round_bad(x, nd):
    same("round_number", x, nd)
    same("round_number", 1.5, x)


DIGITS = "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"


@st.composite
def int_texts(draw, base):
    eff = base if base >= 2 else 10
    ws = st.sampled_from(["", "", "", " ", "\t", "\n ", "\xa0", " ", "\x1c", "\x85", "​"])
    out = draw(ws) + draw(st.sampled_from(["", "", "-", "+", "--", "+-"]))
    if draw(st.integers(0, 3)) == 0:
        out += draw(st.sampled_from(["0x", "0X", "0o", "0O", "0b", "0B", "0", "0_x", "0x_"]))
    alphabet = DIGITS[:min(eff, 36)] + DIGITS[10:10 + max(0, min(eff, 36) - 10)].upper()
    chars = draw(st.lists(st.one_of(st.sampled_from(alphabet), st.sampled_from(alphabet), st.just("_"),
                                    st.sampled_from(["٣", "১", "１", "Ⅻ", "z", ".", "e", "²", "𝟗"])),
                          max_size=draw(st.sampled_from([3, 12, 40]))))
    out += "".join(chars) + draw(ws)
    return out


BASES = st.one_of(st.sampled_from([0, 2, 8, 10, 16, 36]), st.integers(2, 36), st.sampled_from([1, 37, -1, -10, 2**64]))


@st.composite
def valid_int_texts(draw):
    base = draw(st.sampled_from([0, 0, 2, 8, 10, 10, 16, 16, 36]) | st.integers(2, 36))
    eff = {0: draw(st.sampled_from([10, 16, 8, 2]))}.get(base, base)
    alphabet = DIGITS[:eff] + DIGITS[10:eff].upper()
    digits = draw(st.lists(st.sampled_from(alphabet), min_size=1, max_size=draw(st.sampled_from([4, 20, 60]))))
    for i in sorted(draw(st.sets(st.integers(1, max(1, len(digits) - 1)), max_size=3)), reverse=True):
        if i < len(digits):
            digits.insert(i, "_")
    body = "".join(digits)
    prefix = ""
    if eff != 10 and (base == 0 or (base == eff and eff in (2, 8, 16) and draw(st.booleans()))):
        prefix = {16: "0x", 8: "0o", 2: "0b"}[eff]
        if draw(st.booleans()):
            prefix = prefix.upper()
        if draw(st.integers(0, 3)) == 0:
            prefix += "_"
    if base == 0 and eff == 10 and body.strip("0_") and body[0] == "0":
        body = "1" + body
    sign = draw(st.sampled_from(["", "", "-", "+"]))
    ws = draw(st.sampled_from(["", "", " ", "\t\n"]))
    return ws + sign + prefix + body + ws, base


@given(valid_int_texts())
def test_parse_int_valid(tb):
    same("parse_int", *tb)


@given(st.data())
def test_parse_int(data):
    base = data.draw(BASES)
    t = data.draw(st.one_of(int_texts(base if base in range(0, 37) else 10), text(max_size=6)))
    same("parse_int", t, base)
    if data.draw(st.booleans()):
        same("parse_int", t.encode("utf-8", "surrogatepass"), base)


@given(st.data())
def test_parse_int_default_base(data):
    t = data.draw(st.one_of(int_texts(10), int_texts(10), text(max_size=5)))
    same("parse_int", t)
    if data.draw(st.booleans()):
        same("parse_int", bytearray(t.encode("utf-8", "surrogatepass")))


@given(st.integers(4295, 4305), st.sampled_from([10, 0, 16, 2, 8, 7]), st.sampled_from(["", "-", " "]))
def test_parse_int_long(n, base, prefix):
    same("parse_int", prefix + "1" * n, base)


@given(st.one_of(ints(), floats(), st.none(), st.booleans(), st.lists(st.integers(), max_size=1), refused()), st.one_of(st.just(10), BASES, st.none(), floats(), text(max_size=2)))
def test_parse_int_bad_types(x, base):
    same("parse_int", x, base)
    same("parse_int", x)


@st.composite
def float_texts(draw):
    ws = st.sampled_from(["", "", "", " ", "\t", "\n", "\xa0", "　"])
    out = draw(ws) + draw(st.sampled_from(["", "", "-", "+", "+-"]))
    kind = draw(st.integers(0, 6))
    if kind == 0:
        out += draw(st.sampled_from(["inf", "INF", "Infinity", "iNfinity", "infinit", "nan", "NaN", "nan(1)", "infinityy", "in", "-nan", "snan"]))
    else:
        dig = st.text(alphabet="0123456789_", max_size=draw(st.sampled_from([3, 20, 30])))
        out += draw(dig)
        if draw(st.booleans()):
            out += "." + draw(dig)
        if draw(st.booleans()):
            out += draw(st.sampled_from(["e", "E"])) + draw(st.sampled_from(["", "+", "-"])) + draw(st.one_of(st.integers(0, 400).map(str), dig))
        if draw(st.integers(0, 6)) == 0:
            out += draw(st.sampled_from(["x", ".", "١", "５", "e", "j", "_"]))
    out += draw(ws)
    return out


@given(st.one_of(float_texts(), float_texts(), text(max_size=6), floats().map(repr), floats().map(lambda f: format(f, ".25g"))))
def test_parse_float(t):
    same("parse_float", t)


@given(float_texts(), st.booleans())
def test_parse_float_bytes(t, arr):
    b = t.encode("utf-8")
    same("parse_float", bytearray(b) if arr else b)


@given(st.one_of(ints(1100), floats(), st.booleans(), st.none(), st.lists(st.integers(), max_size=1), refused()))
def test_parse_float_other_types(x):
    same("parse_float", x)


@given(st.one_of(ints(1100), ints(), st.booleans(),
                 st.integers(0, 1100).flatmap(lambda b: st.integers(-3, 3).map(lambda d: 2**b + d)),
                 st.integers(960, 1030).flatmap(lambda b: st.sampled_from([2**1024 - 2**b, -(2**1024 - 2**b), 2**1024 - 2**b - 1]))))
def test_int_to_float(n):
    same("int_to_float", n)


@given(st.one_of(floats(), text(max_size=5), st.none(), float_texts()))
def test_int_to_float_other(n):
    same("int_to_float", n)


def operands():
    return st.one_of(numbers(), numbers(), st.sampled_from([0, 0.0, -0.0, 1, -1, True, False, float("inf"), float("nan")]),
                     ints(2100))


OPS = ["floor_div", "modulo", "div_mod", "true_div"]


@given(st.sampled_from(OPS), operands(), operands())
def test_arith(op, a, b):
    same(op, a, b)


@given(st.sampled_from(OPS), st.one_of(ints(), floats()), st.sampled_from([0, 0.0, -0.0, False, 1, -1, float("inf"), float("-inf"), float("nan")]))
def test_arith_edge_divisors(op, a, b):
    same(op, a, b)
    same(op, b, a)


@given(st.sampled_from(OPS), st.integers(-2**2200, 2**2200), st.integers(-2**2200, 2**2200) | st.integers(-10, 10))
def test_arith_big_ints(op, a, b):
    same(op, a, b)


@given(st.sampled_from(OPS), st.one_of(scalars(), refused(), st.lists(st.integers(), max_size=2)), st.one_of(scalars(), st.lists(st.integers(), max_size=1).map(tuple)))
def test_arith_other_types(op, a, b):
    same(op, a, b)
