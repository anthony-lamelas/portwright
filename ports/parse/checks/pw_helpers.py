"""Shared helpers for the parse checks (not a test module)."""

from portwright.compare import Pair, PythonExpression, same


def inspect_result(r):
    """Compare every observable part of a Result / Match pair."""
    if not isinstance(r, Pair):
        return r
    if r._api == "Result":
        r.get("fixed")
        r.get("named")
        r.get("spans")
    elif r._api == "Match":
        res = r.call("evaluate_result")
        inspect_result(res)
    return r


def re_match(pattern, string, flags="re.I|re.S", fn="match"):
    """A PythonExpression that builds a real ``re.Match`` on each side."""
    return PythonExpression(
        "(lambda re: re.compile(%r, %s).%s(%r))(__import__('re'))" % (pattern, flags, fn, string)
    )


def converter(body, pattern=None, group_count=None):
    """A PythonExpression for a type converter, optionally with pattern attrs."""
    if pattern is None:
        return PythonExpression("lambda s: %s" % body)
    attrs = "f.pattern = %r" % pattern
    expr = "(lambda f: (setattr(f, 'pattern', %r), setattr(f, 'regex_group_count', %r), f)[2])(lambda s: %s)" % (
        pattern, group_count, body)
    del attrs
    return PythonExpression(expr)


def all_entry_points(fmt, string, case_sensitive=None, extra_types=None):
    """Run fmt/string through parse, search, findall and a compiled Parser."""
    kw = {}
    if case_sensitive is not None:
        kw["case_sensitive"] = case_sensitive
    if extra_types is not None:
        kw["extra_types"] = extra_types
    inspect_result(same("parse", fmt, string, **kw))
    inspect_result(same("search", fmt, string, **kw))
    same("findall", fmt, string, **kw)
    p = same("compile", fmt, **kw)
    if isinstance(p, Pair):
        p.get("named_fields")
        p.get("fixed_fields")
        p.get("format")
        inspect_result(p.call("parse", string))
        inspect_result(p.call("parse", string, evaluate_result=False))
        inspect_result(p.call("search", string, evaluate_result=False))
        p.call("findall", string, evaluate_result=False)
    return p
