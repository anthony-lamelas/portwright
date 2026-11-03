"""Targeted rules: str alignment, tiny int quotients, %e precision memory band."""
from hypothesis import given, strategies as st
from portwright.compare import same, PythonExpression
from portwright.compare.strategies import text

from pcstrats import FILL_CHARS, floats


@st.composite
def valid_str_specs(draw):
    """Specs str.__format__ accepts: [[fill]align][0][width][.precision][s]."""
    spec = ""
    align = draw(st.sampled_from(["", "<", ">", "^"]))
    if align:
        spec += draw(st.sampled_from(FILL_CHARS)) + align
    spec += draw(st.sampled_from(["", "", "0"]))
    spec += draw(st.sampled_from(["", "1", "2", "3", "4", "5", "7", "8", "11", "16", "25"]))
    if draw(st.integers(0, 2)) == 0:
        spec += "." + draw(st.sampled_from(["0", "1", "2", "3", "5", "9"]))
    spec += draw(st.sampled_from(["", "s"]))
    return spec


@given(text(max_size=12), valid_str_specs())
def test_format_str_valid_specs(value, spec):
    same("format_value", value, spec)


@given(text(min_size=0, max_size=9), st.sampled_from(["", "*", "\U0001f600", "\ud800", "0"]),
       st.sampled_from(["<", ">", "^"]), st.integers(0, 30))
def test_format_str_alignment(value, fill, align, width):
    same("format_value", value, f"{fill}{align}{width}")
    same("format_value", value, f"{fill}{align}{width}.2s")


@given(st.integers(1, 2 ** 120), st.integers(1, 2 ** 120), st.integers(900, 1140), st.booleans())
def test_true_div_tiny_quotients(a, c, k, neg):
    # |a / (c * 2**k)| lands around 2**-900 .. 2**-1260: subnormals, deep scaling, underflow.
    b = c * 2 ** k
    if neg:
        a = -a
    same("true_div", a, b)
    same("true_div", a, b | 1)
    same("true_div", 1, 2 ** k)


@given(st.integers(524_000_000, 536_800_000), st.sampled_from(["e", "E"]),
       st.one_of(floats(), st.sampled_from([0.0, 1.5, -2.0, 1e300, 5e-324])),
       st.sampled_from(["", "#", "+", "-", "0"]))
def test_percent_e_memory_band(prec, kind, x, flag):
    # CPython 3.12 under the 1 GiB worker limit raises MemoryError here at once (its threshold
    # is ~522.9M in a fresh process and only lower after other allocations; never pick below it,
    # or CPython builds a 500 MB string). The port's 'e' memory arm must also refuse up to 2**29.
    same("percent_format", f"%{flag}.{prec}{kind}", x)
    same("percent_format", f"%{flag}.*{kind}", (prec, x))
