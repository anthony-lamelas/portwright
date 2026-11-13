"""Python's string formatting and number semantics, as plain functions.

Each function calls the built-in it names, so CPython is the reference. Portwright
ports this package to Rust, which gives a Rust crate that formats, rounds, parses
and divides numbers exactly as Python 3.12 does.

Arguments must be core built-in values: None, bool, int, float, str, bytes, bytearray,
and lists, tuples, sets, frozensets and dicts of them. Anything else, such as a
Decimal or Fraction, raises TypeError. Those types bring their own formatting and
arithmetic from their own modules, which this package doesn't cover.

Sets and frozensets holding str or bytes (directly or in a tuple or frozenset) also
raise TypeError. Their iteration order follows the per-process hash seed, so ``str``
of one would differ from run to run.
"""

import _string
import functools as _functools

__all__ = [
    "format_value", "str_format", "percent_format",
    "float_repr", "int_str", "round_number",
    "parse_int", "parse_float", "int_to_float",
    "floor_div", "modulo", "div_mod", "true_div",
]

_BUILTIN_TYPES = (type(None), bool, int, float, str, bytes, bytearray, list, tuple, set, frozenset, dict)


def _hash_randomized(value):
    """Whether ``value``'s hash depends on the process's hash seed: a str or bytes, or a
    tuple or frozenset holding one."""
    if type(value) in (str, bytes):
        return True
    if type(value) in (tuple, frozenset):
        return any(_hash_randomized(item) for item in value)
    return False


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
        if type(value) in (set, frozenset) and any(_hash_randomized(item) for item in value):
            raise TypeError("pc_format doesn't support sets of str or bytes, whose order changes between processes")


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


def _refuse_attribute_fields(template, depth=0):
    """Raise TypeError if a replacement field reads an attribute, as in ``{0.real}``.

    Attribute lookups reach Python's object model (``{0.__class__.__doc__}`` is int's
    docstring), which a port over plain values can't reproduce. Fields are scanned in
    order, including fields nested in format specs. A template Python can't parse is
    left for ``str.format`` to reject.
    """
    try:
        fields = list(_string.formatter_parser(template))
    except ValueError:
        return
    for _literal, field, spec, _conversion in fields:
        if field is not None:
            try:
                for is_attribute, _name in _string.formatter_field_name_split(field)[1]:
                    if is_attribute:
                        raise TypeError("pc_format doesn't support attribute access in format fields")
            except ValueError:
                pass
        if spec and depth < 1:
            _refuse_attribute_fields(spec, depth + 1)


@_builtins_only
def str_format(template, args=(), kwargs=None):
    """``template.format(*args, **kwargs)``, without attribute access in fields."""
    if type(template) is str:
        _refuse_attribute_fields(template)
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
