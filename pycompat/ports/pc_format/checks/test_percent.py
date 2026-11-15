from hypothesis import given, strategies as st
from portwright.compare import same
from portwright.compare.strategies import text

from pcf_strategies import ints, floats, values, refused, safe_sets

KEYS = ["a", "b", "", "x y", "日"]


@st.composite
def directive(draw, mapping, as_bytes=False):
    out = "%"
    if mapping and draw(st.integers(0, 5)) > 0:
        out += "(" + draw(st.sampled_from(KEYS + ["(a)", "a"])) + ")"
    out += "".join(draw(st.lists(st.sampled_from("-+ #0"), max_size=3)))
    out += draw(st.one_of(st.just(""), st.just(""), st.just("*"), st.integers(0, 30).map(str), st.just("9" * 20)))
    if draw(st.booleans()):
        out += "." + draw(st.one_of(st.just(""), st.just("*"), st.integers(0, 25).map(str), st.just("120")))
    out += draw(st.sampled_from(["", "", "", "l", "h", "L"]))
    types = "diouxXeEfFgGcrsa%" + ("b" if as_bytes else "")
    out += draw(st.one_of(st.sampled_from(list(types)), st.sampled_from(["", "z", "y", "b", " ", "日"])))
    return out


@st.composite
def percent_templates(draw, mapping):
    parts = []
    for _ in range(draw(st.integers(0, 4))):
        if draw(st.integers(0, 3)) > 0:
            parts.append(draw(directive(mapping)))
        else:
            parts.append(draw(text(max_size=4)))
    return "".join(parts)


def pvalue():
    return st.one_of(ints(), floats(), text(max_size=6), st.booleans(), st.none(),
                     st.integers(-10, 0x10FFFF + 3), st.text(max_size=2),
                     st.binary(max_size=3), st.lists(st.integers(-3, 3), max_size=2))


@given(st.data())
def test_percent_tuple(data):
    vals = data.draw(st.lists(pvalue(), max_size=5))
    template = data.draw(percent_templates(False))
    same("percent_format", template, tuple(vals))


@given(st.data())
def test_percent_single(data):
    template = data.draw(percent_templates(False))
    same("percent_format", template, data.draw(pvalue()))


@given(st.data())
def test_percent_mapping(data):
    mapping = data.draw(st.dictionaries(st.sampled_from(KEYS), pvalue(), max_size=4))
    template = data.draw(percent_templates(True))
    same("percent_format", template, mapping)


@st.composite
def bytes_template(draw):
    parts = []
    for _ in range(draw(st.integers(0, 3))):
        if draw(st.booleans()):
            parts.append(draw(directive(False, True)))
        else:
            parts.append(draw(st.text(alphabet=st.characters(max_codepoint=127), max_size=3)))
    return "".join(parts).encode("utf-8")


@given(bytes_template(), st.lists(st.one_of(st.binary(max_size=4), st.binary(max_size=3).map(bytearray), ints(), floats(), text(max_size=2), st.integers(0, 300)), max_size=3), st.booleans())
def test_percent_bytes(template, vals, as_array):
    t = bytearray(template) if as_array else template
    same("percent_format", t, tuple(vals))
    if vals:
        same("percent_format", t, vals[0])


@given(st.sampled_from(["%s", "%d", "%f", "%(a)s", "%r", "%c", "%%", "%s %s", "%.3g", "%x", "%-8.3e"]),
       st.one_of(values(), refused(), safe_sets()))
def test_percent_any_value(template, value):
    same("percent_format", template, value)


@given(st.one_of(ints(), st.none(), st.lists(st.integers(), max_size=2)), pvalue())
def test_percent_non_str_template(template, value):
    same("percent_format", template, value)


@given(st.sampled_from(["%c", "%5c", "%-3c", "%d", "%x", "%o", "%#o", "%#X", "%+05d", "% i", "%u", "%e", "%.0f", "%g", "%#.3g"]),
       st.one_of(st.integers(-3, 0x110003), ints(), floats(), text(min_size=0, max_size=2)))
def test_percent_numeric_types(template, value):
    same("percent_format", template, (value,))


@st.composite
def valid_percent(draw):
    """Well-formed template with a matching value per directive."""
    parts, vals = [], []
    for _ in range(draw(st.integers(1, 4))):
        parts.append(draw(text(max_size=3)).replace("%", "%%"))
        conv = draw(st.sampled_from(list("diouxXeEfFgGcrsa%")))
        flags = "".join(draw(st.lists(st.sampled_from("-+ #0"), max_size=2)))
        width = draw(st.sampled_from(["", "", "5", "12", "*"]))
        prec = draw(st.sampled_from(["", "", ".0", ".3", ".10", ".*"]))
        if conv == "%":
            parts.append("%%")
            continue
        parts.append("%" + flags + width + prec + conv)
        if width == "*":
            vals.append(draw(st.integers(-15, 15)))
        if prec == ".*":
            vals.append(draw(st.integers(0, 12)))
        if conv in "diouxX":
            vals.append(draw(st.one_of(ints(), st.booleans(), floats().filter(lambda f: f == f and abs(f) != float("inf")))))
        elif conv in "eEfFgG":
            vals.append(draw(st.one_of(floats(), st.integers(-10**6, 10**6))))
        elif conv == "c":
            vals.append(draw(st.one_of(st.integers(0, 0x10FFFF), st.characters())))
        else:
            vals.append(draw(st.one_of(text(max_size=5), ints(), floats(), st.none(), st.lists(st.integers(-3, 3), max_size=2))))
    return "".join(parts), tuple(vals)


@given(valid_percent())
def test_percent_valid(tv):
    template, vals = tv
    same("percent_format", template, vals)
    if len(vals) == 1:
        same("percent_format", template, vals[0])
