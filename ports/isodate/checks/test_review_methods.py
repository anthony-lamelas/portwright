"""Review finding: Duration's operator methods (__add__/__radd__/__sub__/__rsub__/
__mul__/__rmul__) must match upstream in names, __qualname__, __module__, __doc__,
signatures, binding, calls and errors.

Per the person's decision, their type identity, isfunction/ismethod,
FunctionType/MethodType and function-only attributes (__code__, __globals__,
__defaults__, __closure__, ...) may differ and are NOT compared here.

Each probe is evaluated on both sides; its text goes through ``str`` reached via
the Duration method the probe is about (``Duration.<m>.__name__.__class__``), so a
difference is filed under that method, not under an unrelated API."""

from hypothesis import example, given, settings, strategies as st

from portwright.compare import same, PythonExpression

from iso_inputs import ISO

INS = "__import__('inspect')"
TYP = "__import__('types')"
D = f"{ISO}.Duration"

OPS = ["__add__", "__radd__", "__sub__", "__rsub__", "__mul__", "__rmul__"]
# Other Duration methods are PyO3 method/slot descriptors; their introspection differs
# from upstream too, but that is outside this finding (see /work/out/tried.md).

PROBES = [
    "repr({D}.{m}.__doc__)",
    "repr({d}.{m}.__doc__)",
    "{D}.{m}.__name__",
    "{D}.{m}.__qualname__",
    "{D}.{m}.__module__",
    "{d}.{m}.__qualname__",
    "{ins}.getdoc({D}.{m})",
    "str({ins}.signature({D}.{m}))",
    "str({ins}.signature({d}.{m}))",
    "{d}.{m}.__func__ is {D}.{m}",
    "(lambda x: x.{m}.__self__ is x)({d})",
    "{D}.{m}.__get__({d}, {D}).__func__ is {D}.{m}",
    "{D}.__dict__.get('{m}') is {D}.{m}",
    "{ins}.isroutine({D}.{m})",
    "hasattr({D}.{m}, '__wrapped__')",
    "callable({D}.{m})",
    "sorted(k for k in ('__self__', '__func__', '__wrapped__') if hasattr({D}.{m}, k))",
    # Review finding 1: docs, signatures and module of the bound method too.
    "repr({ins}.getdoc({d}.{m}))",
    "{d}.{m}.__name__",
    "{d}.{m}.__module__",
    "repr({d}.{m}.__doc__) == repr({D}.{m}.__doc__)",
    "[(n, str(p.kind), p.default is p.empty) for n, p in {ins}.signature({D}.{m}).parameters.items()]",
    "[(n, str(p.kind)) for n, p in {ins}.signature({d}.{m}).parameters.items()]",
    "{ins}.signature({D}.{m}).return_annotation is {ins}.Signature.empty",
    "str({ins}.signature({D}.{m}.__get__({d}, {D})))",
    "str({ins}.signature({d}.{m}.__func__))",
    "{ins}.isroutine({d}.{m})",
    "type({D}.{m}.__module__).__name__",
    "type({d}.{m}.__module__).__name__",
    "type({D}.{m}.__name__).__name__ + type({D}.{m}.__qualname__).__name__",
]

DURS = [f"{D}(1)", f"{D}(years=2, months=3, days=4)", f"{D}(months=-1.5)"]


def show(src, m="__add__"):
    """Evaluate ``src`` on each side; compare its str, or the exception it raised.

    An exception escaping a PythonExpression kills the worker, so the expression
    catches it itself (via exec) and reports type and message as text."""
    code = (f"try:\n r = str({src})\n"
            "except BaseException as e:\n r = 'raised ' + type(e).__qualname__ + ': ' + str(e)\n"
            "r = __import__('re').sub(r' at 0x[0-9a-fA-F]+', ' at 0x', r)\n")
    return same(f"Duration.{m}.__name__.__class__", PythonExpression(f"(lambda ns: (exec({code!r}, ns), ns['r'])[1])({{}})"))


def probe(p, m, d):
    show(p.format(ins=INS, typ=TYP, D=D, d=d, m=m), m)


@settings(max_examples=200)
@given(st.sampled_from(PROBES), st.sampled_from(OPS), st.sampled_from(DURS))
@example("repr({D}.{m}.__doc__)", "__add__", f"{D}(1)")
@example("repr({D}.{m}.__doc__)", "__sub__", f"{D}(1)")
@example("repr({D}.{m}.__doc__)", "__rsub__", f"{D}(1)")
@example("repr({D}.{m}.__doc__)", "__mul__", f"{D}(1)")
@example("{D}.{m}.__qualname__", "__radd__", f"{D}(1)")
@example("{D}.{m}.__qualname__", "__rmul__", f"{D}(1)")
def test_method_identity(p, m, d):
    probe(p, m, d)


def test_every_op_probe():
    """Every probe on every operator method, deterministically."""
    for m in OPS:
        for p in PROBES:
            probe(p, m, DURS[1])


def test_aliases():
    for src in [f"{D}.__radd__ is {D}.__add__", f"{D}.__rmul__ is {D}.__mul__",
                f"{D}.__add__ == {D}.__radd__", f"{D}.__sub__ is {D}.__rsub__",
                f"{D}(1).__add__ == {D}(1).__add__", f"{D}(1).__radd__ == {D}(1).__add__",
                f"hash({D}.__add__) == hash({D}.__radd__)",
                f"{ISO}.duration.Duration.__add__ is {D}.__add__"]:
        show(src)


OPERANDS = ["1", "2.5", "'x'", "None", f"{D}(2)", f"{D}(years=1)",
            "__import__('datetime').timedelta(3)", "__import__('datetime').date(2000, 1, 31)",
            "__import__('datetime').datetime(2000, 1, 31, 12)", "__import__('decimal').Decimal('1.5')",
            "-3", "0", "True"]


@settings(max_examples=200)
@given(st.sampled_from(OPS), st.sampled_from(DURS), st.sampled_from(OPERANDS),
       st.sampled_from(["unbound", "get", "bound", "func"]))
@example("__add__", f"{D}(1)", "'x'", "unbound")
@example("__rsub__", f"{D}(1)", "__import__('datetime').date(2000, 1, 31)", "unbound")
@example("__mul__", f"{D}(years=2, months=3, days=4)", "2.5", "func")
def test_call_forms(m, d, other, form):
    """Calling the method objects directly (not via operators): NotImplemented
    returns, exceptions and results must match."""
    call = {
        "unbound": f"{D}.{m}({d}, {other})",
        "get": f"{D}.{m}.__get__({d})({other})",
        "bound": f"{d}.{m}({other})",
        "func": f"{d}.{m}.__func__({d}, {other})",
    }[form]
    show(f"repr({call})", m)


@given(st.sampled_from(OPS), st.sampled_from(["()", "(1, 2, 3)", "(other=1)", "(self=1, other=2)"]))
def test_bad_arity(m, args):
    """Wrong argument counts/keywords: TypeError text names the function."""
    show(f"(lambda f: (lambda: f{args})())({D}.{m})" if args != "()" else f"{D}.{m}()", m)


# ---- review finding 1: explicit repros (doc, signature) --------------------------

@example("repr({D}.{m}.__doc__)", "__add__", f"{D}(1)")
@example("repr({D}.{m}.__doc__)", "__mul__", f"{D}(1)")
@example("repr({D}.{m}.__doc__)", "__rmul__", f"{D}(1)")
@example("repr({d}.{m}.__doc__)", "__add__", f"{D}(1)")
@example("repr({d}.{m}.__doc__)", "__mul__", f"{D}(1)")
@example("str({ins}.signature({D}.{m}))", "__add__", f"{D}(1)")
@example("str({ins}.signature({D}.{m}))", "__rsub__", f"{D}(1)")
@example("str({ins}.signature({d}.{m}))", "__add__", f"{D}(1)")
@example("str({ins}.signature({d}.{m}))", "__rmul__", f"{D}(years=2, months=3, days=4)")
@example("{d}.{m}.__module__", "__sub__", f"{D}(1)")
@example("type({D}.{m}.__module__).__name__", "__add__", f"{D}(1)")
@given(st.sampled_from([p for p in PROBES if "doc" in p or "signature" in p or "__module__" in p]),
       st.sampled_from(OPS), st.sampled_from(DURS))
def test_method_doc_signature(p, m, d):
    probe(p, m, d)


# ---- Duration.__new__ without __init__ (review finding 6: object.__new__ on the pyclass)

NEW_PROBES = ["repr({D}.__new__({D}))", "{D}.__new__({D}).years", "{D}.__new__({D}).tdelta",
              "{D}.__new__({D}) == {D}()", "str({D}.__new__({D}))",
              "type({D}.__new__({D})) is {D}", "{D}.__new__({D}).totimedelta(__import__('datetime').date(2000, 1, 1))",
              "{D}.__new__({D}, 1, 2)", "{D}.__new__({D}).__init__(days=1)",
              "(lambda x: (x.__init__(days=3, years=1), repr(x))[1])({D}.__new__({D}))"]


@given(st.sampled_from(NEW_PROBES))
@example("repr({D}.__new__({D}))")
@example("{D}.__new__({D}).years")
def test_new_without_init(p):
    code = p.format(D=D)
    c = (f"try:\n r = str({code})\n"
         "except BaseException as e:\n r = 'raised ' + type(e).__qualname__ + ': ' + str(e)[:60]\n"
         "r = __import__('re').sub(r' at 0x[0-9a-fA-F]+', ' at 0x', r)\n")
    same("Duration.__new__.__name__.__class__",
         PythonExpression(f"(lambda ns: (exec({c!r}, ns), ns['r'])[1])({{}})"))
