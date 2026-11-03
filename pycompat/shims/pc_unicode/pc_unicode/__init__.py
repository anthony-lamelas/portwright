"""Python's Unicode string methods and unicodedata, as plain functions.

Each function calls the built-in it names, so CPython 3.12 and its Unicode 15.0 data
are the reference. Strings may hold lone surrogates, as Python strings can.

Arguments must be core built-in values: None, bool, int, float, str, bytes, bytearray,
and lists, tuples, sets, frozensets and dicts of them. Anything else, such as a
Decimal, raises TypeError, because its behavior comes from its own module.
"""

import unicodedata as _ud

import functools as _functools

__all__ = [
    "upper", "lower", "casefold", "title", "capitalize", "swapcase",
    "isalpha", "isdigit", "isdecimal", "isnumeric", "isalnum", "isspace", "isupper", "islower",
    "istitle", "isidentifier", "isprintable", "isascii",
    "split", "rsplit", "splitlines", "strip", "lstrip", "rstrip",
    "center", "ljust", "rjust", "zfill", "expandtabs",
    "normalize", "is_normalized", "category", "bidirectional", "combining", "east_asian_width",
    "mirrored", "decomposition", "char_name", "lookup", "numeric_value", "decimal_value", "digit_value",
    "unidata_version",
]

_BUILTIN_TYPES = (type(None), bool, int, float, str, bytes, bytearray, list, tuple, set, frozenset, dict)


def _check_builtin(value):
    if type(value) not in _BUILTIN_TYPES:
        raise TypeError(f"pc_unicode supports built-in types only, not {type(value).__name__}")
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



@_builtins_only
def upper(s): return str.upper(s)
@_builtins_only
def lower(s): return str.lower(s)
@_builtins_only
def casefold(s): return str.casefold(s)
@_builtins_only
def title(s): return str.title(s)
@_builtins_only
def capitalize(s): return str.capitalize(s)
@_builtins_only
def swapcase(s): return str.swapcase(s)

@_builtins_only
def isalpha(s): return str.isalpha(s)
@_builtins_only
def isdigit(s): return str.isdigit(s)
@_builtins_only
def isdecimal(s): return str.isdecimal(s)
@_builtins_only
def isnumeric(s): return str.isnumeric(s)
@_builtins_only
def isalnum(s): return str.isalnum(s)
@_builtins_only
def isspace(s): return str.isspace(s)
@_builtins_only
def isupper(s): return str.isupper(s)
@_builtins_only
def islower(s): return str.islower(s)
@_builtins_only
def istitle(s): return str.istitle(s)
@_builtins_only
def isidentifier(s): return str.isidentifier(s)
@_builtins_only
def isprintable(s): return str.isprintable(s)
@_builtins_only
def isascii(s): return str.isascii(s)


@_builtins_only
def split(s, sep=None, maxsplit=-1):
    """``str.split(s, sep, maxsplit)``."""
    return str.split(s, sep, maxsplit)


@_builtins_only
def rsplit(s, sep=None, maxsplit=-1):
    """``str.rsplit(s, sep, maxsplit)``."""
    return str.rsplit(s, sep, maxsplit)


@_builtins_only
def splitlines(s, keepends=False):
    """``str.splitlines(s, keepends)``."""
    return str.splitlines(s, keepends)


@_builtins_only
def strip(s, chars=None): return str.strip(s, chars)
@_builtins_only
def lstrip(s, chars=None): return str.lstrip(s, chars)
@_builtins_only
def rstrip(s, chars=None): return str.rstrip(s, chars)


@_builtins_only
def center(s, width, fillchar=" "): return str.center(s, width, fillchar)
@_builtins_only
def ljust(s, width, fillchar=" "): return str.ljust(s, width, fillchar)
@_builtins_only
def rjust(s, width, fillchar=" "): return str.rjust(s, width, fillchar)
@_builtins_only
def zfill(s, width): return str.zfill(s, width)
@_builtins_only
def expandtabs(s, tabsize=8): return str.expandtabs(s, tabsize)


@_builtins_only
def normalize(form, s):
    """``unicodedata.normalize(form, s)``."""
    return _ud.normalize(form, s)


@_builtins_only
def is_normalized(form, s):
    """``unicodedata.is_normalized(form, s)``."""
    return _ud.is_normalized(form, s)


@_builtins_only
def category(ch): return _ud.category(ch)
@_builtins_only
def bidirectional(ch): return _ud.bidirectional(ch)
@_builtins_only
def combining(ch): return _ud.combining(ch)
@_builtins_only
def east_asian_width(ch): return _ud.east_asian_width(ch)
@_builtins_only
def mirrored(ch): return _ud.mirrored(ch)
@_builtins_only
def decomposition(ch): return _ud.decomposition(ch)


@_builtins_only
def char_name(ch, default=None):
    """``unicodedata.name(ch)``, or ``default`` when given and the character has none."""
    if default is None:
        return _ud.name(ch)
    return _ud.name(ch, default)


@_builtins_only
def lookup(name):
    """``unicodedata.lookup(name)``."""
    return _ud.lookup(name)


@_builtins_only
def numeric_value(ch, default=None):
    """``unicodedata.numeric(ch)``, or ``default`` when given."""
    return _ud.numeric(ch) if default is None else _ud.numeric(ch, default)


@_builtins_only
def decimal_value(ch, default=None):
    """``unicodedata.decimal(ch)``, or ``default`` when given."""
    return _ud.decimal(ch) if default is None else _ud.decimal(ch, default)


@_builtins_only
def digit_value(ch, default=None):
    """``unicodedata.digit(ch)``, or ``default`` when given."""
    return _ud.digit(ch) if default is None else _ud.digit(ch, default)


@_builtins_only
def unidata_version():
    """``unicodedata.unidata_version``."""
    return _ud.unidata_version
