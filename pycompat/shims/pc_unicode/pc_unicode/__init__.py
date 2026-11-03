"""Python's Unicode string methods and unicodedata, as plain functions.

Each function calls the built-in it names, so CPython 3.12 and its Unicode 15.0 data
are the reference. Strings may hold lone surrogates, as Python strings can.
"""

import unicodedata as _ud

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


def upper(s): return s.upper()
def lower(s): return s.lower()
def casefold(s): return s.casefold()
def title(s): return s.title()
def capitalize(s): return s.capitalize()
def swapcase(s): return s.swapcase()

def isalpha(s): return s.isalpha()
def isdigit(s): return s.isdigit()
def isdecimal(s): return s.isdecimal()
def isnumeric(s): return s.isnumeric()
def isalnum(s): return s.isalnum()
def isspace(s): return s.isspace()
def isupper(s): return s.isupper()
def islower(s): return s.islower()
def istitle(s): return s.istitle()
def isidentifier(s): return s.isidentifier()
def isprintable(s): return s.isprintable()
def isascii(s): return s.isascii()


def split(s, sep=None, maxsplit=-1):
    """``s.split(sep, maxsplit)``."""
    return s.split(sep, maxsplit)


def rsplit(s, sep=None, maxsplit=-1):
    """``s.rsplit(sep, maxsplit)``."""
    return s.rsplit(sep, maxsplit)


def splitlines(s, keepends=False):
    """``s.splitlines(keepends)``."""
    return s.splitlines(keepends)


def strip(s, chars=None): return s.strip(chars)
def lstrip(s, chars=None): return s.lstrip(chars)
def rstrip(s, chars=None): return s.rstrip(chars)


def center(s, width, fillchar=" "): return s.center(width, fillchar)
def ljust(s, width, fillchar=" "): return s.ljust(width, fillchar)
def rjust(s, width, fillchar=" "): return s.rjust(width, fillchar)
def zfill(s, width): return s.zfill(width)
def expandtabs(s, tabsize=8): return s.expandtabs(tabsize)


def normalize(form, s):
    """``unicodedata.normalize(form, s)``."""
    return _ud.normalize(form, s)


def is_normalized(form, s):
    """``unicodedata.is_normalized(form, s)``."""
    return _ud.is_normalized(form, s)


def category(ch): return _ud.category(ch)
def bidirectional(ch): return _ud.bidirectional(ch)
def combining(ch): return _ud.combining(ch)
def east_asian_width(ch): return _ud.east_asian_width(ch)
def mirrored(ch): return _ud.mirrored(ch)
def decomposition(ch): return _ud.decomposition(ch)


def char_name(ch, default=None):
    """``unicodedata.name(ch)``, or ``default`` when given and the character has none."""
    if default is None:
        return _ud.name(ch)
    return _ud.name(ch, default)


def lookup(name):
    """``unicodedata.lookup(name)``."""
    return _ud.lookup(name)


def numeric_value(ch, default=None):
    """``unicodedata.numeric(ch)``, or ``default`` when given."""
    return _ud.numeric(ch) if default is None else _ud.numeric(ch, default)


def decimal_value(ch, default=None):
    """``unicodedata.decimal(ch)``, or ``default`` when given."""
    return _ud.decimal(ch) if default is None else _ud.decimal(ch, default)


def digit_value(ch, default=None):
    """``unicodedata.digit(ch)``, or ``default`` when given."""
    return _ud.digit(ch) if default is None else _ud.digit(ch, default)


def unidata_version():
    """``unicodedata.unidata_version``."""
    return _ud.unidata_version
