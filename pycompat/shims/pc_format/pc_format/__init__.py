"""Python's string formatting and number semantics, as plain functions.

Each function calls the built-in it names, so CPython is the reference. Portwright
ports this package to Rust, which gives a Rust crate that formats, rounds, parses
and divides numbers exactly as Python 3.12 does.

Arguments must be core built-in values: None, bool, int, float, str, bytes, bytearray,
and lists, tuples, sets, frozensets and dicts of them. Anything else, such as a
Decimal or Fraction, raises TypeError. Those types bring their own formatting and
arithmetic from their own modules, which this package doesn't cover.
"""

import functools as _functools

__all__ = [
    "format_value", "str_format", "percent_format",
    "float_repr", "int_str", "round_number",
    "parse_int", "parse_float", "int_to_float",
    "floor_div", "modulo", "div_mod", "true_div",
]

_BUILTIN_TYPES = (type(None), bool, int, float, str, bytes, bytearray, list, tuple, set, frozenset, dict)


def _check_builtin(value):
    if type(value) not in _BUILTIN_TYPES:
        raise TypeError(f"pc_format supports built-in types only, not {type(value).__name__}")
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
def format_value(value, spec):
    """``format(value, spec)`` for an int, float, str or bool."""
    return format(value, spec)


@_builtins_only
def str_format(template, args=(), kwargs=None):
    """``template.format(*args, **kwargs)``."""
    return template.format(*args, **(kwargs or {}))


@_builtins_only
def percent_format(template, values):
    """``template % values``. ``values`` is one value, a tuple or a dict."""
    return template % values


@_builtins_only
def float_repr(x):
    """``repr(x)`` for a float: the shortest text that reads back as ``x``."""
    return repr(x)


@_builtins_only
def int_str(n):
    """``str(n)`` for an int, including the limit on very long ints."""
    return str(n)


@_builtins_only
def round_number(x, ndigits=None):
    """``round(x)`` or ``round(x, ndigits)`` for an int or float."""
    if ndigits is None:
        return round(x)
    return round(x, ndigits)


@_builtins_only
def parse_int(text, base=10):
    """``int(text, base)``."""
    return int(text, base)


@_builtins_only
def parse_float(text):
    """``float(text)``."""
    return float(text)


@_builtins_only
def int_to_float(n):
    """``float(n)`` for an int."""
    return float(n)


@_builtins_only
def floor_div(a, b):
    """``a // b`` for ints and floats."""
    return a // b


@_builtins_only
def modulo(a, b):
    """``a % b`` for ints and floats."""
    return a % b


@_builtins_only
def div_mod(a, b):
    """``divmod(a, b)`` for ints and floats."""
    return divmod(a, b)


@_builtins_only
def true_div(a, b):
    """``a / b`` for ints and floats."""
    return a / b
