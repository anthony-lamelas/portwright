"""Review round 4 (tree unchanged since round 3): reviewer's exact repros.

#1 `hasattr(p, x)` is False for match/search/findall/groupindex on the patterns from
   build_date_regexps / build_time_regexps / isotzinfo.TZ_RE. Compare which public
   Pattern / Match / Scanner members exist (a fixed list of re's API names, so type
   identity is not compared, per the person's decision).
#2 Duration operator __doc__/signature: explicit repros (also in test_review_methods.py).
"""

from hypothesis import example, given, settings, strategies as st

from portwright.compare import same, PythonExpression
from portwright.compare.strategies import text

from iso_inputs import ISO, date_string, time_string


def _wrap(src):
    code = (f"try:\n r = str({src})\n"
            "except BaseException as e:\n r = 'raised ' + type(e).__qualname__ + ': ' + str(e)\n"
            "r = __import__('re').sub(r' at 0x[0-9a-fA-F]+', ' at 0x', r)\n")
    return PythonExpression(f"(lambda ns: (exec({code!r}, ns), ns['r'])[1])({{}})")


def show(src, api):
    return same(f"{api}.__name__.__class__", _wrap(src))


SOURCES = {
    "date": (f"{ISO}.isodates.build_date_regexps()[0]", "isodates.build_date_regexps"),
    "date_last": (f"{ISO}.isodates.build_date_regexps(4, False)[-1]", "isodates.build_date_regexps"),
    "date6": (f"{ISO}.isodates.build_date_regexps(6, True)[0]", "isodates.build_date_regexps"),
    "time": (f"{ISO}.isotime.build_time_regexps()[0]", "isotime.build_time_regexps"),
    "tz": (f"{ISO}.isotzinfo.TZ_RE", "isotzinfo.TZ_RE.match"),
}

PATTERN_MEMBERS = ["match", "search", "fullmatch", "findall", "finditer", "scanner", "sub",
                   "subn", "split", "groupindex", "groups", "pattern", "flags"]
MATCH_MEMBERS = ["span", "start", "end", "group", "groups", "groupdict", "lastgroup",
                 "lastindex", "re", "string", "pos", "endpos", "expand", "regs", "__getitem__"]
SCANNER_MEMBERS = ["match", "search", "pattern"]


@settings(max_examples=60)
@given(st.sampled_from(sorted(SOURCES)), st.sampled_from(PATTERN_MEMBERS))
@example("date", "match")
@example("date", "search")
@example("date", "findall")
@example("date", "groupindex")
@example("tz", "match")
@example("tz", "search")
@example("tz", "findall")
@example("tz", "groupindex")
@example("time", "match")
@example("time", "groupindex")
def test_pattern_hasattr(kind, name):
    src, api = SOURCES[kind]
    show(f"(hasattr({src}, {name!r}), callable(getattr({src}, {name!r}, None)))", api)


def test_member_surface():
    for kind, (src, api) in SOURCES.items():
        show(f"[n for n in {PATTERN_MEMBERS!r} if hasattr({src}, n)]", api)
        show(f"[n for n in {SCANNER_MEMBERS!r} if hasattr({src}.scanner(''), n)]", api)


strings = st.one_of(date_string(), time_string(),
                    st.sampled_from(["2020-01-01", "12:30:45Z", "+01:00", "Z", "-0530", ""]),
                    text(max_size=10))


@given(st.sampled_from(sorted(SOURCES)), strings)
@example("date", "2020-01-01")
@example("time", "12:30:45Z")
@example("tz", "+01:00")
@example("tz", "Z")
def test_match_members(kind, s):
    src, api = SOURCES[kind]
    for meth in ("match", "search", "fullmatch"):
        show(f"(lambda m: m and [n for n in {MATCH_MEMBERS!r} if hasattr(m, n)])"
             f"({src}.{meth}({s!r}))", api)
    show(f"(lambda sc: (sc.pattern is {src}, (lambda m: m and m.span())(sc.search()), "
         f"(lambda m: m and m.span())(sc.search())))({src}.scanner({s!r}))", api)


DOC_PROBES = ["repr({D}.{m}.__doc__)", "str({ins}.signature({D}.{m}))",
              "repr({D}(1).{m}.__doc__)", "str({ins}.signature({D}(1).{m}))"]


@example("repr({D}.{m}.__doc__)", "__add__")
@example("repr({D}.{m}.__doc__)", "__mul__")
@example("str({ins}.signature({D}.{m}))", "__add__")
@example("repr({D}(1).{m}.__doc__)", "__add__")
@example("str({ins}.signature({D}(1).{m}))", "__sub__")
@given(st.sampled_from(DOC_PROBES),
       st.sampled_from(["__add__", "__radd__", "__sub__", "__rsub__", "__mul__", "__rmul__"]))
def test_op_doc_signature_repro(p, m):
    show(p.format(D=f"{ISO}.Duration", ins="__import__('inspect')", m=m), f"Duration.{m}")
