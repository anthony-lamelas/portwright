"""Differential checks for pc_re: pc_re (CPython 3.12 sre) is the reference for everything."""

from hypothesis import given, settings, strategies as st
from portwright.compare import same, get, Pair
from portwright.compare.strategies import text

import re_strategies as rs

CONSTANTS = ["ASCII", "DOTALL", "IGNORECASE", "LOCALE", "MULTILINE", "NOFLAG", "UNICODE", "VERBOSE", "error"]
GROUP_NAMES = rs.NAMES + ["nope", "", "1", "x"]
DEFAULTS = [None, "", "-", 0, -1, 1.5, b"d", ["l"], ("t",), True, {"k": "v"}]


def flags_args(draw, f):
    """Either omit flags, pass them positionally, or by keyword."""
    how = draw(st.sampled_from(["omit", "pos", "pos", "kw"]))
    if how == "omit":
        return (), {}
    if how == "pos":
        return (f,), {}
    return (), {"flags": f}


def probe_match(draw, m):
    """Read everything a Match exposes, through both versions."""
    if not isinstance(m, Pair):
        return
    for name in ["match_groups", "match_groupdict", "match_lastindex", "match_lastgroup", "match_pos",
                 "match_endpos", "match_string", "match_group", "match_start", "match_end", "match_span"]:
        same(name, m)
    for g in draw(st.lists(st.one_of(st.integers(-2, 5), st.sampled_from(GROUP_NAMES)), min_size=1, max_size=4)):
        same("match_group", m, g)
        same("match_start", m, g)
        same("match_end", m, g)
        same("match_span", m, g)
    if draw(st.booleans()):
        same("match_group", m, *draw(st.lists(st.one_of(st.integers(0, 3), st.sampled_from(GROUP_NAMES)),
                                              min_size=2, max_size=4)))
    if draw(st.integers(0, 4)) == 0:
        bad = draw(st.one_of(rs.bad_index(), st.sampled_from([2 ** 31, 2 ** 63, -2 ** 63, 2 ** 100, -2 ** 100])))
        same(draw(st.sampled_from(["match_group", "match_start", "match_end", "match_span"])), m, bad)
    d = draw(st.sampled_from(DEFAULTS))
    same("match_groups", m, d)
    same("match_groupdict", m, d)
    if draw(st.booleans()):
        same("match_groups", m, default=d)
        same("match_groupdict", m, default=d)
        same("match_start", m, group=draw(st.integers(0, 2)))
    same("match_expand", m, draw(rs.template()))
    same("match_expand", m, draw(rs.safe_template()))


@given(st.data())
def test_module_functions(data):
    draw = data.draw
    p = draw(rs.any_pattern(), label="pattern")
    s = draw(rs.subject_for(p), label="subject")
    fa, fk = flags_args(draw, draw(rs.flags(), label="flags"))
    for name in ["search", "match", "fullmatch"]:
        m = same(name, p, s, *fa, **fk)
        probe_match(draw, m)
    same("findall", p, s, *fa, **fk)
    same("finditer", p, s, *fa, **fk)
    maxsplit = draw(st.sampled_from([None, 0, 1, 2, -1, 5]))
    if maxsplit is None:
        same("split", p, s, **fk) if not fa else same("split", p, s, 0, *fa)
    elif fk or draw(st.booleans()):
        same("split", p, s, maxsplit=maxsplit, **fk)
    else:
        same("split", p, s, maxsplit, *fa)
    repl = draw(st.one_of(rs.template(), rs.safe_template()), label="repl")
    count = draw(st.sampled_from([None, 0, 1, 2, -1, 100]))
    for name in ["sub", "subn"]:
        if count is None:
            same(name, p, repl, s, **fk) if not fa else same(name, p, repl, s, 0, *fa)
        elif fk:
            same(name, p, repl, s, count=count, **fk)
        else:
            same(name, p, repl, s, count, *fa)


@given(st.data())
def test_valid_patterns_with_matches(data):
    """Mostly-compiling patterns against subjects drawn from their own characters."""
    draw = data.draw
    p = draw(rs.valid_pattern(), label="pattern")
    f = draw(st.sampled_from([0, 0, 0, 2, 8, 16, 64, 256, 2 | 256, 2 | 8 | 16]), label="flags")
    for _ in range(3):
        s = draw(rs.subject_for(p), label="subject")
        m = same(draw(st.sampled_from(["search", "match", "fullmatch"])), p, s, f)
        probe_match(draw, m)
        same("finditer", p, s, f)
        same("findall", p, s, f)
        same("split", p, s, draw(st.integers(0, 3)), f)
        same("subn", p, draw(rs.template()), s, draw(st.integers(0, 3)), f)
        same("sub", p, draw(rs.safe_template()), s, 0, f)


@given(st.data())
def test_compiled_pattern(data):
    draw = data.draw
    p = draw(st.one_of(rs.valid_pattern(), rs.any_pattern()), label="pattern")
    if draw(st.booleans()):
        c = same("compile", p)
    else:
        c = same("compile", p, draw(rs.flags(), label="flags"))
    if not isinstance(c, Pair):
        return
    for name in ["pattern_text", "pattern_flags", "pattern_groups", "pattern_groupindex"]:
        same(name, c)
    for _ in range(draw(st.integers(1, 3))):
        s = draw(rs.subject_for(p), label="subject")
        how = draw(st.sampled_from(["none", "pos", "both", "kw", "endpos_kw"]))
        pos = draw(rs.index_like())
        endpos = draw(st.one_of(rs.index_like(), st.none()))
        for name in ["pattern_search", "pattern_match", "pattern_fullmatch", "pattern_findall", "pattern_finditer"]:
            if how == "none":
                r = same(name, c, s)
            elif how == "pos":
                r = same(name, c, s, pos)
            elif how == "both":
                r = same(name, c, s, pos, endpos)
            elif how == "kw":
                r = same(name, c, s, pos=pos, endpos=endpos)
            else:
                r = same(name, c, s, endpos=endpos)
            probe_match(draw, r)
        ms = draw(st.sampled_from([None, 0, 1, 2, -1, 10]))
        same("pattern_split", c, s) if ms is None else same("pattern_split", c, s, ms)
        cnt = draw(st.sampled_from([None, 0, 1, 2, -1, 10]))
        repl = draw(st.one_of(rs.template(), rs.safe_template()), label="repl")
        for name in ["pattern_sub", "pattern_subn"]:
            if cnt is None:
                same(name, c, repl, s)
            elif draw(st.booleans()):
                same(name, c, repl, s, cnt)
            else:
                same(name, c, repl, s, count=cnt)


SIMPLE = ["(a)(b)?", "(?P<n>a)(?P<m>b)?c?", "(a)|(b)", "(?P<n>\\w)+", "((a)|b)+", "(a*)(b*)", "(?:(a)|(b))*",
          "(?P<n>x)(?P<m>y)?(?P<g1>z)?", "", "(?P<é>é)|(?P<Σ>Σ)", "(.)(.)(.)(.)(.)(.)(.)(.)(.)(.)(.)(.)",
          "(?P<n>a)|(?P<m>b)|(c)", "(a)(?=(b))", "(?<=(a))(b)"]


@given(st.sampled_from(SIMPLE), st.data())
def test_match_accessors_on_groupy_patterns(p, data):
    """Patterns with many groups, so lastindex/lastgroup/groupdict/expand are exercised on real matches."""
    draw = data.draw
    c = same("compile", p, draw(st.sampled_from([0, 2])))
    s = draw(st.one_of(rs.subject_for(p), st.lists(st.sampled_from(list("abcxyzé Σ")), max_size=14).map("".join)))
    for m in [same("pattern_search", c, s), same("pattern_search", c, s, draw(st.integers(-2, 5))),
              same("pattern_match", c, s, draw(st.integers(0, 3)), draw(st.integers(-1, 15))),
              same("search", p, s)]:
        probe_match(draw, m)
    same("pattern_finditer", c, s)
    same("pattern_findall", c, s, draw(st.integers(0, 3)))
    t = draw(rs.template())
    same("pattern_sub", c, t, s)
    same("pattern_subn", c, t, s, draw(st.integers(0, 3)))
    same("pattern_split", c, s, draw(st.integers(-1, 3)))


@given(st.data())
def test_upstream_tables(data):
    draw = data.draw
    p = draw(st.sampled_from(rs.UPSTREAM_PATTERNS + rs.UPSTREAM_ERRORS))
    s = draw(st.one_of(st.sampled_from([
        '', 'a', 'aa', 'aaa', 'ab', 'abc', 'ABC', 'aBc', 'ba', 'cab', 'b', 'c', 'x', 'xy', 'abab', 'aab', 'aaab',
        'hello world', 'John Smith', '11', '1 2 3', 'a\nb\nc', 'é', 'ÉÉ', 'Σσς', 'ss ß SS', 'ſ s S', 'K k K', 'İi',
        '—', '😀', 'a.b', '$', '\\', '\t\n', 'x\ud800y', '\x00', 'AAA', 'c b a', 'abcdefghijkl', 'a\n', '\n']),
        rs.subject_for(p)))
    f = draw(st.sampled_from([0, 2, 8, 16, 64, 256, 2 | 256, 2 | 8 | 16 | 64]))
    for name in ["search", "match", "fullmatch"]:
        probe_match(draw, same(name, p, s, f))
    same("findall", p, s, f)
    same("finditer", p, s, f)
    same("split", p, s, draw(st.integers(0, 2)), f)
    same("subn", p, draw(rs.template()), s, 0, f)


@given(st.data())
def test_flags(data):
    draw = data.draw
    p = draw(st.one_of(st.sampled_from(['^a.', 'a$', '\\w+', '(?i)b', '[k]', 's', ' a # x', '(?u)x', '(?a)\\w',
                                        '(?L)a', '\\bß\\b', '[^\\W]', '(?i)[^k]', '\\d', '\\s', 'a\n#c\nb', '.']),
                                       rs.valid_pattern()))
    s = draw(st.one_of(st.sampled_from(['a\nA\nb', 'K', 'ſ', 'ﬀ', 'x b', 'É', 'ab', '٣', '\xa0', ' ']),
                       rs.subject_for(p)))
    f = draw(st.one_of(rs.flags(), rs.bad_flags()))
    same("search", p, s, f)
    c = same("compile", p, f)
    if isinstance(c, Pair):
        same("pattern_flags", c)
        same("pattern_findall", c, s)


@given(rs.valid_pattern(), st.sampled_from([128, 128 | 2, 128 | 8 | 16, 128 | 64, 128 | 256, 128 | 32]))
@settings(max_examples=40)
def test_debug_flag_output(p, f):
    """re.DEBUG prints the parsed pattern and the compiled code."""
    same("compile", p, f)


@given(text(max_size=40))
def test_escape(s):
    same("escape", s)
    e = same("escape", s)
    same("fullmatch", e, s)
    same("findall", e, s + s)


@given(st.sampled_from(rs.ALPHABET + ["".join(map(chr, range(128))), "[]{}()*+?^$|\\", "#&~", "-_/", " \t\n\r\x0b\x0c"]))
def test_escape_specials(s):
    same("escape", s)


def test_constants():
    for name in CONSTANTS:
        get(name)


@given(st.one_of(text(max_size=10), st.sampled_from(["bad", "", "missing ), unterminated subpattern"]), rs.non_str()),
       st.one_of(st.none(), text(max_size=12), st.sampled_from(["a\nb\nc", "\n", "abc\n\ndef", "x\r\ny"]),
                 st.sampled_from([b"a\nb", b"abc"])),
       st.one_of(st.none(), st.integers(-3, 15), st.sampled_from([1.5, "2", 2 ** 70])))
def test_error_constructor(msg, pattern, pos):
    same("error", msg)
    same("error", msg, pattern, pos)
    same("error", msg, pattern=pattern, pos=pos)
    same("error", msg, pattern)


@given(st.data())
def test_compile_errors(data):
    """Error messages include the position (and line/column for multi-line patterns)."""
    draw = data.draw
    p = draw(st.one_of(st.sampled_from(rs.UPSTREAM_ERRORS), rs.near_miss(),
                       rs.near_miss().map(lambda q: q.replace("|", "\n|") if q else q),
                       st.builds(lambda a, b: a + "\n" + b, rs.valid_pattern(), st.sampled_from(rs.UPSTREAM_ERRORS))))
    f = draw(st.sampled_from([0, 2, 64, 16, 256]))
    same("compile", p, f)
    same("search", p, draw(rs.subject_for(p)), f)


@given(st.data())
def test_wrong_types(data):
    draw = data.draw
    bad = draw(rs.non_str())
    p = draw(rs.valid_pattern())
    s = draw(rs.subject_for(p))
    which = draw(st.integers(0, 2))
    args3 = [p, s, 0]
    args3[which] = bad
    same(draw(st.sampled_from(["search", "match", "fullmatch", "findall", "finditer", "split"])), *args3)
    sub_args = [p, "x", s]
    sub_args[draw(st.integers(0, 2))] = bad
    same(draw(st.sampled_from(["sub", "subn"])), *sub_args)
    same("compile", bad)
    same("escape", bad)
    same("compile", p, draw(rs.bad_flags()))
    same("sub", p, "x", s, draw(rs.bad_index()))
    same("split", p, s, draw(rs.bad_index()))
    c = same("compile", p)
    if isinstance(c, Pair):
        name = draw(st.sampled_from(["pattern_search", "pattern_match", "pattern_fullmatch", "pattern_findall",
                                     "pattern_finditer", "pattern_split"]))
        same(name, c, bad)
        same(name, c, s, draw(rs.bad_index()))
        if name != "pattern_split":
            same(name, c, s, 0, draw(rs.bad_index()))
        else:
            same(name, c, s, draw(rs.index_like()))
        same(draw(st.sampled_from(["pattern_sub", "pattern_subn"])), c, bad, s)
        same(draw(st.sampled_from(["pattern_sub", "pattern_subn"])), c, "x", bad)
        same(draw(st.sampled_from(["pattern_sub", "pattern_subn"])), c, "x", s, draw(rs.bad_index()))
        # a Pattern where a str is expected, and the other way round
        same(draw(st.sampled_from(["search", "match", "compile", "findall", "escape"])), c, s)
        same("sub", p, c, s)
        m = same("pattern_search", c, s)
        if isinstance(m, Pair):
            same("match_expand", m, bad)
            same(draw(st.sampled_from(["pattern_search", "pattern_text", "pattern_groups", "pattern_sub"])), m, "x")
            same(draw(st.sampled_from(["match_group", "match_span", "match_string", "match_groups"])), c)
        same(draw(st.sampled_from(["pattern_text", "pattern_flags", "pattern_groupindex", "match_group", "match_pos"])),
             draw(st.sampled_from([p, None, 1, b"a"])))
        # arguments nested in containers are checked too
        same("search", p, s, draw(st.sampled_from([[c], {"k": c}, (1, [None, 2.0])])))


@given(st.sampled_from(["a+", "\\w+", ".+", "(?s).*", "\\S+", "[^\\n]+", "(.)", "(?P<n>.+)", "\\W*", "x*"]),
       text(min_size=30, max_size=300), st.data())
@settings(max_examples=60)
def test_long_strings(p, s, data):
    """Match reprs truncate the matched text; pattern reprs truncate long patterns."""
    m = same("search", p, s)
    probe_match(data.draw, m)
    same("finditer", p, s)
    same("sub", p, "<\\g<0>>", s)
    same("split", p, s)
    e = same("escape", s)
    if isinstance(e, dict) and e.get("t") == "str":
        c = same("compile", e["v"], data.draw(st.sampled_from([0, 2, 2 | 8])))
        if isinstance(c, Pair):
            same("pattern_text", c)
            same("pattern_fullmatch", c, s)
    same("compile", "(?x)" + s[:250])


@given(st.data())
def test_kwargs_and_defaults(data):
    draw = data.draw
    p = draw(rs.valid_pattern())
    s = draw(rs.subject_for(p))
    same("search", pattern=p, string=s)
    same("match", p, string=s, flags=draw(rs.flags()))
    same("fullmatch", p, s, flags=0)
    same("findall", pattern=p, string=s, flags=2)
    same("sub", p, repl="-", string=s, count=1)
    same("subn", pattern=p, repl="", string=s)
    same("split", p, string=s, maxsplit=1)
    same("escape", pattern=s)
    c = same("compile", pattern=p, flags=0)
    if isinstance(c, Pair):
        same("pattern_sub", c, repl="x", string=s, count=1)
        same("pattern_split", c, string=s, maxsplit=2)
        m = same("pattern_search", c, string=s, pos=0, endpos=None)
        if isinstance(m, Pair):
            same("match_group", m, 0)
            same("match_start", m, group=0)
            same("match_end", m, group=0)
            same("match_span", m, group=0)
            same("match_expand", m, template="\\g<0>")
    # unknown keyword arguments
    same("search", p, s, flag=2)


@given(st.data())
def test_patterns_with_witnesses(data):
    """Patterns drawn together with a subject they match, so the engine really backtracks and captures."""
    draw = data.draw
    p, s = draw(rs.pattern_and_subject(depth=draw(st.sampled_from([1, 2, 2, 3]))), label="pattern, subject")
    f = draw(st.sampled_from([0, 0, 0, 2, 8, 16, 64, 2 | 16, 256]), label="flags")
    for name in ["search", "match", "fullmatch"]:
        probe_match(draw, same(name, p, s, f))
    same("findall", p, s, f)
    same("finditer", p, s, f)
    same("split", p, s, draw(st.integers(0, 3)), f)
    same("sub", p, draw(rs.template()), s, draw(st.integers(0, 3)), f)
    same("subn", p, draw(st.one_of(rs.safe_template(), rs.template())), s, 0, f)
    c = same("compile", p, f)
    if isinstance(c, Pair):
        same("pattern_groupindex", c)
        same("pattern_groups", c)
        pos = draw(st.integers(0, max(0, len(s))))
        endpos = draw(st.integers(pos, len(s) + 1))
        for name in ["pattern_search", "pattern_match", "pattern_fullmatch"]:
            probe_match(draw, same(name, c, s, pos, endpos))
        same("pattern_findall", c, s, pos, endpos)
        same("pattern_finditer", c, s, pos)
        same("pattern_split", c, s)
        same("pattern_subn", c, draw(rs.template()), s)


@given(st.data())
def test_nested_argument_validation(data):
    """Every argument, positional or keyword, is checked recursively for non-builtin values."""
    from decimal import Decimal
    draw = data.draw
    p = draw(rs.valid_pattern())
    s = draw(rs.subject_for(p))
    bad = draw(st.sampled_from([Decimal("1"), [Decimal("1")], {"k": Decimal("2")}, {Decimal("3"): 1},
                                (1, [None, {"x": (Decimal("4"),)}]), frozenset([Decimal("5")]), [1, (2, {3: [4]})]]))
    same("search", p, s, flags=bad)
    same("compile", p, bad)
    same("split", p, s, maxsplit=bad)
    same("sub", p, "x", s, bad)
    same("subn", p, "x", s, count=bad)
    c = same("compile", p)
    if isinstance(c, Pair):
        same("pattern_search", c, s, bad)
        same("pattern_match", c, s, endpos=bad)
        same("pattern_split", c, s, bad)
        same("pattern_sub", c, "x", s, count=bad)
        m = same("pattern_search", c, s)
        if isinstance(m, Pair):
            same("match_group", m, 0, bad)
            same("match_groups", m, bad)
            same("match_groupdict", m, default=bad)
            same("match_start", m, bad)
