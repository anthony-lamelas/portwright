"""Review findings: upstream's `_builtins_only` wrapper checks every positional value and
then every keyword value (in passed order) *before* Python binds parameters, so the
builtin-type TypeError beats arity / unexpected-keyword / multiple-values errors.
Also: CPython's arity wording ("1 positional argument" singular), round ndigits
non-int error paths, and deep nesting (RecursionError)."""
from decimal import Decimal

from hypothesis import example, given, settings, strategies as st
from portwright.compare import same, PythonExpression
from portwright.compare.strategies import text

from pcf_strategies import values, REFUSED

D = Decimal("1.5")
OBJ = PythonExpression("object()")

PARAMS = {
    "format_value": ["value", "spec"],
    "str_format": ["template", "args", "kwargs"],
    "percent_format": ["template", "values"],
    "float_repr": ["x"],
    "int_str": ["n"],
    "round_number": ["x", "ndigits"],
    "parse_int": ["text", "base"],
    "parse_float": ["text"],
    "int_to_float": ["n"],
    "floor_div": ["a", "b"],
    "modulo": ["a", "b"],
    "div_mod": ["a", "b"],
    "true_div": ["a", "b"],
}
BOGUS = ["foo", "x", "n", "a", "args", "kwargs", "value", "self", "spec2"]

GOOD = {
    "value": 1.5, "spec": ".2f", "template": "{}%s", "args": (1,), "kwargs": {"a": 1},
    "values": (1,), "x": 2.5, "n": 7, "ndigits": 1, "text": "12", "base": 10, "a": 7, "b": 2,
}


def arg_value():
    return st.one_of(
        values(1), values(1),
        st.sampled_from([D, OBJ, PythonExpression("1j"), PythonExpression("type")] + REFUSED),
        st.sampled_from(list(GOOD.values())),
        text(max_size=5),
    )


# ---------- reviewer repros ----------

def test_review_repros():
    same("str_format", args=(D,), template=OBJ)
    same("str_format", template=OBJ, args=(D,))
    same("str_format", args=(D,))
    same("str_format", args=(1,))
    same("str_format", kwargs={"a": D})
    same("parse_int", "1", foo=D)
    same("parse_int", "1", foo=1)
    same("parse_int", "1", base=D, foo=OBJ)
    same("parse_int", "1", foo=OBJ, base=D)
    same("float_repr", 1, D)
    same("float_repr", 1, 2)
    same("float_repr", 1.0, 2.0, 3.0)
    same("float_repr")
    same("float_repr", x=1.5)
    same("float_repr", 1.5, x=D)
    same("float_repr", 1.5, x=2.5)
    same("round_number", 1.5, ndigits=D, x=2)
    same("round_number", 1.5, ndigits=1, x=2)
    same("round_number", 1.5, 1, 2)
    same("round_number", 1.5, 1, D)
    same("round_number")
    same("round_number", ndigits=1)
    same("int_str", 1, 2)
    same("int_str", n=1, m=2)
    same("int_str", m=D)
    same("format_value", 1)
    same("format_value", 1, "", "")
    same("format_value", 1, "", D)
    same("format_value", spec="", value=3)
    same("format_value", spec=D, value=OBJ)
    same("div_mod", 1)
    same("div_mod", 1, 2, 3)
    same("div_mod", 1, b=2, a=D)
    same("true_div", b=0, a=1)
    same("percent_format", "%s")
    same("percent_format", values=1, template="%s")
    same("parse_float")
    same("parse_float", "1", "2")
    same("int_to_float", n=3)
    same("modulo", 1, 2, c=D)
    same("floor_div", D, 1, c=OBJ)


# ---------- generated call shapes ----------

@st.composite
def call_shapes(draw):
    target = draw(st.sampled_from(sorted(PARAMS)))
    params = PARAMS[target]
    npos = draw(st.integers(0, len(params) + 1))
    pos = []
    for i in range(npos):
        if i < len(params) and draw(st.booleans()):
            pos.append(GOOD[params[i]])
        else:
            pos.append(draw(arg_value()))
    names = draw(st.lists(st.sampled_from(params + BOGUS), unique=True, max_size=3))
    kw = {}
    for name in names:
        kw[name] = GOOD[name] if name in GOOD and draw(st.booleans()) else draw(arg_value())
    return target, pos, kw


@settings(max_examples=400)
@given(call_shapes())
@example(("str_format", [], {"args": (D,), "template": OBJ}))
@example(("str_format", [], {"args": (D,)}))
@example(("parse_int", ["1"], {"foo": D}))
@example(("float_repr", [1, D], {}))
@example(("float_repr", [1, 2], {}))
@example(("round_number", [1.5], {"ndigits": D, "x": 2}))
def test_call_shapes(shape):
    target, pos, kw = shape
    same(target, *pos, **kw)


@settings(max_examples=200)
@given(st.sampled_from(sorted(PARAMS)), st.integers(0, 4))
def test_arity_wording(target, n):
    # All-good values so only arity matters; covers singular/plural wording.
    params = PARAMS[target]
    pos = [GOOD[params[i]] if i < len(params) else 1 for i in range(n)]
    same(target, *pos)


@given(st.sampled_from(sorted(PARAMS)), st.lists(st.sampled_from(BOGUS + ["base", "ndigits", "spec"]),
                                                   unique=True, min_size=1, max_size=3), st.booleans())
def test_full_args_plus_keywords(target, names, bad_first):
    # Every parameter supplied positionally, then extra keywords (duplicate or unknown).
    params = PARAMS[target]
    pos = [GOOD[p] for p in params]
    kw = {name: (D if (bad_first and i == len(names) - 1) else 1) for i, name in enumerate(names)}
    same(target, *pos, **kw)


# ---------- round_number ndigits that are not ints (index() error path) ----------

@given(st.one_of(st.sampled_from([2.5, -0.0, 1e300, float("nan")]), st.integers(-10**20, 10**20)),
       st.one_of(st.sampled_from(["1", b"1", 1.0, 1.5, -0.0, float("nan"), [1], (1,), {1: 2}, frozenset({1}), True, False]),
                 text(max_size=3), values(1)))
@example(1.5, "x")
@example(2, 1.5)
@example(1.5, [1])
@example(7, True)
def test_round_bad_ndigits(x, ndigits):
    same("round_number", x, ndigits)
    same("round_number", x, ndigits=ndigits)


# ---------- deep nesting: RecursionError vs. accepted ----------

def nested(depth, leaf, kind="list"):
    v = leaf
    for _ in range(depth):
        v = [v] if kind == "list" else (v,) if kind == "tuple" else {0: v}
    return v


@settings(max_examples=30, deadline=None)
@given(st.sampled_from([1, 10, 100, 400, 800]), st.sampled_from(["list", "tuple", "dict"]),
       st.sampled_from([1, D, "s"]),
       st.sampled_from(["float_repr", "int_str", "parse_int", "true_div"]))
def test_nested_values(depth, kind, leaf, target):
    same(target, nested(depth, leaf, kind))
