from hypothesis import given, strategies as st
from portwright.compare import same
from portwright.compare.strategies import text

from pcstrats import scalars, any_value, ints, floats, foreign

TYPES = list("sradiuxXoeEfFgGc%")


@st.composite
def directive(draw, mapping):
    out = "%"
    if mapping:
        out += "(" + draw(st.sampled_from(["a", "b", "c", "", "é", "a)b", "missing"])) + ")"
    out += "".join(draw(st.lists(st.sampled_from(["-", "+", " ", "#", "0"]), max_size=3)))
    w = draw(st.sampled_from(["", "", "", "1", "5", "10", "*", "0", "20"]))
    out += w
    if draw(st.integers(0, 2)) == 0:
        out += "." + draw(st.sampled_from(["", "0", "1", "2", "3", "6", "10", "*", "17", "30"]))
    out += draw(st.sampled_from(["", "", "", "", "l", "h", "L"]))
    out += draw(st.sampled_from(TYPES))
    return out


def literal():
    return st.one_of(st.sampled_from(["", " ", "x", "%%", "é", "\n"]),
                     text(max_size=3).map(lambda s: s.replace("%", "%%")))


@st.composite
def templates(draw, mapping=False):
    out = [draw(literal())]
    for _ in range(draw(st.integers(1, 3))):
        out.append(draw(directive(mapping)))
        out.append(draw(literal()))
    return "".join(out)


def pvalue():
    return st.one_of(ints(), floats(), st.booleans(), text(max_size=6), st.none(),
                     st.integers(-3, 0x110003), st.binary(max_size=3))


@given(templates(), st.lists(pvalue(), max_size=4).map(tuple))
def test_percent_tuple(template, values):
    same("percent_format", template, values)


@given(st.integers(0, 3).flatmap(lambda n: st.tuples(
    st.lists(st.sampled_from(TYPES[:-1]), min_size=n, max_size=n),
    st.lists(st.sampled_from(["", "5", "-8", "05", "+", " ", "#", ".2", "10.3", "+.0", "#.0", "-*", "*.*"]), min_size=n, max_size=n),
)), st.data())
def test_percent_matched_tuple(spec, data):
    kinds, flags = spec
    template = " ".join("%" + f + k for f, k in zip(flags, kinds))
    values = []
    for f, k in zip(flags, kinds):
        values += [data.draw(st.integers(-30, 30))] * f.count("*")
        if k in "diuxXoc":
            values.append(data.draw(st.one_of(ints(), st.integers(0, 0x10FFFF), st.booleans(), floats())))
        elif k in "eEfFgG":
            values.append(data.draw(st.one_of(floats(), ints(), st.booleans())))
        else:
            values.append(data.draw(pvalue()))
    same("percent_format", template, tuple(values))


@given(templates(), pvalue())
def test_percent_single(template, value):
    same("percent_format", template, value)


@given(st.sampled_from(["%s", "%r", "%a", "%d", "%x", "%f", "%.3g", "%10.4e", "%c", "%5s", "%-7r", "%+d", "%#o"]),
       pvalue())
def test_percent_one_directive(template, value):
    same("percent_format", template, value)
    same("percent_format", template, (value,))


@given(templates(mapping=True),
       st.dictionaries(st.sampled_from(["a", "b", "c", "", "é", "a)b"]), pvalue(), max_size=5))
def test_percent_mapping(template, mapping):
    same("percent_format", template, mapping)


@given(st.lists(st.sampled_from(list("%()abs drf.*-+ #0123x%c")), max_size=10).map("".join),
       st.one_of(st.lists(pvalue(), max_size=3).map(tuple), pvalue(),
                 st.dictionaries(st.sampled_from(["a", "b", ""]), pvalue(), max_size=2)))
def test_percent_fragments(template, values):
    same("percent_format", template, values)


@given(text(max_size=10), st.one_of(st.just(()), pvalue(), any_value()))
def test_percent_any(template, values):
    same("percent_format", template, values)


@given(st.one_of(st.binary(max_size=6), st.binary(max_size=6).map(bytearray), ints(), st.none(), foreign(), floats()),
       st.one_of(st.just(()), pvalue(), st.lists(pvalue(), max_size=2).map(tuple), foreign()))
def test_percent_bad_types(template, values):
    same("percent_format", template, values)


@given(st.sampled_from([b"%s", b"%d", b"%b", b"%x %r", b"%a", b"%c", b"%(a)s", bytearray(b"%s-%d")]),
       st.one_of(st.binary(max_size=3), ints(), st.lists(st.one_of(st.binary(max_size=2), ints()), max_size=2).map(tuple)))
def test_percent_bytes_templates(template, values):
    same("percent_format", template, values)
