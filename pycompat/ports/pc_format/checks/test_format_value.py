from hypothesis import given, settings, strategies as st
from portwright.compare import same
from portwright.compare.strategies import text

from pcf_strategies import (ints, floats, format_specs, any_spec, values, refused, safe_sets,
                            TYPES_INT, TYPES_FLOAT)


@given(ints(), format_specs(TYPES_INT + ["e", "f", "g", "%", "s"]))
def test_format_int(value, spec):
    same("format_value", value, spec)


@given(floats(), format_specs(TYPES_FLOAT + ["d", "x", "s"]))
def test_format_float(value, spec):
    same("format_value", value, spec)


@given(text(max_size=20), format_specs(["", "s", "s", "d", "x", "f"]))
def test_format_str(value, spec):
    same("format_value", value, spec)


@given(st.booleans(), format_specs())
def test_format_bool(value, spec):
    same("format_value", value, spec)


@given(st.one_of(ints(), floats(), text(max_size=8), st.booleans()), any_spec())
def test_format_any_spec(value, spec):
    same("format_value", value, spec)


@given(st.one_of(values(), safe_sets()), st.sampled_from(["", "", "s", "5", "d", ">3"]))
def test_format_other_values(value, spec):
    same("format_value", value, spec)


@given(refused(), st.sampled_from(["", "s"]))
def test_format_refused(value, spec):
    same("format_value", value, spec)


@given(st.one_of(ints(), floats(), text(max_size=5)), st.one_of(st.none(), st.integers(), st.binary(max_size=3)))
def test_format_bad_spec_type(value, spec):
    same("format_value", value, spec)


@given(st.integers(0, 0x10FFFF + 5) | st.integers(-5, 5), st.sampled_from(["c", "5c", "<3c", "^4c", "*>2c", "+c", "#c", ",c", "05c"]))
def test_format_char(value, spec):
    same("format_value", value, spec)


@given(st.sampled_from([10**4299, 10**4300, -10**4300, 2**20000, 10**5000]),
       st.sampled_from(["", "d", "x", "b", "o", ",", "_x", "n", ">10", "e", "f", ".3g"]))
def test_format_huge_int(value, spec):
    same("format_value", value, spec)


def test_format_defaults_examples():
    for v, s in [(1234567, ","), (1234567.891, "_.2f"), (-0.0, "z.1f"), (-0.001, "z.1f"), (0.5, ".0f"),
                 (1e16, ""), (1e16, "n"), (255, "#010x"), (-255, "=+10_b"), ("abc", "^9.2"), (True, "")]:
        same("format_value", v, s)


@given(ints(), format_specs(list("bdoxXn") + ["", "c"], valid=True))
def test_format_int_valid(value, spec):
    same("format_value", value, spec)


@given(floats(), format_specs(TYPES_FLOAT, valid=True))
def test_format_float_valid(value, spec):
    same("format_value", value, spec)
