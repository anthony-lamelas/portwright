"""Review round 3 findings that claim behaviour differences.

#1 Pattern objects (build_date_regexps, build_time_regexps, isotzinfo.TZ_RE) must
   offer upstream's full match API: match/search/fullmatch/findall/finditer/scanner,
   groupindex, Match objects (group by name, start/end/span by name, expand,
   lastgroup, regs, __getitem__). Type identity is NOT compared (person's decision).
#2 Duration operator __doc__ / signatures: covered in test_review_methods.py;
   reviewer repros repeated here as explicit examples.
#4 tp_name in TypeError text: CPython prints the type's real tp_name, never a
   name/module/flags a metaclass reports. Feed objects whose metaclass spoofs
   __name__, __module__ (raising) and __flags__ to every API that formats it.
"""

from hypothesis import example, given, settings, strategies as st

from portwright.compare import same, PythonExpression
from portwright.compare.strategies import text

from iso_inputs import DT, ISO, date_string, time_string, mutate


def _wrap(src):
    code = (f"try:\n r = str({src})\n"
            "except BaseException as e:\n r = 'raised ' + type(e).__qualname__ + ': ' + str(e)\n"
            "r = __import__('re').sub(r' at 0x[0-9a-fA-F]+', ' at 0x', r)\n")
    return PythonExpression(f"(lambda ns: (exec({code!r}, ns), ns['r'])[1])({{}})")


def show(src, api):
    """str() of ``src`` on each side (or its exception), filed under ``api``."""
    return same(f"{api}.__name__.__class__", _wrap(src))


# ---------------------------------------------------------------- finding #1

TZ = f"{ISO}.isotzinfo.TZ_RE"
SOURCES = {
    "date": (f"{ISO}.isodates.build_date_regexps()", "isodates.build_date_regexps"),
    "date6": (f"{ISO}.isodates.build_date_regexps(6, True)", "isodates.build_date_regexps"),
    "time": (f"{ISO}.isotime.build_time_regexps()", "isotime.build_time_regexps"),
    "tz": (f"[{TZ}]", "isotzinfo.TZ_RE.match"),
}

MV = ("(lambda m: None if m is None else (m.span(), m[0], m.groups(), m.groups('-'), m.groupdict(), "
      "m.groupdict(0), m.lastgroup, m.lastindex, m.regs, m.re is p, m.string, m.pos, m.endpos, "
      "[(k, m.group(k), m[k], m.span(k), m.start(k), m.end(k)) for k in p.groupindex], "
      "[m.span(i) for i in range(p.groups + 1)], m.expand(r'<\\g<0>>'), repr(m)))")

USES = [
    "p.match(s)", "p.search(s)", "p.fullmatch(s)", "p.match(s, 1)", "p.search(s, 1, len(s) - 1)",
    "p.fullmatch(s, pos=0, endpos=len(s))", "p.match(string=s)", "p.scanner(s).match()",
    "p.scanner(s).search()", "p.scanner(s, 1).search()",
]
LISTY = [
    "p.findall(s)", "p.findall(s, 1)", "[m.span() for m in p.finditer(s)]",
    "[m.groupdict() for m in p.finditer(s, 0, 8)]", "p.sub('#', s)", "p.subn('', s, 1)",
    "p.split(s)", "p.split(s, maxsplit=1)", r"p.sub(r'\g<0>|', s, count=2)",
    "p.sub(lambda m: str(m.span()), s)", "sorted(p.groupindex.items())", "p.groups",
    "p.pattern", "p.flags", "repr(p)", "str(p)", "p == p", "hash(p) == hash(p)",
    "p.groupindex['tzname'] if 'tzname' in p.groupindex else None",
    "p.match()", "p.match(1)", "p.match(b'x')", "p.match(s, 'a')", "p.search(s, x=1)",
    "p.groupindex.__class__.__name__", "len(p.groupindex)", "p.sub('\\\\g<nope>', s)",
    "p.match(s).group('nope') if p.match(s) else None",
    "p.match(s).group(99) if p.match(s) else None",
    "__import__('copy').copy(p) == p", "__import__('copy').deepcopy(p) == p",
]

strings = st.one_of(
    date_string(), time_string(), mutate(date_string()), mutate(time_string()),
    st.sampled_from(["Z", "+01:00", "-0530", "+12", "z", "+1:00", "-00:00", "+2400", ""]),
    st.builds(lambda a, b: a + b, time_string(), st.sampled_from(["Z", "+01:00", "-05", "+0130"])),
    text(max_size=12),
)


@settings(max_examples=250)
@given(st.sampled_from(sorted(SOURCES)), st.sampled_from(USES + LISTY), strings)
@example("date", "p.match(s)", "2020-01-01")
@example("date", "p.findall(s)", "2020-01-01")
@example("date", "p.scanner(s).match()", "2020-01-01")
@example("date", "[m.span() for m in p.finditer(s)]", "2020-01-01")
@example("date", "sorted(p.groupindex.items())", "2020-01-01")
@example("time", "p.fullmatch(s)", "12:30:45Z")
@example("tz", "p.match(s)", "+01:00")
@example("tz", "p.search(s)", "x-0530")
@example("tz", "p.fullmatch(s)", "Z")
@example("tz", "p.findall(s)", "Z+01:00")
@example("tz", "p.scanner(s).search()", "aZ")
@example("tz", "sorted(p.groupindex.items())", "")
@example("tz", "p.match()", "")
@example("tz", "p.sub('#', s)", "+01")
def test_pattern_api(kind, use, s):
    src, api = SOURCES[kind]
    show(f"(lambda s: [(lambda r: r if r is None or isinstance(r, (str, int, tuple, list, bool)) "
         f"else {MV}(r))({use}) for p in {src}])({s!r})", api)


@given(st.sampled_from(sorted(SOURCES)), strings)
@example("tz", "+01:00")
@example("date", "2020-W01-1")
def test_match_view_direct(kind, s):
    """Full Match view of every pattern's match and search."""
    src, api = SOURCES[kind]
    for meth in ("match", "search", "fullmatch"):
        show(f"(lambda s: [(lambda p: {MV}(p.{meth}(s)))(p) for p in {src}])({s!r})", api)


@given(strings)
@example("+01:00")
@example("Z")
def test_tz_re_direct(s):
    """TZ_RE methods called straight through `same`."""
    for m in ("match", "search", "fullmatch", "findall", "split"):
        same(f"isotzinfo.TZ_RE.{m}", s)
    same("isotzinfo.TZ_RE.sub", "#", s)
    same("isotzinfo.TZ_RE.match", s, 1)


# ---------------------------------------------------------------- finding #2

@example("repr({D}.{m}.__doc__)", "__add__")
@example("repr({D}.{m}.__doc__)", "__mul__")
@example("repr({D}.{m}.__doc__)", "__sub__")
@example("str({ins}.signature({D}.{m}))", "__add__")
@example("str({ins}.signature({D}.{m}))", "__rmul__")
@example("str({ins}.signature({D}(1).{m}))", "__add__")
@example("repr({D}(1).{m}.__doc__)", "__radd__")
@example("{D}(1).{m}.__module__", "__rsub__")
@given(st.sampled_from(["repr({D}.{m}.__doc__)", "str({ins}.signature({D}.{m}))",
                        "str({ins}.signature({D}(1).{m}))", "repr({D}(1).{m}.__doc__)",
                        "{D}(1).{m}.__module__", "repr({ins}.getdoc({D}.{m}))"]),
       st.sampled_from(["__add__", "__radd__", "__sub__", "__rsub__", "__mul__", "__rmul__"]))
def test_op_doc_signature(p, m):
    show(p.format(D=f"{ISO}.Duration", ins="__import__('inspect')", m=m), f"Duration.{m}")


# ---------------------------------------------------------------- finding #4

# Metaclass variants: spoof __name__, make __module__ raise, fake __flags__.
_RAISE = "property(lambda c: (_ for _ in ()).throw(AttributeError('nomod')))"
METAS = {
    "name": "{'__name__': property(lambda c: 'Spoof')}",
    "module": "{'__module__': " + _RAISE + "}",
    "flags0": "{'__flags__': property(lambda c: 0)}",
    "all": "{'__name__': property(lambda c: 'Spoof'), '__module__': " + _RAISE +
           ", '__flags__': property(lambda c: 0)}",
    "plain": "{}",
}


def odd(meta):
    """An instance of class 'Real' whose metaclass misreports its identity."""
    return f"type('M', (type,), {METAS[meta]})('Real', (), {{}})()"


CALLS = {
    "Duration(days=x)": ("Duration", lambda x: f"{ISO}.Duration(days={x})"),
    "Duration(seconds=x)": ("Duration", lambda x: f"{ISO}.Duration(0, {x})"),
    "Duration(weeks=x)": ("Duration", lambda x: f"{ISO}.Duration(weeks={x})"),
    "Duration(years=x)": ("Duration", lambda x: f"{ISO}.Duration(years={x})"),
    "Duration+x": ("Duration", lambda x: f"{ISO}.Duration(1) + {x}"),
    "x-Duration": ("Duration", lambda x: f"{x} - {ISO}.Duration(1)"),
    "Duration*x": ("Duration", lambda x: f"{ISO}.Duration(1) * {x}"),
    "parse_date": ("parse_date", lambda x: f"{ISO}.parse_date({x})"),
    "parse_time": ("parse_time", lambda x: f"{ISO}.parse_time({x})"),
    "parse_tzinfo": ("parse_tzinfo", lambda x: f"{ISO}.parse_tzinfo({x})"),
    "parse_duration": ("parse_duration", lambda x: f"{ISO}.parse_duration({x})"),
    "strftime": ("strftime", lambda x: f"{ISO}.strftime({DT}.date(2000, 1, 1), {x})"),
    "FixedOffset": ("FixedOffset", lambda x: f"{ISO}.FixedOffset({x})"),
    "FixedOffset_m": ("FixedOffset", lambda x: f"{ISO}.FixedOffset(1, {x})"),
    "build_date_regexps": ("isodates.build_date_regexps", lambda x: f"{ISO}.isodates.build_date_regexps({x})"),
    "build_tzinfo": ("isotzinfo.build_tzinfo", lambda x: f"{ISO}.isotzinfo.build_tzinfo('', '+', {x})"),
    "TZ_RE.match": ("isotzinfo.TZ_RE.match", lambda x: f"{ISO}.isotzinfo.TZ_RE.match({x})"),
    "build_date_regexps()[0].match": ("isodates.build_date_regexps",
                                      lambda x: f"{ISO}.isodates.build_date_regexps()[0].match({x})"),
    "duration_isoformat": ("duration_isoformat", lambda x: f"{ISO}.duration_isoformat({ISO}.Duration(1), {x})"),
    "max_days_in_month": ("duration.max_days_in_month", lambda x: f"{ISO}.duration.max_days_in_month({x}, 1)"),
    "fquotmod": ("duration.fquotmod", lambda x: f"{ISO}.duration.fquotmod({x}, 1, 2)"),
}


@settings(max_examples=200)
@given(st.sampled_from(sorted(CALLS)), st.sampled_from(sorted(METAS)))
@example("Duration(days=x)", "module")
@example("Duration(days=x)", "name")
@example("Duration(days=x)", "flags0")
@example("Duration(days=x)", "all")
@example("parse_date", "all")
@example("FixedOffset", "all")
@example("strftime", "module")
@example("Duration*x", "all")
def test_metaclass_type_names(call, meta):
    api, build = CALLS[call]
    show(build(odd(meta)), api)


def test_metaclass_every_call():
    for call, (api, build) in CALLS.items():
        for meta in METAS:
            show(build(odd(meta)), api)
