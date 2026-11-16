"""Review finding #6: build_date_regexps/build_time_regexps return a stand-in for
re.Pattern. Upstream returns real compiled patterns, which callers can use: match,
search, fullmatch, groupindex, and the Match objects they produce. Compare that use.

(isinstance(p, re.Pattern) can't be True without importing re, which the port
may not do; it is left out, see /work/out/tried.md.)"""

from hypothesis import example, given, settings, strategies as st

from portwright.compare import same, PythonExpression
from portwright.compare.strategies import text

from iso_inputs import ISO, date_string, time_string, mutate


def show(src, api="isodates.build_date_regexps"):
    """str() of ``src`` on each side, or the exception's type and message."""
    code = (f"try:\n r = str({src})\n"
            "except BaseException as e:\n r = 'raised ' + type(e).__qualname__ + ': ' + str(e)\n"
            "r = __import__('re').sub(r' at 0x[0-9a-fA-F]+', ' at 0x', r)\n")
    # Text goes through str reached via the API the probe is about.
    return same(f"{api}.__name__.__class__", PythonExpression(f"(lambda ns: (exec({code!r}, ns), ns['r'])[1])({{}})"))


API = {"date": "isodates.build_date_regexps", "time": "isotime.build_time_regexps"}


def patterns(kind, yd, exp):
    if kind == "date":
        return f"{ISO}.isodates.build_date_regexps({yd}, {exp})"
    return f"{ISO}.isotime.build_time_regexps()"


MATCH_VIEW = ("(lambda m: None if m is None else (m.span(), m.group(0), m.groups(), m.groupdict(), "
              "m.lastgroup, m.lastindex, m.re is p, m.string == s, m.pos, m.endpos))")

METHODS = ["match", "fullmatch", "search"]

strings = st.one_of(
    date_string(), date_string(yeardigits=6, expanded=True), date_string(yeardigits=2),
    time_string(), mutate(date_string()), mutate(time_string()),
    st.builds(lambda a, b: a + "T" + b, date_string(), time_string()),
    text(max_size=12),
)


@settings(max_examples=150)
@given(st.sampled_from(["date", "time"]), st.integers(1, 7), st.booleans(), strings,
       st.sampled_from(METHODS))
@example("date", 4, False, "2020-01-02", "match")
@example("date", 4, False, "2020-01-02", "fullmatch")
@example("date", 6, True, "+002020-01-02", "match")
@example("date", 4, False, "x2020-01-02", "search")
@example("time", 4, False, "23:20:50.5+01:00", "match")
@example("time", 4, False, "T232050Z", "match")
@example("date", 4, False, "2020-W01-1\n", "match")
@example("date", 4, False, "\ud800", "match")
def test_pattern_methods(kind, yd, exp, s, method):
    show(f"(lambda s: [(lambda p: {MATCH_VIEW}(p.{method}(s)))(p) "
         f"for p in {patterns(kind, yd, exp)}])({s!r})", API[kind])


@given(st.sampled_from(["date", "time"]), st.integers(1, 7), st.booleans(), strings,
       st.integers(-3, 20), st.integers(-3, 30))
@example("date", 4, False, "xx2020-01-02", 2, 12)
def test_pattern_pos_endpos(kind, yd, exp, s, pos, endpos):
    show(f"(lambda s: [(lambda p: {MATCH_VIEW}(p.search(s, {pos}, {endpos})))(p) "
         f"for p in {patterns(kind, yd, exp)}])({s!r})", API[kind])


@given(st.sampled_from(["date", "time"]), st.integers(1, 7), st.booleans())
@example("date", 4, False)
@example("time", 4, False)
def test_pattern_attributes(kind, yd, exp):
    show(f"[(p.pattern, p.flags, p.groups, sorted(p.groupindex.items()), p == p, hash(p) == hash(p)) "
         f"for p in {patterns(kind, yd, exp)}]", API[kind])


@given(st.sampled_from(["date", "time"]), strings)
@example("date", "2020-01-02")
def test_pattern_findall_sub(kind, s):
    for use in ("p.findall(s)", "[m.span() for m in p.finditer(s)]", "p.sub('#', s)", "p.split(s)"):
        show(f"(lambda s: [{use} for p in {patterns(kind, 4, False)}])({s!r})", API[kind])


# ---- review finding 2: reviewer's repro, default arguments, more Pattern API -----

USES = ["p.match(s)", "p.fullmatch(s)", "p.search(s)", "p.findall(s)",
        "[m.span() for m in p.finditer(s)]", "p.sub('#', s)", "p.subn('#', s)", "p.split(s)",
        "p.sub(lambda m: m.group(0)[::-1], s)", r"p.sub(r'\g<0>!', s)",
        "sorted(p.groupindex.items())", "p.groups", "p.pattern", "p.flags", "repr(p)",
        "p.match(s, 1)", "p.fullmatch(s, 0, len(s) - 1)", "p.scanner(s).match()",
        r"(lambda m: m and (m.expand(r'\g<0>'), m.start(0), m.end(0), m.regs))(p.match(s))"]


@settings(max_examples=150)
@given(st.sampled_from(["date", "time"]), st.sampled_from(USES), strings)
@example("date", "(lambda m: m and (m.span(), m.groupdict()))(p.match(s))", "2020-01-01")
@example("date", "p.match(s)", "2020-01-01")
@example("date", "p.search(s)", "x2020-01-01")
@example("time", "p.match(s)", "12:30:45Z")
@example("date", "sorted(p.groupindex.items())", "")
@example("date", "p.findall(s)", "2020-01-01")
@example("date", "p.split(s)", "2020-01-01")
def test_pattern_defaults(kind, use, s):
    """build_*_regexps() called with defaults; reviewer's repro is
    build_date_regexps()[0].match("2020-01-01")."""
    call = f"{ISO}.isodates.build_date_regexps()" if kind == "date" else f"{ISO}.isotime.build_time_regexps()"
    show(f"(lambda s: [(lambda r: r if r is None or isinstance(r, (str, int, tuple, list)) "
         f"else {MATCH_VIEW}(r))({use}) for p in {call}])({s!r})", API[kind])
