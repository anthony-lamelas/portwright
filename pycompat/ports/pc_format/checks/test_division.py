from hypothesis import given, strategies as st
from portwright.compare import same

from pcstrats import ints, floats, numbers, any_value, foreign
from portwright.compare.strategies import text

OPS = ["floor_div", "modulo", "div_mod", "true_div"]
SMALL = st.one_of(st.integers(-10, 10), st.sampled_from([0.0, -0.0, 0.5, -0.5, 3.0, -2.5, float("inf"),
                                                          -float("inf"), float("nan"), True, False, 1e-320, 1e308]))


@given(st.sampled_from(OPS), numbers(), numbers())
def test_div_numbers(op, a, b):
    same(op, a, b)


@given(st.sampled_from(OPS), ints(), ints())
def test_div_ints(op, a, b):
    same(op, a, b)


@given(st.sampled_from(OPS), floats(), floats())
def test_div_floats(op, a, b):
    same(op, a, b)


@given(st.sampled_from(OPS), st.one_of(numbers(), SMALL), SMALL)
def test_div_small_divisors(op, a, b):
    same(op, a, b)


@given(st.sampled_from(OPS), st.integers(-10 ** 6, 10 ** 6), st.integers(-1000, 1000))
def test_div_small_ints(op, a, b):
    same(op, a, b)
    same(op, float(a), b)
    same(op, a, float(b))


@given(st.integers(-10 ** 500, 10 ** 500), st.integers(-10 ** 500, 10 ** 500))
def test_true_div_huge(a, b):
    same("true_div", a, b)
    same("true_div", a * 2 ** 1100, b | 1)
    same("true_div", a, (b | 1) * 2 ** 1100)


@given(st.sampled_from(OPS), st.integers(-10 ** 400, 10 ** 400), floats())
def test_div_huge_int_float(op, a, b):
    same(op, a, b)
    same(op, b, a)


@given(st.sampled_from(OPS), st.one_of(numbers(), text(max_size=3), st.none(), any_value(), foreign()),
       st.one_of(numbers(), text(max_size=3), st.none(), any_value(), foreign()))
def test_div_other_types(op, a, b):
    same(op, a, b)


@given(st.integers(4300, 5200), st.integers(-9, 9), st.integers(1, 999))
def test_div_beyond_str_limit(digits, k, small):
    from portwright.compare import PythonExpression
    big = PythonExpression(f"{k} * 10 ** {digits} + {small}")
    near = PythonExpression(f"{small} * 10 ** {digits - 5}")
    same("true_div", big, near)
    same("modulo", big, small)
    same("div_mod", big, near)
    same("floor_div", big, near)
    same("true_div", small, big)
    same("modulo", big, float(small))
    same("int_to_float", big)
