"""Python's ``re`` module for str patterns, as plain functions.

Each function calls ``re``, so CPython 3.12's regex engine (sre) is the reference.
Pattern and Match methods are exposed as functions taking the object first, so every
one of them is part of the API a port must match. Replacements are template strings;
callables aren't supported, because a port of the built-ins can't call back into Python.

Arguments must be core built-in values: None, bool, int, float, str, bytes, bytearray,
and lists, tuples, sets, frozensets and dicts of them (plus re flags, and compiled
patterns and matches from this package). Anything else, such as a
Decimal, raises TypeError, because its behavior comes from its own module.
The ``p`` of ``pattern_*`` functions must be a compiled pattern and the ``m`` of
``match_*`` functions a match object. Anything else raises the error the method lookup
would (AttributeError, or TypeError for a Pattern's data attribute), from a fixed table
and without touching the object.
"""

import re as _re

import functools as _functools

__all__ = [
    "compile", "search", "match", "fullmatch", "findall", "finditer", "sub", "subn", "split", "escape",
    "pattern_search", "pattern_match", "pattern_fullmatch", "pattern_findall", "pattern_finditer",
    "pattern_sub", "pattern_subn", "pattern_split", "pattern_text", "pattern_flags", "pattern_groups",
    "pattern_groupindex",
    "match_group", "match_groups", "match_groupdict", "match_start", "match_end", "match_span",
    "match_expand", "match_lastindex", "match_lastgroup", "match_pos", "match_endpos", "match_string",
    "error", "NOFLAG", "ASCII", "IGNORECASE", "LOCALE", "MULTILINE", "DOTALL", "VERBOSE", "UNICODE",
]

_BUILTIN_TYPES = (type(None), bool, int, float, str, bytes, bytearray, list, tuple, set, frozenset, dict, _re.RegexFlag, _re.Pattern, _re.Match)


def _check_builtin(value):
    if type(value) not in _BUILTIN_TYPES:
        raise TypeError(f"pc_re supports built-in types only, not {type(value).__name__}")
    if type(value) is dict:
        for key, item in value.items():
            _check_builtin(key)
            _check_builtin(item)
    elif type(value) in (list, tuple, set, frozenset):
        for item in value:
            _check_builtin(item)


def _builtins_only(function):
    @_functools.wraps(function)
    def checked(*args, **kwargs):
        for value in (*args, *kwargs.values()):
            _check_builtin(value)
        return function(*args, **kwargs)
    return checked


error = _re.error
NOFLAG = int(_re.NOFLAG)
ASCII = int(_re.ASCII)
IGNORECASE = int(_re.IGNORECASE)
LOCALE = int(_re.LOCALE)
MULTILINE = int(_re.MULTILINE)
DOTALL = int(_re.DOTALL)
VERBOSE = int(_re.VERBOSE)
UNICODE = int(_re.UNICODE)


def _text(value, what):
    if not isinstance(value, str):
        raise TypeError(f"{what} must be str, not {type(value).__name__}")
    return value


# Type names as CPython prints them in attribute errors. RegexFlag counts as int, as the
# port converts it. Every other admitted type is a built-in, named by its __name__.
_TYPE_NAMES = {_re.Pattern: "re.Pattern", _re.Match: "re.Match", _re.RegexFlag: "int"}
# Pattern's data attributes that share a name with a Match method: calling one raises
# TypeError for the attribute's type instead of AttributeError.
_PATTERN_DATA = {"groups": "int", "flags": "int", "pattern": "str", "groupindex": "mappingproxy"}


def _no_attribute(value, attr):
    name = _TYPE_NAMES.get(type(value), type(value).__name__)
    return AttributeError(f"'{name}' object has no attribute '{attr}'")


def _pattern(value, method):
    """``value`` if it's a compiled pattern, else the error ``value.method`` would raise.

    The error is built from a fixed table, without touching ``value``: a str ``p`` would
    otherwise run ``str.split``, and a Match ``p`` would hand back a bound method.
    """
    if type(value) is not _re.Pattern:
        raise _no_attribute(value, method)
    return value


def _match(value, method):
    """``value`` if it's a match object, else the error ``value.method(...)`` would raise."""
    if type(value) is not _re.Match:
        if type(value) is _re.Pattern and method in _PATTERN_DATA:
            raise TypeError(f"'{_PATTERN_DATA[method]}' object is not callable")
        raise _no_attribute(value, method)
    return value


def _window(p, method, string, pos, endpos):
    _text(string, "string")
    _pattern(p, method)
    return getattr(p, method)(string, pos) if endpos is None else getattr(p, method)(string, pos, endpos)


@_builtins_only
def compile(pattern, flags=0):  # noqa: A001 - mirrors re.compile
    """``re.compile(pattern, flags)``."""
    return _re.compile(_text(pattern, "pattern"), flags)


@_builtins_only
def search(pattern, string, flags=0):
    return _re.search(_text(pattern, "pattern"), _text(string, "string"), flags)


@_builtins_only
def match(pattern, string, flags=0):
    return _re.match(_text(pattern, "pattern"), _text(string, "string"), flags)


@_builtins_only
def fullmatch(pattern, string, flags=0):
    return _re.fullmatch(_text(pattern, "pattern"), _text(string, "string"), flags)


@_builtins_only
def findall(pattern, string, flags=0):
    return _re.findall(_text(pattern, "pattern"), _text(string, "string"), flags)


@_builtins_only
def finditer(pattern, string, flags=0):
    """``list(re.finditer(...))``: a list, so errors surface at the call."""
    return list(_re.finditer(_text(pattern, "pattern"), _text(string, "string"), flags))


@_builtins_only
def sub(pattern, repl, string, count=0, flags=0):
    return _re.sub(_text(pattern, "pattern"), _text(repl, "repl"), _text(string, "string"), count, flags)


@_builtins_only
def subn(pattern, repl, string, count=0, flags=0):
    return _re.subn(_text(pattern, "pattern"), _text(repl, "repl"), _text(string, "string"), count, flags)


@_builtins_only
def split(pattern, string, maxsplit=0, flags=0):
    return _re.split(_text(pattern, "pattern"), _text(string, "string"), maxsplit, flags)


@_builtins_only
def escape(pattern):
    return _re.escape(_text(pattern, "pattern"))


@_builtins_only
def pattern_search(p, string, pos=0, endpos=None): return _window(p, "search", string, pos, endpos)
@_builtins_only
def pattern_match(p, string, pos=0, endpos=None): return _window(p, "match", string, pos, endpos)
@_builtins_only
def pattern_fullmatch(p, string, pos=0, endpos=None): return _window(p, "fullmatch", string, pos, endpos)
@_builtins_only
def pattern_findall(p, string, pos=0, endpos=None): return _window(p, "findall", string, pos, endpos)
@_builtins_only
def pattern_finditer(p, string, pos=0, endpos=None): return list(_window(p, "finditer", string, pos, endpos))


@_builtins_only
def pattern_sub(p, repl, string, count=0):
    return _pattern(p, "sub").sub(_text(repl, "repl"), _text(string, "string"), count)


@_builtins_only
def pattern_subn(p, repl, string, count=0):
    return _pattern(p, "subn").subn(_text(repl, "repl"), _text(string, "string"), count)


@_builtins_only
def pattern_split(p, string, maxsplit=0):
    string = _text(string, "string")
    return _pattern(p, "split").split(string, maxsplit)


@_builtins_only
def pattern_text(p): return _pattern(p, "pattern").pattern
@_builtins_only
def pattern_flags(p): return _pattern(p, "flags").flags
@_builtins_only
def pattern_groups(p): return _pattern(p, "groups").groups
@_builtins_only
def pattern_groupindex(p): return dict(_pattern(p, "groupindex").groupindex)


@_builtins_only
def match_group(m, *groups): return _match(m, "group").group(*groups)
@_builtins_only
def match_groups(m, default=None): return _match(m, "groups").groups(default)
@_builtins_only
def match_groupdict(m, default=None): return _match(m, "groupdict").groupdict(default)
@_builtins_only
def match_start(m, group=0): return _match(m, "start").start(group)
@_builtins_only
def match_end(m, group=0): return _match(m, "end").end(group)
@_builtins_only
def match_span(m, group=0): return _match(m, "span").span(group)
@_builtins_only
def match_expand(m, template): return _match(m, "expand").expand(_text(template, "template"))
@_builtins_only
def match_lastindex(m): return _match(m, "lastindex").lastindex
@_builtins_only
def match_lastgroup(m): return _match(m, "lastgroup").lastgroup
@_builtins_only
def match_pos(m): return _match(m, "pos").pos
@_builtins_only
def match_endpos(m): return _match(m, "endpos").endpos
@_builtins_only
def match_string(m): return _match(m, "string").string
