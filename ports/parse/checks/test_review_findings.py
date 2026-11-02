"""Checks for behavioural findings from the Rust review.

#1 deeply nested custom-type patterns: upstream raises RecursionError (from 493
   nested groups on), the port must not match or crash.
#2 lone surrogates in the string being parsed must work like any character.
#3 Parser instances accept arbitrary attribute assignment like a plain class.
"""
from hypothesis import example, given, strategies as st
from portwright.compare import Pair, PythonExpression, same
from portwright.compare.strategies import text

from pw_helpers import all_entry_points, inspect_result

# ---------------------------------------------------------------- 1. nesting depth
OPENERS = ["(", "(?:", "(?=a)(", "(a|", "(?!b)(?:"]


def nested_converter(depth, opener):
    pattern = opener * depth + "a" + ")" * depth
    return PythonExpression(
        "__import__('parse').with_pattern(%r, regex_group_count=0)(lambda s: s)" % pattern
    )


def run_nested(depth, opener):
    et = {"f": nested_converter(depth, opener)}
    inspect_result(same("parse", "{:f}", "a", et))
    inspect_result(same("search", "x{:f}", "xa", extra_types=et))
    same("findall", "{:f}", "aa", extra_types=et)
    p = same("compile", "{:f}", et)
    if isinstance(p, Pair):
        inspect_result(p.call("parse", "a"))
        inspect_result(p.call("search", "a"))


@given(st.one_of(st.integers(0, 480), st.integers(505, 4000)), st.sampled_from(OPENERS))
@example(300, "(")
@example(492, "(")
@example(493, "(")
@example(500, "(")
@example(500, "(?:")
@example(3000, "(")
@example(20000, "(")
@example(20000, "(?:")
@example(20000, "(a|")
def test_nested_custom_pattern(depth, opener):
    run_nested(depth, opener)


def test_nested_with_pattern_decorator():
    # The reviewer's exact repro shape, via the with_pattern API itself.
    for n in (300, 500, 3000, 20000):
        deco = same("with_pattern", "(" * n + "a" + ")" * n)
        fn = deco.call("__call__", PythonExpression("lambda s: s"))
        inspect_result(same("parse", "{:f}", "a", {"f": fn}))


# ---------------------------------------------------------------- 2. surrogates
SURR = st.sampled_from(["\ud800", "\udfff", "\udc80", "\ud83d", "\ude00", "\udcff"])


@given(text(max_size=12))
@example("\ud800")
@example("a\udc80b")
@example("😀")
@example("12\udfff")
def test_surrogate_plain(s):
    all_entry_points("{}", s)
    all_entry_points("{x}", s)
    inspect_result(same("search", "{}", s))


@given(SURR, text(max_size=6), st.sampled_from(["{} {}", "{:d}{}", "{a}-{b}", "<{:w}>", "{:^}", "{}{:l}"]))
@example("\ud800", "x", "{} {}")
@example("\udc80", "", "{a}-{b}")
def test_surrogate_structured(sur, s, fmt):
    for string in [sur + " " + s, s + "-" + sur, "12" + sur, "<" + sur + ">", sur + s + "z"]:
        all_entry_points(fmt, string)


@given(text(max_size=4), text(max_size=4))
@example("\ud800", "lit")
def test_surrogate_in_format_and_string(lit, s):
    # Surrogates in literal format text as well as the matched string.
    fmt = lit.replace("{", "{{").replace("}", "}}") + "{}"
    all_entry_points(fmt, lit + s)
    all_entry_points(fmt, lit.upper() + s, case_sensitive=True)


# ---------------------------------------------------------------- 3. Parser attributes
@given(st.sampled_from(["foo", "bar_", "x1", "format", "named_fields"]),
       st.one_of(st.integers(), text(max_size=4), st.none()))
@example("foo", 1)
def test_parser_setattr(name, value):
    p = same("compile", "{} {x:d}")
    p.call("__setattr__", name, value)
    p.get(name)
    p.call("__delattr__", name)
    p.get(name)
    inspect_result(p.call("parse", "a 5"))


def test_parser_attr_missing():
    p = same("compile", "{}")
    p.get("foo")
    p.call("__setattr__", "foo", [1, 2])
    p.get("foo")
    p.call("__getattribute__", "foo")
