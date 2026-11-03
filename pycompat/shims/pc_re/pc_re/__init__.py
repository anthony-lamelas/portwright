"""Python's ``re`` module for str patterns, as plain functions.

Each function calls ``re``, so CPython 3.12's regex engine (sre) is the reference.
Pattern and Match methods are exposed as functions taking the object first, so every
one of them is part of the API a port must match. Replacements are template strings;
callables aren't supported, because a port of the built-ins can't call back into Python.
"""

import re as _re

__all__ = [
    "compile", "search", "match", "fullmatch", "findall", "finditer", "sub", "subn", "split", "escape",
    "pattern_search", "pattern_match", "pattern_fullmatch", "pattern_findall", "pattern_finditer",
    "pattern_sub", "pattern_subn", "pattern_split", "pattern_text", "pattern_flags", "pattern_groups",
    "pattern_groupindex",
    "match_group", "match_groups", "match_groupdict", "match_start", "match_end", "match_span",
    "match_expand", "match_lastindex", "match_lastgroup", "match_pos", "match_endpos", "match_string",
    "error", "NOFLAG", "ASCII", "IGNORECASE", "LOCALE", "MULTILINE", "DOTALL", "VERBOSE", "UNICODE",
]

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


def _window(p, method, string, pos, endpos):
    _text(string, "string")
    return getattr(p, method)(string, pos) if endpos is None else getattr(p, method)(string, pos, endpos)


def compile(pattern, flags=0):  # noqa: A001 - mirrors re.compile
    """``re.compile(pattern, flags)``."""
    return _re.compile(_text(pattern, "pattern"), flags)


def search(pattern, string, flags=0):
    return _re.search(_text(pattern, "pattern"), _text(string, "string"), flags)


def match(pattern, string, flags=0):
    return _re.match(_text(pattern, "pattern"), _text(string, "string"), flags)


def fullmatch(pattern, string, flags=0):
    return _re.fullmatch(_text(pattern, "pattern"), _text(string, "string"), flags)


def findall(pattern, string, flags=0):
    return _re.findall(_text(pattern, "pattern"), _text(string, "string"), flags)


def finditer(pattern, string, flags=0):
    """``list(re.finditer(...))``: a list, so errors surface at the call."""
    return list(_re.finditer(_text(pattern, "pattern"), _text(string, "string"), flags))


def sub(pattern, repl, string, count=0, flags=0):
    return _re.sub(_text(pattern, "pattern"), _text(repl, "repl"), _text(string, "string"), count, flags)


def subn(pattern, repl, string, count=0, flags=0):
    return _re.subn(_text(pattern, "pattern"), _text(repl, "repl"), _text(string, "string"), count, flags)


def split(pattern, string, maxsplit=0, flags=0):
    return _re.split(_text(pattern, "pattern"), _text(string, "string"), maxsplit, flags)


def escape(pattern):
    return _re.escape(_text(pattern, "pattern"))


def pattern_search(p, string, pos=0, endpos=None): return _window(p, "search", string, pos, endpos)
def pattern_match(p, string, pos=0, endpos=None): return _window(p, "match", string, pos, endpos)
def pattern_fullmatch(p, string, pos=0, endpos=None): return _window(p, "fullmatch", string, pos, endpos)
def pattern_findall(p, string, pos=0, endpos=None): return _window(p, "findall", string, pos, endpos)
def pattern_finditer(p, string, pos=0, endpos=None): return list(_window(p, "finditer", string, pos, endpos))


def pattern_sub(p, repl, string, count=0):
    return p.sub(_text(repl, "repl"), _text(string, "string"), count)


def pattern_subn(p, repl, string, count=0):
    return p.subn(_text(repl, "repl"), _text(string, "string"), count)


def pattern_split(p, string, maxsplit=0):
    return p.split(_text(string, "string"), maxsplit)


def pattern_text(p): return p.pattern
def pattern_flags(p): return p.flags
def pattern_groups(p): return p.groups
def pattern_groupindex(p): return dict(p.groupindex)


def match_group(m, *groups): return m.group(*groups)
def match_groups(m, default=None): return m.groups(default)
def match_groupdict(m, default=None): return m.groupdict(default)
def match_start(m, group=0): return m.start(group)
def match_end(m, group=0): return m.end(group)
def match_span(m, group=0): return m.span(group)
def match_expand(m, template): return m.expand(_text(template, "template"))
def match_lastindex(m): return m.lastindex
def match_lastgroup(m): return m.lastgroup
def match_pos(m): return m.pos
def match_endpos(m): return m.endpos
def match_string(m): return m.string
