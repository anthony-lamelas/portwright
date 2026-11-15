from hypothesis import given, strategies as st
from portwright.compare import same
from portwright.compare.strategies import text

from pcf_strategies import values, format_specs, ints, floats, refused, safe_sets

NAMES = ["a", "b", "x_1", "日本", ""]


@st.composite
def field_names(draw, nargs):
    kind = draw(st.sampled_from(["auto", "auto", "index", "name", "bad"]))
    if kind == "auto":
        name = ""
    elif kind == "index":
        name = str(draw(st.integers(0, nargs + 1)))
    elif kind == "name":
        name = draw(st.sampled_from(NAMES[:4]))
    else:
        name = draw(st.sampled_from(["-1", " 0", "0 ", "00", "1a", "a b", "99999999999999999999", "\udc80", "é"]))
    for _ in range(draw(st.integers(0, 2))):
        acc = draw(st.sampled_from(["idx", "idx", "key", "attr", "badidx"]))
        if acc == "idx":
            name += "[" + str(draw(st.integers(0, 3))) + "]"
        elif acc == "key":
            name += "[" + draw(st.sampled_from(["a", "b", "k", "-1", " ", "x y", "0x"])) + "]"
        elif acc == "attr":
            name += "." + draw(st.sampled_from(["real", "imag", "upper", "", "__class__"]))
        else:
            name += draw(st.sampled_from(["[", "[]", "]", "[0]x", "[0", "!"]))
    return name


@st.composite
def fields(draw, nargs, depth=0):
    out = "{" + draw(field_names(nargs))
    out += draw(st.sampled_from(["", "", "", "!r", "!s", "!a", "!", "!z", "!rs"]))
    if draw(st.booleans()):
        out += ":"
        if depth < 2 and draw(st.integers(0, 3)) == 0:
            pre = draw(st.sampled_from(["", ">", "*^", "0", "+"]))
            post = draw(st.sampled_from(["", "d", ".2f", "s", "x"]))
            out += pre + draw(fields(nargs, depth + 1)) + post
            if draw(st.booleans()):
                out += "." + draw(fields(nargs, depth + 1))
        else:
            spec = draw(format_specs())
            out += spec.replace("{", "").replace("}", "")
    out += draw(st.sampled_from(["}", "}", "}", "}", "", "}}"]))
    return out


@st.composite
def templates(draw, nargs):
    parts = []
    for _ in range(draw(st.integers(0, 4))):
        choice = draw(st.integers(0, 5))
        if choice <= 2:
            parts.append(draw(fields(nargs)))
        elif choice == 3:
            parts.append(draw(text(max_size=5)).replace("{", "{{").replace("}", "}}"))
        elif choice == 4:
            parts.append(draw(st.sampled_from(["{{", "}}", "{", "}", "{}", "x"])))
        else:
            parts.append(draw(text(max_size=4)))
    return "".join(parts)


def arg_values():
    return st.one_of(ints(), floats(), text(max_size=6), st.booleans(), st.none(),
                     st.lists(st.one_of(st.integers(-9, 9), text(max_size=3)), max_size=4),
                     st.dictionaries(st.sampled_from(["a", "b", "k", "0", 0, 1, -1]), st.integers(-9, 99), max_size=3),
                     st.lists(st.integers(-9, 9), max_size=4).map(tuple))


@st.composite
def format_call(draw):
    args = draw(st.lists(arg_values(), max_size=4))
    kwargs = draw(st.dictionaries(st.sampled_from(NAMES), arg_values(), max_size=3))
    template = draw(templates(len(args)))
    return template, args, kwargs


@given(format_call(), st.booleans())
def test_str_format(call, as_tuple):
    template, args, kwargs = call
    same("str_format", template, tuple(args) if as_tuple else args, kwargs)


@given(st.lists(arg_values(), max_size=3).map(tuple), st.data())
def test_str_format_defaults(args, data):
    template = data.draw(templates(len(args)))
    same("str_format", template, args)
    if not args:
        same("str_format", template)


@given(st.sampled_from(["{0.real}", "{.imag}", "{0:{1.real}}", "{0.real}{", "{0}{1.imag", "{a.b}", "{0[0].x}",
                        "{0:{1:{2.real}}}", "{0:>{1}}{2.real}", "{0..x}", "{0.}", "{0:{0.real}{}}", "{0[.]}",
                        "x{!r:{.real}}", "{0!r:{1}}", "{:{}.{}f}", "{0:{1}{}"]),
       st.lists(arg_values(), max_size=4))
def test_str_format_attributes(template, args):
    same("str_format", template, args, {})


@given(st.one_of(st.binary(max_size=4), ints(), st.none(), st.lists(st.integers(), max_size=2)), st.lists(st.integers(), max_size=2))
def test_str_format_non_str_template(template, args):
    same("str_format", template, args)


@given(st.one_of(st.integers(), text(max_size=3), st.none()), st.one_of(st.integers(), st.none(), st.lists(st.integers(), max_size=1)))
def test_str_format_bad_arg_types(args, kwargs):
    same("str_format", "{}", args, kwargs)


@given(st.dictionaries(st.one_of(text(max_size=4), st.integers()), arg_values(), max_size=3), st.sampled_from(["{a}", "{0}", "{}", "x", "{\udc80}", "{ }"]))
def test_str_format_odd_kwargs(kwargs, template):
    same("str_format", template, [], kwargs)


@given(st.one_of(refused(), safe_sets()), st.sampled_from(["{}", "{!r}", "{!s:>5}"]))
def test_str_format_refused_and_sets(value, template):
    same("str_format", template, (value,))
    same("str_format", template, [], {"a": value})


@st.composite
def valid_format(draw):
    """Well-formed template using auto-numbering, a value matched to each spec."""
    parts, args = [], []
    for _ in range(draw(st.integers(1, 4))):
        parts.append(draw(text(max_size=3)).replace("{", "{{").replace("}", "}}"))
        kind = draw(st.sampled_from(["int", "float", "str"]))
        if kind == "int":
            spec = draw(format_specs(list("bdoxXn") + ["", "e", "%"], valid=True))
            args.append(draw(ints()))
        elif kind == "float":
            spec = draw(format_specs(list("eEfFgG%") + [""], valid=True))
            args.append(draw(floats()))
        else:
            spec = draw(format_specs(["", "s"]))
            args.append(draw(text(max_size=6)))
        spec = spec.replace("{", "").replace("}", "")
        conv = draw(st.sampled_from(["", "", "", "!r", "!s", "!a"]))
        if conv and kind != "str":
            spec = draw(format_specs(["", "s"])).replace("{", "").replace("}", "")
        nested = draw(st.integers(0, 4)) == 0
        if nested:
            args.append(draw(st.integers(0, 20)))
            parts.append("{" + conv + ":>{}}")
        else:
            parts.append("{" + conv + (":" + spec if spec or draw(st.booleans()) else "") + "}")
    return "".join(parts), args


@given(valid_format())
def test_str_format_valid(ta):
    template, args = ta
    same("str_format", template, args)
