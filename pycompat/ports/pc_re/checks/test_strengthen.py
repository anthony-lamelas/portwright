"""Focused checks for engine/parser paths the broad grammar reaches only rarely.

Each test builds structured inputs for one mechanism: backreferences, scoped verbose
groups, bitmap/bigcharset sets, bounded lazy repeats, multi-char literal prefixes with a
window, ignore-case charset prefixes, and str()/repr of float messages.
"""

from hypothesis import given, strategies as st
from portwright.compare import same, Pair
from portwright.compare.strategies import text

import re_strategies as rs

AB = st.text(alphabet="aAbB", max_size=10)


def _probe(m):
    if isinstance(m, Pair):
        same("match_span", m)
        same("match_groups", m)


# ---------------------------------------------------------------- backreferences
GROUP_BODIES = ["a", "b", "a|b", "a*", "a+", "[ab]+", "[ab]*?", "(?:ab)?", "a?b?", "", "ab|a", "\\w", "."]
REF_FLAGS = ["", "(?i)", "(?a)(?i)", "(?s)", "(?i)(?s)"]


@st.composite
def backref_pattern(draw):
    n = draw(st.integers(1, 3))
    parts = []
    for k in range(1, n + 1):
        body = draw(st.sampled_from(GROUP_BODIES))
        q = draw(st.sampled_from(["", "", "?", "*", "{0,2}"]))
        if draw(st.booleans()):
            parts.append("(?P<g%d>%s)%s" % (k, body, q))
        else:
            parts.append("(%s)%s" % (body, q))
        for _ in range(draw(st.integers(0, 2))):
            r = draw(st.integers(1, k))
            ref = draw(st.sampled_from(["\\%d" % r, "(?:\\%d)" % r, "(?P=g%d)" % r if "g%d>" % r in "".join(parts)
                                        else "\\%d" % r]))
            parts.append(ref + draw(st.sampled_from(["", "", "*", "+", "?", "{2}"])))
        if draw(st.integers(0, 3)) == 0:
            parts.append(draw(st.sampled_from(["b", "a", "$", "\\b", "(?(%d)a|b)" % k])))
    return draw(st.sampled_from(REF_FLAGS)) + "".join(parts)


@given(backref_pattern(), AB)
def test_backreferences(p, s):
    s2 = s + s  # repeated subjects make references match
    for subject in (s, s2, s.swapcase() + s):
        _probe(same("search", p, subject))
        same("findall", p, subject)
    _probe(same("fullmatch", p, s2))


@given(st.sampled_from(["(a)\\1", "(a+)\\1", "(?i)(a)\\1", "(?i)(é|ß)\\1", "(?i)(σ)\\1", "(?ai)(k)\\1",
                        "(a)(b)\\2\\1", "(?P<x>a|b)(?P=x)+", "(a)?\\1", "(a)|\\1", "(a\\1)", "(a)\\1{2}",
                        "((a)\\2)+", "(?:(a)|b)\\1"]),
       st.sampled_from(["aa", "a", "AaA", "abba", "aab", "", "aaaa", "ÉéSSß", "Σσς", "Kkk", "baab", "bab"]))
def test_backreference_table(p, s):
    _probe(same("search", p, s))
    same("findall", p, s)


# ---------------------------------------------------------------- scoped verbose
VERBOSE_BODIES = [" a b ", "a # c\n b", " [ ] ", "a\\ b", " a{1, 2} ", "a | b", " \\# ", " (?: a ) ", "a#\n"]


@given(st.sampled_from(["(?x:%s)", "(?x:%s)c", "c(?x:%s)", "(?-x:%s)", "(?x:(?-x:%s))", "(?ix:%s)",
                        "(?x-i:%s)", "(?s-x:%s)", "(?x:(?i:%s))", "(?i:(?x:%s))"]),
       st.sampled_from(VERBOSE_BODIES), st.sampled_from(["", "(?x)", "(?i)"]),
       st.sampled_from(["ab", " a b ", "a b", "AB", "a", "b", "c ab", "abc", "a{1, 2}", "#", " #", " ] ", "a ",
                        "a\n", "a c\n b"]))
def test_scoped_verbose_groups(tmpl, body, prefix, s):
    p = prefix + (tmpl % body)
    _probe(same("search", p, s))
    same("findall", p, s)
    c = same("compile", p)
    if isinstance(c, Pair):
        same("pattern_flags", c)


# ---------------------------------------------------------------- charset bitmaps
def _render(c):
    return "\\x%02x" % c if c < 256 else "\\u%04x" % c if c < 0x10000 else "\\U%08x" % c


@st.composite
def wide_set(draw):
    """A set with several separate runs, so sre compiles it to CHARSET or BIGCHARSET."""
    hi = draw(st.sampled_from([128, 256, 256, 0x300, 0x10000, 0x110000]))
    items = []
    for _ in range(draw(st.integers(3, 12))):
        a = draw(st.integers(0, hi - 1))
        if draw(st.booleans()):
            b = min(hi - 1, a + draw(st.integers(0, 40)))
            items.append(_render(a) + "-" + _render(b))
        else:
            items.append(_render(a))
    if draw(st.integers(0, 3)) == 0:
        items.append(draw(st.sampled_from(["\\d", "\\s", "\\w", "\\W"])))
    neg = draw(st.sampled_from(["", "", "^"]))
    return "[" + neg + "".join(items) + "]"


LATIN1 = "".join(map(chr, range(256)))


@given(wide_set(), st.sampled_from(["", "(?i)", "(?a)", "(?ai)"]), st.data())
def test_charset_bitmaps(s, flag, data):
    p = flag + s
    same("findall", p, LATIN1)
    extra = data.draw(st.text(alphabet=st.characters(max_codepoint=0x400), max_size=20)) + data.draw(text(max_size=6))
    same("findall", p, extra)
    same("findall", p + "+", extra + LATIN1[::7])
    _probe(same("search", flag + "x" + s + "y", "x" + extra[:1] + "y"))


# ---------------------------------------------------------------- bounded lazy repeats
REP_ATOMS = ["a", "b", "ab", "(a|ab)", "(?:a|b)", "(?:ab|a)", "(a)", "(?:a|)", "(?:b?a)", "[ab]", "(?:aa|b)",
             "(?:a(?:b|))"]
REP_QUANTS = ["{0,1}?", "{1,2}?", "{2,3}?", "{0,2}?", "{1,3}?", "{2}?", "*?", "+?", "??", "{0,4}?", "{3,}?",
              "{1,2}", "{0,3}", "*", "+"]


@st.composite
def lazy_pattern(draw, depth=2):
    parts = []
    for _ in range(draw(st.integers(1, 3))):
        if depth > 0 and draw(st.integers(0, 2)) == 0:
            inner = draw(lazy_pattern(depth - 1))
            atom = draw(st.sampled_from(["(?:%s)", "(%s)", "(?:%s|b)"])) % inner
        else:
            atom = draw(st.sampled_from(REP_ATOMS))
        parts.append(atom + draw(st.sampled_from(REP_QUANTS)))
    parts.append(draw(st.sampled_from(["c", "b", "$", "bc", "(?:b|c)", "", "a$", "\\1" if "(" in "".join(parts)
                                       and "(?:" != "".join(parts)[:3] else "c"])))
    return "".join(parts)


@given(lazy_pattern(), st.text(alphabet="aabbc", max_size=12))
def test_lazy_bounded_repeats(p, s):
    for subject in (s, s + "c", "a" * len(s) + "bc"):
        _probe(same("search", p, subject))
        _probe(same("fullmatch", p, subject))
        same("findall", p, subject)


# ---------------------------------------------------------------- literal prefixes with a window
PREFIXES = ["ab", "aab", "abab", "abcabd", "aaa", "aba", "xyx", "éé", "a\ud800", "abc", "aaab"]


@given(st.sampled_from(PREFIXES), st.sampled_from(["", "", "c", "\\w", "[ab]", "(?:x|)", "$", "+", "*", "?"]),
       st.text(alphabet="abcxy", max_size=12), st.data())
def test_literal_prefix_window(prefix, tail, filler, data):
    draw = data.draw
    p = prefix + tail
    reps = draw(st.integers(1, 3))
    s = filler[: draw(st.integers(0, len(filler)))] + prefix * reps + filler + prefix[:-1]
    c = same("compile", p)
    if not isinstance(c, Pair):
        return
    for _ in range(3):
        pos = draw(st.integers(0, len(s)))
        endpos = draw(st.integers(0, len(s) + 1))
        _probe(same("pattern_search", c, s, pos, endpos))
        same("pattern_findall", c, s, pos, endpos)
        same("pattern_finditer", c, s, 0, endpos)
    same("pattern_findall", c, s)
    same("pattern_split", c, s)


# ---------------------------------------------------------------- ignore-case charset prefixes
IC_SETS = ["[a-c]", "[b-z]", "[x-z]", "[k]", "[s-t]", "[a]", "[ab]", "[0-9a-c]", "[ʰ-ʸ]", "[α-γ]", "[ä-ö]",
           "[\\u0100-\\u017f]", "[ſ]", "[\\U00010400-\\U0001044f]", "[\\U0001F600-\\U0001F64F]", "[0-9]", "[-]",
           "(?:a|b)", "(?:1|2)", "[^a-c]", "[\\u1e9e]", "[ß-ÿ]"]


@given(st.sampled_from(IC_SETS), st.sampled_from(["x", "", "\\d", "yz", "[xy]"]),
       st.sampled_from(["(?i)", "(?i)", "(?ai)", ""]),
       st.lists(st.sampled_from(["A", "B", "C", "a", "z", "Z", "X", "x", "y", "Y", "K", "K", "S", "ſ", "Α", "Γ",
                                 "β", "Ä", "Ö", "ö", "Ā", "ā", "\U00010400", "\U00010428", "1", "2", " ", "ẞ", "ß",
                                 "ʰ", "😀", "-"]), max_size=10).map("".join))
def test_ignorecase_charset_prefix(head, tail, flag, s):
    p = flag + head + tail
    for subject in (s, s + s.swapcase(), "zz" + s.upper() + "x"):
        _probe(same("search", p, subject))
        same("findall", p, subject)


# ---------------------------------------------------------------- float messages
FLOATS = st.one_of(
    st.floats(allow_nan=True, allow_infinity=True),
    st.sampled_from([1e16, 1e-5, 1.5e-5, 1e-4, 1.25e17, 1e22, 1e100, -1e-300, 5e-324, 1.7976931348623157e308,
                     2.5e16, -1e16, 0.0001, 0.00012, 123456789012345680.0, 1e15, 9999999999999998.0, -0.0, 0.1]),
)


@given(FLOATS, st.sampled_from(["plain", "list", "tuple", "dict"]))
def test_error_float_messages(f, how):
    msg = {"plain": f, "list": [f, 1.0], "tuple": (f,), "dict": {"k": f}}[how]
    same("error", msg)
    same("error", msg, None, None)


# Bodies whose alternatives overlap, so the body backtracks after a failed extra iteration
# and re-enters the lazy repeat; the repeat count must be restored exactly.
OVERLAP_BODIES = ["a|ab", "ab|a", "a|aa", "aa|a", "b|ba", "a|ab|abb", "(a|ab)", "(?:a|ab)(?:b|)", "a(?:b|)",
                  "(a)|(ab)", "[ab]|ab"]


@given(st.sampled_from(OVERLAP_BODIES), st.integers(0, 3), st.integers(0, 3),
       st.sampled_from(["d", "bd", "ad", "$", "(?:d|b$)", "\\1" ]),
       st.lists(st.sampled_from(["a", "ab", "aa", "b", "ba", "abb"]), max_size=5),
       st.sampled_from(["d", "", "bd", "ad", "c"]), st.sampled_from(["?", "?", ""]))
def test_lazy_repeat_count_restore(body, lo, extra, tail, pieces, end, lazy):
    p = "(?:%s){%d,%d}%s%s" % (body, lo, lo + extra, lazy, tail)
    s = "".join(pieces) + end
    _probe(same("search", p, s))
    _probe(same("match", p, s))
    same("findall", p, s)
    same("findall", "x" + p, "x" + s + "x" + s[1:])


FIXED_FLOATS = [1e16, 1e-5, 1.5e-5, 1e-4, 1.25e17, 1e22, 1e100, -1e-300, 5e-324, 1.7976931348623157e308, 2.5e16,
                -1e16, 0.0001, 0.00012, 1e15, -2.5e-7, 3e-10, 1.234e-50, float("nan"), float("-inf"), -0.0, 0.1]


@given(st.sampled_from(["plain", "list", "tuple"]))
def test_error_float_messages_fixed(how):
    """Every exponent form (positive/negative exponent, one/many digits) each run."""
    for f in FIXED_FLOATS:
        msg = {"plain": f, "list": [f], "tuple": (f, 2.0)}[how]
        same("error", msg)


IC_SUBJECTS = ["zzBx", "Ax", "CX", "Kx", "ẞx", "Sx", "ΑΓx", "Öx", "Ā1", "\U00010400x", "ſx", "B1", "zzYz", "ʰx"]


@given(st.sampled_from(["x", "", "\\d", "yz", "[xy]"]), st.sampled_from(["(?i)", "(?ai)"]))
def test_ignorecase_charset_prefix_table(tail, flag):
    """Search must not skip an upper-case start that only matches by case folding."""
    for head in IC_SETS:
        p = flag + head + tail
        for s in IC_SUBJECTS:
            _probe(same("search", p, s))
