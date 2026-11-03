"""Python's string formatting and number semantics, as plain functions.

Each function calls the built-in it names, so CPython is the reference. Portwright
ports this package to Rust, which gives a Rust crate that formats, rounds, parses
and divides numbers exactly as Python 3.12 does.
"""

__all__ = [
    "format_value", "str_format", "percent_format",
    "float_repr", "int_str", "round_number",
    "parse_int", "parse_float", "int_to_float",
    "floor_div", "modulo", "div_mod", "true_div",
]


def format_value(value, spec):
    """``format(value, spec)`` for an int, float, str or bool."""
    return format(value, spec)


def str_format(template, args=(), kwargs=None):
    """``template.format(*args, **kwargs)``."""
    return template.format(*args, **(kwargs or {}))


def percent_format(template, values):
    """``template % values``. ``values`` is one value, a tuple or a dict."""
    return template % values


def float_repr(x):
    """``repr(x)`` for a float: the shortest text that reads back as ``x``."""
    return repr(x)


def int_str(n):
    """``str(n)`` for an int, including the limit on very long ints."""
    return str(n)


def round_number(x, ndigits=None):
    """``round(x)`` or ``round(x, ndigits)`` for an int or float."""
    if ndigits is None:
        return round(x)
    return round(x, ndigits)


def parse_int(text, base=10):
    """``int(text, base)``."""
    return int(text, base)


def parse_float(text):
    """``float(text)``."""
    return float(text)


def int_to_float(n):
    """``float(n)`` for an int."""
    return float(n)


def floor_div(a, b):
    """``a // b`` for ints and floats."""
    return a // b


def modulo(a, b):
    """``a % b`` for ints and floats."""
    return a % b


def div_mod(a, b):
    """``divmod(a, b)`` for ints and floats."""
    return divmod(a, b)


def true_div(a, b):
    """``a / b`` for ints and floats."""
    return a / b
