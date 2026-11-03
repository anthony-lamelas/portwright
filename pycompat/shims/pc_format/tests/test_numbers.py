"""repr, str, round, int(), float() and division, compared with the real built-ins."""

import itertools
import sys

import pc_format as pc

INF = float("inf")
NAN = float("nan")


def outcome(func, *args):
    try:
        value = func(*args)
    except Exception as exc:
        return ("raised", type(exc).__name__, str(exc))
    return ("returned", type(value).__name__, repr(value))


def same(port_func, builtin, *args):
    assert outcome(port_func, *args) == outcome(builtin, *args), args


FLOATS = [0.0, -0.0, 1.0, -1.0, 0.1, 0.2, 0.30000000000000004, 1 / 3, 1e16, 1e15, 123456789012345680.0, 1e-4, 1e-5,
          5e-324, 2.2250738585072014e-308, 1.7976931348623157e308, 1e22, 1e23, 9007199254740993.0, 0.5, 2.5, 3.5,
          -2.5, 1.005, 2.675, 0.045, 1234.5678, INF, -INF, NAN, 100.0, 1e300, 4.35, 0.285, 1.15, 2 ** -1074]
INTS = [0, 1, -1, 5, 15, 25, -25, 35, 45, 123456, -123456, 2**31, 2**53 + 1, 2**63, -(2**64), 10**20 + 5, 10**30,
        3**100, -(7**80)]


def test_float_repr():
    for value in FLOATS:
        same(pc.float_repr, repr, value)
    for exponent in range(-330, 310, 7):
        for mantissa in (1.0, 1.5, 9.87654321, 3.0000000000000004):
            same(pc.float_repr, repr, mantissa * 10.0**exponent if -308 < exponent < 308 else mantissa * 1e-300)


def test_int_str_and_its_digit_limit():
    for value in INTS + [10**4299, 10**4299 * 9, -(10**4299)]:
        same(pc.int_str, str, value)
    same(pc.int_str, str, 10**4300)
    same(pc.int_str, str, -(10**5000))
    assert sys.get_int_max_str_digits() == 4300


def test_round_floats():
    for value in FLOATS:
        same(pc.round_number, round, value)
        for ndigits in (-400, -308, -20, -3, -1, 0, 1, 2, 3, 10, 17, 20, 300, 400):
            same(pc.round_number, round, value, ndigits)


def test_round_ints():
    for value in INTS:
        same(pc.round_number, round, value)
        for ndigits in (-50, -20, -2, -1, 0, 1, 5):
            same(pc.round_number, round, value, ndigits)


def test_round_argument_errors():
    for value, ndigits in [(1.5, 1.0), (1, "x"), (1.5, None), (True, 1), (2, True), (2.5, 2**70), (2.5, -(2**70))]:
        same(pc.round_number, lambda v, n: round(v, n), value, ndigits)


def test_parse_int_base_10():
    texts = ["0", "-0", "+5", "  42  ", "\t7\n", "1_000", "1__0", "_1", "1_", "", " ", "-", "12a", "1.0", "1e3",
             "٣", "١٢٣", "০১", "Ⅻ", "１２", "0x10", "00", "007", "9" * 4300, "9" * 4301, " 12", "1 ",
             "+-1", "--1", "1 2", "0_0", "inf", "nan"]
    for text in texts:
        same(pc.parse_int, int, text)


def test_parse_int_other_bases():
    texts = ["0", "10", "ff", "FF", "-ff", "0x1f", "0X1F", "0o17", "0b101", "0b", "0x", "z", "Z", "zz", "1_f", "0x_1",
             "0_x1", "  0b1_1  ", "12", "777", "8", "g", "0", "00", "0b0", "٣"]
    for text, base in itertools.product(texts, [0, 2, 8, 10, 16, 36, 37, 1, -1]):
        same(pc.parse_int, int, text, base)


def test_parse_float():
    texts = ["0", "-0", "1.5", " 1.5 ", "1e5", "1E-5", ".5", "5.", ".", "e5", "1e", "1e+", "inf", "-inf", "+INF",
             "infinity", "-Infinity", "iNfInItY", "nan", "-nan", "NaN", "nan(1)", "1_000.5", "1__0", "_1", "1_",
             "1_.5", "1._5", "0x10", "١٢.٥", "１.５", "1e400", "-1e400", "1e-400", "4.9e-324", "2.4703282292062327e-324",
             "", " ", "1.5.2", "+-1", "\t-3.25\n", "1e5_0", "1_0e1_0", "0.1e-1_0", "9" * 400 + ".5",
             "0." + "0" * 400 + "1", "2.2250738585072011e-308", "1.7976931348623158e308", "1.7976931348623159e308"]
    for text in texts:
        same(pc.parse_float, float, text)


def test_int_to_float():
    for value in INTS + [2**1023, 2**1024, -(2**1024), 2**1024 - 2**970, 2**1024 - 2**971, 2**53 + 1, 2**54 + 3]:
        same(pc.int_to_float, float, value)


def _pairs():
    numbers = [0, 1, -1, 3, -3, 7, -7, 10**20, -(10**20) - 1, 2**64 + 3, 0.0, -0.0, 0.5, -0.5, 2.5, -7.25, 1e300,
               -1e-300, INF, -INF, NAN, 3.0, True, False]
    return itertools.product(numbers, numbers)


def test_floor_div():
    for a, b in _pairs():
        same(pc.floor_div, lambda x, y: x // y, a, b)


def test_modulo():
    for a, b in _pairs():
        same(pc.modulo, lambda x, y: x % y, a, b)


def test_div_mod():
    for a, b in _pairs():
        same(pc.div_mod, divmod, a, b)


def test_true_div():
    for a, b in _pairs():
        same(pc.true_div, lambda x, y: x / y, a, b)
    for a, b in [(10**400, 10**399), (2**2000, 3), (1, 10**400), (10**400, 7), (-(3**1000), 3**999), (2**1100, 2**75)]:
        same(pc.true_div, lambda x, y: x / y, a, b)
