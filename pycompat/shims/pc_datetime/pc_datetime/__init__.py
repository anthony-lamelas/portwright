"""Python's datetime calendar, arithmetic, ISO 8601, strftime and strptime, on plain data.

Each function builds real ``datetime`` objects and calls the method it names, so
CPython 3.12 is the reference. Values cross as tuples, so a port needs no Python
objects:

- a date is ``(year, month, day)``
- a time is ``(hour, minute, second, microsecond, utcoffset, fold)``
- a datetime is ``(year, month, day, hour, minute, second, microsecond, utcoffset, fold)``
- a timedelta is ``(days, seconds, microseconds)``

Each must be a tuple of exactly those fields, all ints (bool counts as int, as in the
constructors); anything else raises TypeError. ``utcoffset`` is whole seconds east of
UTC as an int, or None for a naive value. Fractional offsets aren't supported, so a
float offset raises TypeError. Functions that depend on the machine's local time zone
are left out, because their results would differ between machines. For the same
reason the process is pinned to UTC on import: ``strptime``'s ``%Z`` accepts the local
zone's names, so "CST" would parse on one machine and fail on another.

Arguments must be core built-in values: None, bool, int, float, str, bytes, bytearray,
and lists, tuples, sets, frozensets and dicts of them. Anything else, such as a
Decimal, raises TypeError, because its behavior comes from its own module.
"""

import datetime as _dt
import os as _os
import time as _time

_os.environ["TZ"] = "UTC"
_time.tzset()

import functools as _functools

__all__ = [
    "make_date", "make_time", "make_datetime", "make_timedelta",
    "timedelta_str", "timedelta_repr", "timedelta_total_seconds", "timedelta_multiply", "timedelta_divide",
    "timedelta_floor_divide", "date_toordinal", "date_fromordinal", "weekday", "isoweekday", "isocalendar",
    "date_fromisocalendar", "date_add", "datetime_add", "datetime_subtract", "datetime_compare",
    "date_isoformat", "time_isoformat", "datetime_isoformat", "date_fromisoformat", "time_fromisoformat",
    "datetime_fromisoformat", "strftime", "strptime", "datetime_str", "datetime_repr", "timezone_name",
    "datetime_timestamp", "datetime_fromtimestamp", "datetime_astimezone",
]

_BUILTIN_TYPES = (type(None), bool, int, float, str, bytes, bytearray, list, tuple, set, frozenset, dict)


def _check_builtin(value):
    if type(value) not in _BUILTIN_TYPES:
        raise TypeError(f"pc_datetime supports built-in types only, not {type(value).__name__}")
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



def _tz(offset):
    if offset is None:
        return None
    if not isinstance(offset, int):
        raise TypeError(f"utcoffset must be whole seconds as an int, or None, not {type(offset).__name__}")
    return _dt.timezone(_dt.timedelta(seconds=offset))


def _offset(value):
    delta = value.utcoffset()
    return None if delta is None else delta.days * 86400 + delta.seconds


def _date_out(d): return (d.year, d.month, d.day)
def _time_out(t): return (t.hour, t.minute, t.second, t.microsecond, _offset(t), t.fold)


def _dt_out(d):
    return (d.year, d.month, d.day, d.hour, d.minute, d.second, d.microsecond, _offset(d), d.fold)


def _td_out(td): return (td.days, td.seconds, td.microseconds)


def _fields(value, kind, names, offset=None):
    """``value`` if it's a tuple of exactly ``names``, all ints (bool counts, as in the
    constructors) except the ``utcoffset`` field at index ``offset``, which ``_tz`` checks.

    Anything else would reach CPython's own handling of other types: indexing a dict or
    bytes, ignoring extra fields, or float seconds in a timedelta.
    """
    if type(value) is not tuple or len(value) != len(names):
        raise TypeError(f"a {kind} must be a tuple ({', '.join(names)})")
    for index, (name, field) in enumerate(zip(names, value)):
        if index != offset and not isinstance(field, int):
            raise TypeError(f"{kind} field {name} must be an int, not {type(field).__name__}")
    return value


_DATE = ("year", "month", "day")
_TIME = ("hour", "minute", "second", "microsecond", "utcoffset", "fold")
_DATETIME = _DATE + _TIME
_TIMEDELTA = ("days", "seconds", "microseconds")


def _date_in(d): return _dt.date(*_fields(d, "date", _DATE))


def _time_in(t):
    t = _fields(t, "time", _TIME, offset=4)
    return _dt.time(t[0], t[1], t[2], t[3], _tz(t[4]), fold=t[5])


def _dt_in(d):
    d = _fields(d, "datetime", _DATETIME, offset=7)
    return _dt.datetime(d[0], d[1], d[2], d[3], d[4], d[5], d[6], _tz(d[7]), fold=d[8])


def _td_in(td):
    td = _fields(td, "timedelta", _TIMEDELTA)
    return _dt.timedelta(days=td[0], seconds=td[1], microseconds=td[2])


@_builtins_only
def make_date(year, month, day):
    """``date(year, month, day)``, including its range errors."""
    return _date_out(_dt.date(year, month, day))


@_builtins_only
def make_time(hour=0, minute=0, second=0, microsecond=0, utcoffset=None, fold=0):
    return _time_out(_dt.time(hour, minute, second, microsecond, _tz(utcoffset), fold=fold))


@_builtins_only
def make_datetime(year, month, day, hour=0, minute=0, second=0, microsecond=0, utcoffset=None, fold=0):
    return _dt_out(_dt.datetime(year, month, day, hour, minute, second, microsecond, _tz(utcoffset), fold=fold))


@_builtins_only
def make_timedelta(days=0, seconds=0, microseconds=0, milliseconds=0, minutes=0, hours=0, weeks=0):
    """``timedelta(...)`` normalized to (days, seconds, microseconds). Floats round half to even."""
    return _td_out(_dt.timedelta(days, seconds, microseconds, milliseconds, minutes, hours, weeks))


@_builtins_only
def timedelta_str(td): return str(_td_in(td))
@_builtins_only
def timedelta_repr(td): return repr(_td_in(td))
@_builtins_only
def timedelta_total_seconds(td): return _td_in(td).total_seconds()
@_builtins_only
def timedelta_multiply(td, factor): return _td_out(_td_in(td) * factor)


@_builtins_only
def timedelta_divide(td, divisor):
    """``td / divisor``: a timedelta for a number, a float for a timedelta given as a tuple."""
    result = _td_in(td) / (_td_in(divisor) if type(divisor) is tuple else divisor)
    return _td_out(result) if isinstance(result, _dt.timedelta) else result


@_builtins_only
def timedelta_floor_divide(td, divisor):
    result = _td_in(td) // (_td_in(divisor) if type(divisor) is tuple else divisor)
    return _td_out(result) if isinstance(result, _dt.timedelta) else result


@_builtins_only
def date_toordinal(d): return _date_in(d).toordinal()
@_builtins_only
def date_fromordinal(n): return _date_out(_dt.date.fromordinal(n))
@_builtins_only
def weekday(d): return _date_in(d).weekday()
@_builtins_only
def isoweekday(d): return _date_in(d).isoweekday()
@_builtins_only
def isocalendar(d): return tuple(_date_in(d).isocalendar())
@_builtins_only
def date_fromisocalendar(year, week, day): return _date_out(_dt.date.fromisocalendar(year, week, day))
@_builtins_only
def date_add(d, td): return _date_out(_date_in(d) + _td_in(td))
@_builtins_only
def datetime_add(d, td): return _dt_out(_dt_in(d) + _td_in(td))


@_builtins_only
def datetime_subtract(a, b):
    """``a - b`` for two datetimes: a timedelta, or TypeError when one is naive."""
    return _td_out(_dt_in(a) - _dt_in(b))


@_builtins_only
def datetime_compare(a, b):
    """-1, 0 or 1 by ``<`` and ``==``. Ordering a naive and an aware value raises TypeError."""
    x, y = _dt_in(a), _dt_in(b)
    if x == y:
        return 0
    return -1 if x < y else 1


@_builtins_only
def date_isoformat(d): return _date_in(d).isoformat()
@_builtins_only
def time_isoformat(t, timespec="auto"): return _time_in(t).isoformat(timespec)
@_builtins_only
def datetime_isoformat(d, sep="T", timespec="auto"): return _dt_in(d).isoformat(sep, timespec)
@_builtins_only
def date_fromisoformat(text): return _date_out(_dt.date.fromisoformat(text))
@_builtins_only
def time_fromisoformat(text): return _time_out(_dt.time.fromisoformat(text))
@_builtins_only
def datetime_fromisoformat(text): return _dt_out(_dt.datetime.fromisoformat(text))


# The C89 directives plus those CPython documents (%f, %z, %:z, %G, %u, %V). Anything
# else, and any flag or width such as %-d or %10Y, goes to the C library's strftime,
# whose output differs between C libraries and versions.
_STRFTIME_DIRECTIVES = frozenset("aAbBcdfGHIjmMpSuUVwWxXyYzZ%")


def _check_strftime_format(fmt):
    i = 0
    while i < len(fmt):
        if fmt[i] == "%":
            if fmt[i + 1:i + 3] == ":z":
                i += 3
                continue
            if fmt[i + 1:i + 2] not in _STRFTIME_DIRECTIVES:
                raise ValueError("pc_datetime's strftime supports only the C89 and CPython-documented "
                                 "directives, without flags or widths")
            i += 2
        else:
            i += 1


@_builtins_only
def strftime(d, fmt):
    """``datetime.strftime(fmt)`` in the C locale, which Python leaves LC_TIME at.

    Only the C89 directives and those CPython documents are supported: %a %A %b %B %c
    %d %f %G %H %I %j %m %M %p %S %u %U %V %w %W %x %X %y %Y %z %:z %Z %%. Others, and
    flags or widths, raise ValueError, because the C library would decide their output.
    """
    value = _dt_in(d)
    if type(fmt) is str:
        _check_strftime_format(fmt)
    return value.strftime(fmt)


@_builtins_only
def strptime(text, fmt):
    """``datetime.strptime(text, fmt)``."""
    return _dt_out(_dt.datetime.strptime(text, fmt))


@_builtins_only
def datetime_str(d): return str(_dt_in(d))
@_builtins_only
def datetime_repr(d): return repr(_dt_in(d))
@_builtins_only
def timezone_name(utcoffset): return _tz(utcoffset).tzname(None)


@_builtins_only
def datetime_timestamp(d):
    """``timestamp()`` for an aware datetime. Naive values would use local time, so they're refused."""
    if _fields(d, "datetime", _DATETIME, offset=7)[7] is None:
        raise ValueError("naive datetimes depend on the local time zone and aren't supported")
    return _dt_in(d).timestamp()


@_builtins_only
def datetime_fromtimestamp(timestamp, utcoffset):
    """``datetime.fromtimestamp(timestamp, timezone(offset))``.

    Where the C library's ``gmtime`` fails (a year past what its ``int`` holds, such as
    ``1e18``), CPython raises OSError with the C library's message. That becomes a fixed
    OverflowError.
    """
    if utcoffset is None:
        raise ValueError("a UTC offset is required; local time isn't supported")
    tz = _tz(utcoffset)
    try:
        value = _dt.datetime.fromtimestamp(timestamp, tz)
    except OSError:
        raise OverflowError("timestamp out of range for the C library's gmtime") from None
    return _dt_out(value)


@_builtins_only
def datetime_astimezone(d, utcoffset):
    """``astimezone(timezone(offset))`` for an aware datetime."""
    if _fields(d, "datetime", _DATETIME, offset=7)[7] is None or utcoffset is None:
        raise ValueError("both offsets are required; local time isn't supported")
    return _dt_out(_dt_in(d).astimezone(_tz(utcoffset)))
