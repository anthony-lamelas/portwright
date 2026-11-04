"""Checks for review findings.

#4: the port raises RecursionError at a fixed nesting depth tuned to one caller. Probe
nesting depths around CPython's cutoff with mixed constructs, through each entry point
(module functions, compile, with and without flags), and mixed with other errors.
#1: DEBUG (flag 128) stdout is out of scope, but results/errors of DEBUG patterns must match
through every API, not only compile.
"""

from hypothesis import given, example, settings, strategies as st
from portwright.compare import same, Pair
from portwright.compare.strategies import text

OPENERS = ["(", "(?:", "(?=", "(?!", "(?<=", "(?<!", "(?>", "(?i:", "(?s-i:", "(?x:", "(?P<g{}>", "(?(1)"]
SUFFIXES = ["", "", "*", "+?", "{2}", "?+", "|b", "|", "*+"]


def nested(openers, core="a", suffix=""):
    out = []
    for i, o in enumerate(openers):
        out.append(o.format(i))
    body = "".join(out) + core
    return body + (")" + suffix) * len(openers)


@st.composite
def deep_pattern(draw):
    n = draw(st.one_of(st.integers(470, 520), st.integers(485, 500), st.integers(1, 1200)))
    kinds = draw(st.lists(st.sampled_from(OPENERS), min_size=1, max_size=4))
    openers = [draw(st.sampled_from(kinds)) for _ in range(n)]
    if "(?(1)" in openers:
        openers = ["("] + openers  # conditionals need group 1 to exist
    core = draw(st.sampled_from(["a", "", "ab|c", "[a-c]", "\\d", "a*", "\\1", "(?P=g0)", "x" + ")"]))
    suffix = draw(st.sampled_from(SUFFIXES))
    p = nested(openers, core, suffix)
    if draw(st.booleans()):
        p = draw(st.sampled_from(["", "a", "(", "x|", "(?:b)"])) + p + draw(st.sampled_from(["", "b", ")", "{", "|c"]))
    return p


DEEP_EXAMPLES = [nested(["("] * n) for n in (493, 494, 495, 496, 497, 498, 500)] + [
    nested(["(?:"] * 494), nested(["(?:"] * 495), nested(["(?:"] * 496),
    nested(["(?<="] * 495), nested(["(?>"] * 496), nested(["(?P<g{}>"] * 495),
    nested(["("] + ["(?(1)"] * 495), nested(["("] * 495, "a", "*"), nested(["("] * 600),
    nested(["("] * 494, "a)"), "(" * 496 + "a", "(" * 495 + "a",
]


@given(deep_pattern(), st.sampled_from([0, 2, 128, 64, 2 | 128, 256]))
@example("(" * 494 + "a" + ")" * 494, 0)
@example("(" * 495 + "a" + ")" * 495, 0)
@example("(" * 496 + "a" + ")" * 496, 0)
@example("(" * 497 + "a" + ")" * 497, 128)
@example("(?:" * 496 + "a" + ")" * 496, 2)
@example("(" * 600 + "a" + ")" * 600, 0)
@example("(" * 496 + "a", 0)
@settings(max_examples=120, deadline=None)
def test_deep_nesting_compile(p, f):
    c = same("compile", p, f)
    if isinstance(c, Pair):
        same("pattern_groups", c)
        same("pattern_search", c, "ab" * 3)


@given(st.sampled_from(DEEP_EXAMPLES + [nested(["("] * n) for n in range(488, 502)]),
       st.sampled_from(["search", "match", "fullmatch", "findall", "split", "finditer"]),
       st.sampled_from(["a", "aab", "", "b"]))
@settings(max_examples=150, deadline=None)
def test_deep_nesting_module_functions(p, api, s):
    same(api, p, s)
    same(api, p, s, flags=2)


@given(st.sampled_from(DEEP_EXAMPLES), st.sampled_from(["x", "\\g<0>", "\\1", "\\g<400>"]), st.sampled_from(["a", "aaa", "ba"]))
@settings(max_examples=60, deadline=None)
def test_deep_nesting_sub(p, repl, s):
    same("sub", p, repl, s)
    same("subn", p, repl, s, count=1)


@given(deep_pattern(), st.sampled_from([0, 128]))
@settings(max_examples=60, deadline=None)
def test_deep_nesting_repeat(p, f):
    # Same pattern twice: cache hits must give the same outcome as the first compile.
    same("search", p, "aa", f)
    same("search", p, "aa", f)
    same("compile", p, f)


# ---- DEBUG flag (#1): only stdout differs; all else must match ----

DEBUG_PATTERNS = ["a", "(a)(b)?", "(?P<x>a+)|b", "[a-z]+", "(a", "a{2,1}", "(?i)A", "\\bfoo\\b", "x*", "(?<=a)b", "(?(1)a|b)", "(a)(?(1)b|c)"]


@given(st.sampled_from(DEBUG_PATTERNS), text(max_size=12),
       st.sampled_from([128, 128 | 2, 128 | 8, 128 | 16, 128 | 64, 128 | 256, 128 | 4, 128 | 32, 128 | 1, 128 | 2 ** 40]))
@example("(a)(b)?", "ab", 128)
@example("(a", "a", 128)
@example("a", "a", 128 | 4)
@example("a", "a", 128 | 256 | 32)
@settings(max_examples=120, deadline=None)
def test_debug_flag_results(p, s, f):
    same("search", p, s, f)
    same("match", p, s, flags=f)
    same("fullmatch", p, s, f)
    same("findall", p, s, f)
    same("split", p, s, 0, f)
    same("sub", p, "<\\g<0>>", s, 0, f)
    same("subn", p, "-", s, flags=f)
    same("finditer", p, s, f)
    c = same("compile", p, f)
    if isinstance(c, Pair):
        same("pattern_flags", c)
        same("pattern_text", c)
        same("pattern_groups", c)
        same("pattern_groupindex", c)
        m = same("pattern_search", c, s)
        if isinstance(m, Pair):
            same("match_span", m)
            same("match_groups", m)
    # DEBUG compiles aren't cached: a second identical call must still match.
    same("compile", p, f)
    same("search", p, s, f)


# ---- DEBUG buffer leak (#1): the port buffers DEBUG text in a thread-local and flushes it
# per call. If a path forgets to flush (error raised mid-compile, RecursionError, a DEBUG
# pattern reached via an object), the text leaks into the *next* call's stdout. APIs that
# take no flags (escape, pattern_*, match_*) have no approved stdout difference, so call
# them right after each DEBUG call and compare their stdout.

LEAK_PATTERNS = DEBUG_PATTERNS + ["(?P<n>a)(?P=n)", "a{4294967295}", "a{2,3}+", "(?>a+)b", "\\", "[", "a**",
                                  "(?P<1>a)", "(?(2)a)", "(" * 600 + "a" + ")" * 600, "(" * 496 + "a" + ")" * 496]


def _after_debug(c_or_none, s):
    # Calls with no flags argument; stdout must be identical (normally empty) on both sides.
    same("escape", s)
    same("escape", "a.b")
    if isinstance(c_or_none, Pair):
        same("pattern_text", c_or_none)
        same("pattern_flags", c_or_none)
        m = same("pattern_search", c_or_none, s)
        same("pattern_findall", c_or_none, s)
        same("pattern_sub", c_or_none, "x", s)
        if isinstance(m, Pair):
            same("match_span", m)
            same("match_group", m, 0)
            same("match_expand", m, "[\\g<0>]")
    plain = same("compile", "(a)|b")
    if isinstance(plain, Pair):
        same("pattern_search", plain, s)


@given(st.sampled_from(LEAK_PATTERNS), text(max_size=10),
       st.sampled_from([128, 128 | 2, 128 | 64, 128 | 256, 128 | 4, 128 | 2 ** 40, 128 | 8 | 16]),
       st.sampled_from(["compile", "search", "match", "fullmatch", "findall", "split", "sub", "subn", "finditer"]))
@example("(a)(b)?", "ab", 128, "compile")
@example("(a", "a", 128, "compile")
@example("a{2,1}", "aa", 128, "search")
@example("(" * 600 + "a" + ")" * 600, "a", 128, "compile")
@example("(" * 496 + "a" + ")" * 496, "a", 128, "search")
@example("a", "a", 128 | 4, "compile")
@example("a", "a", 128 | 2 ** 40, "sub")
@example("a{4294967295}", "a", 128, "compile")
@settings(max_examples=150, deadline=None)
def test_debug_does_not_leak_into_next_call(p, s, f, api):
    if api == "compile":
        r = same("compile", p, f)
    elif api in ("sub", "subn"):
        r = same(api, p, "-", s, 0, f)
    elif api == "split":
        r = same(api, p, s, 0, f)
    else:
        r = same(api, p, s, f)
    c = r if api == "compile" else None
    _after_debug(c, s)
    # second DEBUG call (not cached), then flagless calls again
    same("compile", p, flags=f)
    _after_debug(c, s)


@given(st.lists(st.tuples(st.sampled_from(LEAK_PATTERNS[:14]), st.sampled_from([0, 128, 128 | 2])),
                min_size=1, max_size=5), text(max_size=8))
@example([("(a", 128), ("a", 0)], "a")
@example([("a", 128), ("a", 0), ("a", 128)], "aa")
@settings(max_examples=60, deadline=None)
def test_debug_interleaved_with_cached(seq, s):
    # Same pattern with and without DEBUG: the non-DEBUG one is cached, the DEBUG one never is.
    for p, f in seq:
        c = same("compile", p, f)
        same("search", p, s)
        if isinstance(c, Pair):
            same("pattern_search", c, s)
        same("escape", p)
