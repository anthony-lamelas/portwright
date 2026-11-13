"""re compared with the real module: matches, groups, errors, substitution and splitting."""

import itertools
import re

import pytest

import pc_re as pc


def describe_match(m, port):
    """Everything observable about a match, read through the port's functions or re's methods."""
    if m is None:
        return None
    if port:
        return (type(m).__name__, repr(m), pc.match_span(m), pc.match_groups(m), pc.match_groupdict(m),
                pc.match_lastindex(m), pc.match_lastgroup(m), pc.match_group(m, 0), pc.match_pos(m), pc.match_endpos(m))
    return (type(m).__name__, repr(m), m.span(), m.groups(), m.groupdict(), m.lastindex, m.lastgroup, m.group(0),
            m.pos, m.endpos)


def outcome(func, *args, port=False):
    try:
        value = func(*args)
    except Exception as exc:
        details = (getattr(exc, "msg", None), getattr(exc, "pos", None), getattr(exc, "lineno", None), getattr(exc, "colno", None))
        return ("raised", type(exc).__name__, str(exc), details)
    if isinstance(value, list) and value and all(type(v).__name__ == "Match" for v in value):
        return ("returned", [describe_match(m, port) for m in value])
    if value is None or type(value).__name__ == "Match":
        return ("returned", describe_match(value, port))
    return ("returned", type(value).__name__, repr(value))


def same(port_func, builtin, *args):
    assert outcome(port_func, *args, port=True) == outcome(builtin, *args), args


PATTERNS = [
    "a", "abc", "a*", "a+", "a?", "a*?", "a+?", "a??", "a{2}", "a{2,}", "a{,2}", "a{1,3}", "a{1,3}?", "a{3,1}",
    "a*+", "a++", "a?+", "(?>a+)b", "(?>a|ab)c", ".", ".*", "^a", "a$", "^$", r"\Aa", r"a\Z", r"\ba", r"a\b", r"\Ba",
    "[abc]", "[^abc]", "[a-z]+", "[a-]", "[]a]", "[^]a]", r"[\d\s]", r"[\w-]", r"\d+", r"\D", r"\s+", r"\S", r"\w+",
    r"\W", "(a)(b)?", "(a|b)*c", "(?:ab)+", "(?P<x>a)(?P=x)", r"(a)\1", r"(a)(b)\2\1", "(?P<first>\\w+) (?P<last>\\w+)",
    "(?=a)a", "(?!a).", "(?<=a)b", "(?<!a)b", "(?<=ab|cd)x", "(a)?(?(1)b|c)", "(?P<g>a)?(?(g)b|c)", "(?i)abc",
    "(?i:a)b", "(?-i:a)", "(?a)\\w+", "(?s).+", "(?m)^b", "(?x) a b # comment", "a|b|c", "a||b", "|", "(|a)+",
    "(a*)*", "(a*)+b", "(a|)*", "(?:a?)*?b", r"\x41", r"é", r"\U0001F600", r"\N{EM DASH}", r"\0", r"\07",
    r"\101", r"\t\n\r\f\v", r"\.", r"\\", "é+", "Σ", "ß", "ſ", "K", "İ", "[à-ÿ]+", "\\$", "x*y*z*", "((a)|b)+",
    "(a)|(b)", "(?P<a>x)|(?P<b>y)", "(.)(.)(.)(.)(.)(.)(.)(.)(.)(.)(.)", r"(?P<n>\d)(?P=n)", "[[:alpha:]]",
    "[a-z&&b]", "[a--b]", "[a||b]", "[~~a]",
]
SUBJECTS = ["", "a", "aa", "aaa", "ab", "abc", "ABC", "aBc", "ba", "cab", "b", "c", "x", "xy", "abab", "aab", "aaab",
            "hello world", "John Smith", "11", "1 2 3", "a\nb\nc", "é", "ÉÉ", "Σσς", "ss ß SS", "ſ s S", "K k K",
            "İi", "—", "😀", "a.b", "$", "\\", "\t\n", "x\ud800y", "\x00", "AAA", "c b a"]


@pytest.mark.parametrize("pattern", PATTERNS)
def test_search_match_fullmatch(pattern):
    for subject, flags in itertools.product(SUBJECTS, [0, re.IGNORECASE]):
        same(pc.search, re.search, pattern, subject, flags)
        same(pc.match, re.match, pattern, subject, flags)
        same(pc.fullmatch, re.fullmatch, pattern, subject, flags)


@pytest.mark.parametrize("pattern", PATTERNS[::3])
def test_findall_finditer_split(pattern):
    for subject in SUBJECTS:
        same(pc.findall, re.findall, pattern, subject)
        same(pc.finditer, lambda p, s: list(re.finditer(p, s)), pattern, subject)
        for maxsplit in (0, 1):
            same(pc.split, re.split, pattern, subject, maxsplit)


def test_flags_combinations():
    flag_sets = [0, re.I, re.M, re.S, re.X, re.A, re.I | re.A, re.I | re.M | re.S, re.U, re.L, re.A | re.U, -1, 2**40]
    for pattern, flags in itertools.product(["^a.", "a$", r"\w+", "(?i)b", "[k]", "s", " a # x", "(?u)x"], flag_sets):
        for subject in ["a\nA\nb", "K", "ſ", "ﬀ", "x b", "É"]:
            same(pc.search, re.search, pattern, subject, flags)


ERRORS = ["(", ")", "a)", "(a", "[", "[a", "[]", "[a-", "[z-a]", "*", "+a", "a**", "a{2}{3}", "?", "(?", "(?P", "(?P<",
          "(?P<1>a)", "(?P<a>x)(?P<a>y)", "(?P=a)", r"\1", r"(a)\2", r"\q", r"\x4", r"\u12", r"\U0011FFFF", r"\N{NOPE}",
          r"\N", "(?<=a+)b", "(?<=a|bc)d", "(?z)", "(?i", "a(?i)", "(?-x:a)", "(?ix-i:a)", "(?(1)a|b)", "(?(x)a|b)",
          "(?(1)a|b|c)", "(a)(?(1)a|b|c)", "\\", "x{1,2", r"\8", "[\\d-z]", "a{4294967296}", "(?#unterminated",
          "(?P<a>a)(?P=b)", "(?i)(?a)\\w(?u)", "(?au)", "(?L)", "(?<)", "(?<=)", "(?a:(?u:x))", "(?L:x)"]


@pytest.mark.parametrize("pattern", ERRORS)
def test_compile_errors(pattern):
    same(lambda p: pc.pattern_groups(pc.compile(p)), lambda p: re.compile(p).groups, pattern)


REPLACEMENTS = [r"x", r"\1", r"\g<0>", r"\g<1>", r"\g<n>", r"[\g<n>]", r"\\", r"\n", r"\t", r"\q", r"\g<", r"\g<a",
                r"\g<-1>", r"\g<99>", r"\2", r"\0", r"\07", r"\101", r"\g<1>0", r"\10", "$1", r"\N{EM DASH}", "é"]


def test_sub_and_subn():
    for pattern, repl in itertools.product(["(?P<n>a)", "(a)|b", "(a)(b)?", "x*", "", "(?P<n>)b"], REPLACEMENTS):
        for subject, count in itertools.product(["", "a", "ab", "aXbYa", "bbb", "xyx"], [0, 1, 2, -1]):
            same(pc.sub, re.sub, pattern, repl, subject, count)
            same(pc.subn, re.subn, pattern, repl, subject, count)


def test_pattern_methods_with_pos_and_endpos():
    for pattern in ["a", "^a", "a$", r"\b\w", "(?<=a)b", r"\Bb", ""]:
        compiled_port, compiled = pc.compile(pattern), re.compile(pattern)
        for subject, pos, endpos in itertools.product(["ab", "aab", "ba b"], [-5, 0, 1, 2, 9], [None, -1, 0, 1, 3, 99]):
            assert outcome(pc.pattern_search, compiled_port, subject, pos, endpos, port=True) == outcome(
                lambda p, s, a, b: p.search(s, a) if b is None else p.search(s, a, b), compiled, subject, pos, endpos)
            assert outcome(pc.pattern_findall, compiled_port, subject, pos, endpos, port=True) == outcome(
                lambda p, s, a, b: p.findall(s, a) if b is None else p.findall(s, a, b), compiled, subject, pos, endpos)


def test_pattern_attributes():
    for pattern, flags in itertools.product(["a", "(?P<x>a)(b)", "(?i)a", "(?x) a", "(a)(?P<y>b)(c)"], [0, re.I, re.M | re.S]):
        port, real = pc.compile(pattern, flags), re.compile(pattern, flags)
        assert (pc.pattern_text(port), pc.pattern_flags(port), pc.pattern_groups(port), pc.pattern_groupindex(port), repr(port)) == (
            real.pattern, real.flags, real.groups, dict(real.groupindex), repr(real))


def test_match_accessors():
    cases = [("(?P<a>x)(y)?(?P<c>z)?", "xz"), ("(a)(b)(c)", "abc"), ("(a)|(b)", "b"), ("(?P<only>q)", "q"), ("", "")]
    for pattern, subject in cases:
        port_m, real_m = pc.search(pattern, subject), re.search(pattern, subject)
        for group in [0, 1, 2, 3, 4, -1, "a", "c", "only", "nope", 1.0]:
            for name in ("group", "start", "end", "span"):
                assert outcome(getattr(pc, f"match_{name}"), port_m, group, port=True) == outcome(
                    getattr(type(real_m), name), real_m, group), (pattern, group, name)
        if real_m.re.groups:
            assert outcome(pc.match_group, port_m, 0, 1, port=True) == outcome(real_m.group, 0, 1)
        for default in (None, "", "-"):
            assert pc.match_groups(port_m, default) == real_m.groups(default)
            assert pc.match_groupdict(port_m, default) == real_m.groupdict(default)
        for template in [r"\1", r"\g<0>", "[\\g<a>]", r"\9", "plain", r"\g<x"]:
            assert outcome(pc.match_expand, port_m, template, port=True) == outcome(real_m.expand, template)
        assert pc.match_string(port_m) == real_m.string


def test_escape():
    for text in ["", "a", "a.b", "a b", "\t\n", "#&~", "-_/", "é", "😀", "\x00", "[]{}()*+?^$|\\", "".join(map(chr, range(128)))]:
        same(pc.escape, re.escape, text)


def test_non_str_arguments_are_rejected():
    """Only str patterns are in scope. These messages come from the shim, not from re."""
    for args, message in [((b"a", "a"), "pattern must be str, not bytes"), (("a", b"a"), "string must be str, not bytes"),
                          ((1, "a"), "pattern must be str, not int"), (("a", None), "string must be str, not NoneType")]:
        with pytest.raises(TypeError) as caught:
            pc.search(*args)
        assert str(caught.value) == message


PATTERN_FUNCTIONS = [
    (pc.pattern_search, ("a",)), (pc.pattern_match, ("a",)), (pc.pattern_fullmatch, ("a",)),
    (pc.pattern_findall, ("a",)), (pc.pattern_finditer, ("a",)), (pc.pattern_sub, ("x", "a")),
    (pc.pattern_subn, ("x", "a")), (pc.pattern_split, ("a",)), (pc.pattern_text, ()), (pc.pattern_flags, ()),
    (pc.pattern_groups, ()), (pc.pattern_groupindex, ()),
]
MATCH_FUNCTIONS = [
    (pc.match_group, ()), (pc.match_groups, ()), (pc.match_groupdict, ()), (pc.match_start, ()), (pc.match_end, ()),
    (pc.match_span, ()), (pc.match_expand, ("x",)), (pc.match_lastindex, ()), (pc.match_lastgroup, ()),
    (pc.match_pos, ()), (pc.match_endpos, ()), (pc.match_string, ()),
]


def test_pattern_and_match_arguments_are_type_checked():
    """Review: a str ``p`` ran ``str.split`` and a Match ``p`` gave a bound method. Messages come from the shim."""
    pattern, match = pc.compile("(a)"), pc.search("(a)", "a")
    for func, rest in PATTERN_FUNCTIONS:
        func(pattern, *rest)
        for bad in ["a", match, None, 1, ["a"], re.I]:
            with pytest.raises(TypeError) as caught:
                func(bad, *rest)
            assert str(caught.value) == f"p must be a compiled pattern, not {type(bad).__name__}", (func.__name__, bad)
    for func, rest in MATCH_FUNCTIONS:
        func(match, *rest)
        for bad in [pattern, "a", None, (0, 1), 0]:
            with pytest.raises(TypeError) as caught:
                func(bad, *rest)
            assert str(caught.value) == f"m must be a match object, not {type(bad).__name__}", (func.__name__, bad)
    with pytest.raises(TypeError, match="^p must be a compiled pattern, not str$"):
        pc.pattern_split("a,b,,c", ",", 2)
    with pytest.raises(TypeError, match="^p must be a compiled pattern, not Match$"):
        pc.pattern_groups(match)
    # The pattern is checked before the string, as ``p.split`` is looked up before its arguments are evaluated.
    with pytest.raises(TypeError, match="^p must be a compiled pattern, not int$"):
        pc.pattern_split(1, b"a")
    with pytest.raises(TypeError, match="^string must be str, not bytes$"):
        pc.pattern_split(pattern, b"a")
