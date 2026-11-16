"""Duration months/years holding values the constructor doesn't normally produce:
string inputs at the Decimal exponent limit, signaling NaNs, and plain int/float
attributes assigned after construction (upstream allows it; %P checks truthiness)."""

from decimal import Decimal

from hypothesis import given, strategies as st

from portwright.compare import same, PythonExpression

from iso_inputs import ISO

EMAX = 999999999999999999


def _is_qnan(v):
    """Quiet-NaN hashes are id-based in Python, so they can't be compared."""
    try:
        return Decimal(v).is_qnan()
    except Exception:
        return False

exp_strings = st.builds(
    lambda coeff, delta, sign, esign: f"{sign}{coeff}e{esign}{EMAX + delta}",
    st.sampled_from(["1", "12", "9", "123456", "0", "00", "1.5", "99"]),
    st.integers(-6, 3),
    st.sampled_from(["", "-", "+"]),
    st.sampled_from(["", "+", "-"]),
)
special_strings = st.sampled_from(
    ["sNaN", "-sNaN", "sNaN12", "NaN", "snan", "Inf", "1", "-2.5", "0", "1e-999999999999999999", "1e-1000000000000000100"]
)
month_strings = st.one_of(exp_strings, special_strings)


@given(month_strings, st.sampled_from(["months", "years"]), st.integers(0, 3))
def test_duration_string_months(s, field, days):
    d = same("Duration", days=days, **{field: s})
    if d is None or isinstance(d, dict):
        return
    d.get(field)
    d.call("__repr__")
    d.call("__str__")
    if not _is_qnan(s):
        d.call("__hash__")
    d.call("__eq__", d)
    d.call("__neg__")
    d.call("__mul__", 2)
    same("duration_isoformat", d)


snan_values = st.sampled_from([Decimal("sNaN"), Decimal("-sNaN"), Decimal("sNaN5"), Decimal("NaN"), Decimal("1")])


@given(snan_values, st.sampled_from(["months", "years"]), st.sampled_from(["P%P", "%y-%m", "%Y %C", "%P"]))
def test_duration_snan(v, field, fmt):
    d = same("Duration", **{field: v})
    if d is None or isinstance(d, dict):
        return
    d.call("__repr__")
    d.call("__str__")
    if not _is_qnan(v):
        d.call("__hash__")
    d.call("__eq__", d)
    d.call("__ne__", d)
    d.call("__neg__")
    d.call("__mul__", 3)
    d.call("__add__", d)
    d.call("__sub__", d)
    d.call("totimedelta", PythonExpression("__import__('datetime').date(2000, 1, 31)"))
    d.call("__add__", PythonExpression("__import__('datetime').date(2000, 1, 31)"))
    same("duration_isoformat", d)
    same("strftime", d, fmt)


attr_values = st.one_of(
    st.integers(-3, 3),
    st.integers(-10**30, 10**30),
    st.sampled_from([0.0, -0.0, 1.5, -2.0, float("nan"), float("inf"), 1e-300, 7.0]),
    st.sampled_from([True, False]),
)


@given(attr_values, attr_values, st.integers(-30, 30),
       st.sampled_from(["%P", "P%P", "%y %Y %C %m", "%P %W %S %M %H", "%d"]))
def test_duration_assigned_attrs(years, months, days, fmt):
    src = (f"(lambda d: (setattr(d, 'years', {years!r}), setattr(d, 'months', {months!r}), d)[2])"
           f"({ISO}.Duration(days={days}))")
    src = src.replace("nan", "float('nan')").replace("inf", "float('inf')")
    d = PythonExpression(src)
    same("strftime", d, fmt)
    same("duration_isoformat", d)
    same("duration_isoformat", d, fmt)
