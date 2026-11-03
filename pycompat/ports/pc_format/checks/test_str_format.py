from hypothesis import given, strategies as st
from portwright.compare import same
from portwright.compare.strategies import text

from pcstrats import scalars, any_value, format_specs, near_miss_specs, foreign, ints, floats

KEYS = ["a", "b", "k", "é", "日本", "x_1"]


def literal():
    return st.one_of(
        st.sampled_from(["", " ", "x", "{{", "}}", "é", ":", "!", "[", "]", "\n"]),
        text(max_size=4).map(lambda s: s.replace("{", "{{").replace("}", "}}")),
    )


@st.composite
def field(draw, auto):
    name = "" if auto else draw(st.sampled_from(["0", "1", "2", "a", "b", "k", "é", "日本", "0", "1"]))
    if draw(st.integers(0, 4)) == 0:
        name += draw(st.sampled_from(["[0]", "[1]", "[a]", "[b]", "[k]", "[0][0]", "[-1]", "[10]", "[]", "[x y]"]))
    conv = draw(st.sampled_from(["", "", "", "!r", "!s", "!a"]))
    spec = ""
    roll = draw(st.integers(0, 5))
    if roll == 1:
        spec = ":" + draw(format_specs())
    elif roll == 2:
        spec = ":" + draw(st.sampled_from([">5", "<4", "^6", "*^7", "5", ".2", "s", "d", ".1f", ",", "x"]))
    elif roll == 3:
        inner = "{}" if auto else "{" + draw(st.sampled_from(["1", "2", "a", "b"])) + "}"
        spec = ":" + draw(st.sampled_from([">", "<", "", "^"])) + inner
    return "{" + name + conv + spec + "}"


@st.composite
def templates(draw):
    auto = draw(st.booleans())
    out = [draw(literal())]
    for _ in range(draw(st.integers(0, 4))):
        out.append(draw(field(auto)))
        out.append(draw(literal()))
    return "".join(out)


def args_list():
    return st.lists(st.one_of(scalars(), any_value()), max_size=4).flatmap(
        lambda xs: st.sampled_from([tuple(xs), list(xs)]))


def kwargs_dict():
    return st.dictionaries(st.sampled_from(KEYS), st.one_of(scalars(), any_value()), max_size=4)


@given(templates(), args_list(), kwargs_dict())
def test_str_format_structured(template, args, kwargs):
    same("str_format", template, args, kwargs)


@given(templates(), st.lists(st.one_of(ints(), floats(), st.text(max_size=4)), min_size=3, max_size=4).map(tuple),
       st.fixed_dictionaries({"a": st.one_of(ints(), text(max_size=3)), "b": floats(), "k": st.just({"a": 1, 0: "z"})}))
def test_str_format_mostly_valid(template, args, kwargs):
    same("str_format", template, args, kwargs)


@given(scalars(), format_specs())
def test_str_format_single_spec(value, spec):
    same("str_format", "{0:" + spec + "}", (value,))
    same("str_format", "{:" + spec + "}", args=[value])


@given(scalars(), near_miss_specs(), st.sampled_from(["!r", "!s", "!a", ""]))
def test_str_format_nested_spec(value, spec, conv):
    same("str_format", "{0" + conv + ":{1}}", (value, spec))
    same("str_format", "{v" + conv + ":{s}}", kwargs={"v": value, "s": spec})


@given(text(max_size=12))
def test_str_format_any_template(template):
    same("str_format", template)
    same("str_format", template, (1, "x"), {"a": 2})


@given(st.lists(st.sampled_from(list("{}0123!rsa:.[]<>^ x_é") + ["{0", "0}", "{a.b}", "{0.real}", "{0[0].x}", "{:{0.x}}", "{0:{0.real}}", "{0:{1:{0.x}}}", "{0[0]:{a.b}}", "{0.real}{", "{0!r.x}", "{a[0].imag}", "{0.}", "{.x}", "{0:>{1}.{a.x}}"]), max_size=8).map("".join),
       args_list(), kwargs_dict())
def test_str_format_fragments(template, args, kwargs):
    same("str_format", template, args, kwargs)


@given(st.one_of(st.binary(max_size=4), st.none(), ints(), st.lists(st.text(max_size=2), max_size=2), foreign()),
       st.one_of(st.just(()), args_list(), foreign(), st.none()), st.one_of(st.none(), kwargs_dict(), foreign()))
def test_str_format_bad_types(template, args, kwargs):
    same("str_format", template, args, kwargs)


@given(st.sampled_from(["{}", "{0}", "{a}", "{0}{1}", "{a}{b}"]),
       st.one_of(st.just(()), st.text(max_size=3), st.dictionaries(st.text(max_size=2), ints(), max_size=2),
                 st.sets(ints(), max_size=2)),
       st.one_of(st.none(), st.dictionaries(st.one_of(ints(), st.sampled_from(KEYS)), ints(), max_size=2),
                 st.lists(st.tuples(st.text(max_size=1), ints()), max_size=2)))
def test_str_format_arg_shapes(template, args, kwargs):
    same("str_format", template, args, kwargs)
    same("str_format", template, args)
    same("str_format", template, kwargs=kwargs)
