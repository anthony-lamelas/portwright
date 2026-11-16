"""Review round 5 (source unchanged from round 4): reviewer's exact repros.

#1 Pattern objects from build_date_regexps / build_time_regexps / TZ_RE must have
   match/search/findall/groupindex (type identity NOT compared, person's decision).
#2 Duration.__add__.__doc__ / __mul__.__doc__ / inspect.signature must match.
#4 TypeError text for a type whose metaclass hides __module__ (and fakes
   __flags__) must not depend on which API formats it.
Each probe's text goes through str reached via the API it is about."""

from hypothesis import example, given, settings, strategies as st

from portwright.compare import same, PythonExpression
from portwright.compare.strategies import text

from iso_inputs import DT, ISO, date_string, time_string


def show(src, api):
    code = (f"try:\n r = str({src})\n"
            "except BaseException as e:\n r = 'raised ' + type(e).__qualname__ + ': ' + str(e)\n"
            "r = __import__('re').sub(r' at 0x[0-9a-fA-F]+', ' at 0x', r)\n")
    return same(f"{api}.__name__.__class__",
                PythonExpression(f"(lambda ns: (exec({code!r}, ns), ns['r'])[1])({{}})"))


PATS = {
    "date": (f"{ISO}.isodates.build_date_regexps()[0]", "isodates.build_date_regexps"),
    "date_last": (f"{ISO}.isodates.build_date_regexps()[-1]", "isodates.build_date_regexps"),
    "time": (f"{ISO}.isotime.build_time_regexps()[0]", "isotime.build_time_regexps"),
    "tz": (f"{ISO}.isotzinfo.TZ_RE", "isotzinfo.TZ_RE.match"),
}
ATTRS = ["match", "search", "fullmatch", "findall", "finditer", "scanner", "sub", "subn",
         "split", "groupindex", "pattern", "flags", "groups"]


@given(st.sampled_from(sorted(PATS)), st.sampled_from(ATTRS))
@example("date", "match")
@example("date", "search")
@example("date", "findall")
@example("date", "groupindex")
@example("tz", "match")
@example("tz", "groupindex")
@example("time", "search")
def test_pattern_has_api(kind, attr):
    src, api = PATS[kind]
    show(f"hasattr({src}, {attr!r})", api)


@settings(max_examples=150)
@given(st.sampled_from(sorted(PATS)),
       st.sampled_from(["match", "search", "fullmatch", "findall"]),
       st.one_of(date_string(), time_string(), st.sampled_from(["Z", "+01:00", "-0530", ""]), text(max_size=10)))
@example("date", "match", "2020-01-01")
@example("date", "search", "x2020-01-01")
@example("date", "findall", "2020-01-01 2021-02-02")
@example("tz", "match", "+01:00")
@example("tz", "search", "12:00Z")
@example("time", "match", "12:30:45Z")
def test_pattern_call(kind, method, s):
    src, api = PATS[kind]
    show(f"(lambda r: r if not hasattr(r, 'span') else (r.span(), r.groups(), r.groupdict()))"
         f"({src}.{method}({s!r}))", api)


@given(st.sampled_from(sorted(PATS)))
def test_pattern_groupindex(kind):
    src, api = PATS[kind]
    show(f"sorted({src}.groupindex.items())", api)


D = f"{ISO}.Duration"
OPS = ["__add__", "__radd__", "__sub__", "__rsub__", "__mul__", "__rmul__"]


@given(st.sampled_from(OPS), st.sampled_from([
    "repr({D}.{m}.__doc__)", "str(__import__('inspect').signature({D}.{m}))",
    "str(__import__('inspect').signature({D}(1).{m}))", "repr({D}(1).{m}.__doc__)",
    "{D}.{m}.__module__", "{D}(1).{m}.__module__"]))
@example("__add__", "repr({D}.{m}.__doc__)")
@example("__mul__", "repr({D}.{m}.__doc__)")
@example("__add__", "str(__import__('inspect').signature({D}.{m}))")
@example("__add__", "str(__import__('inspect').signature({D}(1).{m}))")
def test_duration_method_doc_sig(m, p):
    show(p.format(D=D, m=m), f"Duration.{m}")


_RAISE = "property(lambda c: (_ for _ in ()).throw(AttributeError('nomod')))"
METAS = {
    "module": "{'__module__': " + _RAISE + "}",
    "module_flags0": "{'__module__': " + _RAISE + ", '__flags__': property(lambda c: 0)}",
    "flags0": "{'__flags__': property(lambda c: 0)}",
}


def odd(meta):
    return f"type('M', (type,), {METAS[meta]})('Real', (), {{}})()"


CALLS = {
    "Duration(days=x)": ("Duration", lambda x: f"{D}(days={x})"),
    "Duration(hours=x)": ("Duration", lambda x: f"{D}(hours={x})"),
    "Duration(milliseconds=x)": ("Duration", lambda x: f"{D}(milliseconds={x})"),
    "Duration+x": ("Duration", lambda x: f"{D}(1) + {x}"),
    "parse_date": ("parse_date", lambda x: f"{ISO}.parse_date({x})"),
    "parse_tzinfo": ("parse_tzinfo", lambda x: f"{ISO}.parse_tzinfo({x})"),
    "strftime": ("strftime", lambda x: f"{ISO}.strftime({DT}.date(2000, 1, 1), {x})"),
    "FixedOffset": ("FixedOffset", lambda x: f"{ISO}.FixedOffset({x})"),
}


@given(st.sampled_from(sorted(CALLS)), st.sampled_from(sorted(METAS)))
@example("Duration(days=x)", "module_flags0")
@example("Duration(hours=x)", "module_flags0")
@example("parse_date", "module_flags0")
@example("FixedOffset", "module_flags0")
def test_hidden_module_type_name(call, meta):
    api, build = CALLS[call]
    show(build(odd(meta)), api)
