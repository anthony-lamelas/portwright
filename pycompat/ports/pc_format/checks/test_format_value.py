from hypothesis import given, strategies as st
from portwright.compare import same
from portwright.compare.strategies import text

from pcstrats import floats, ints, format_specs, near_miss_specs, scalars, any_value, foreign


@given(ints(), format_specs("bcdeEfFgGnosxX%"))
def test_format_int(value, spec):
    same("format_value", value, spec)


@given(floats(), format_specs("eEfFgGn%"))
def test_format_float(value, spec):
    same("format_value", value, spec)


@given(st.booleans(), format_specs())
def test_format_bool(value, spec):
    same("format_value", value, spec)


@given(text(max_size=10), format_specs("s"))
def test_format_str(value, spec):
    same("format_value", value, spec)


@given(st.integers(-5, 0x110005), st.sampled_from(["c", "5c", "<3c", "^5c", "*>4c", "", "#x"]))
def test_format_char(value, spec):
    same("format_value", value, spec)


@given(scalars(), near_miss_specs())
def test_format_near_miss(value, spec):
    same("format_value", value, spec)


@given(st.one_of(any_value(), foreign()), st.sampled_from(["", "s", "5", "d", "x"]))
def test_format_other_types(value, spec):
    same("format_value", value, spec)


@given(scalars(), st.one_of(st.none(), st.integers(-3, 3), st.binary(max_size=3)))
def test_format_spec_wrong_type(value, spec):
    same("format_value", value, spec)


@given(st.one_of(ints(), floats()), st.sampled_from(["", "f", "e", "g", "%", ",", "n"]),
       st.integers(0, 120))
def test_format_precision_range(value, kind, precision):
    if kind in "f%" and isinstance(value, float) and abs(value) > 1e60:
        precision = min(precision, 30)
    same("format_value", value, f".{precision}{kind}" if kind != "," else f",.{precision}")
