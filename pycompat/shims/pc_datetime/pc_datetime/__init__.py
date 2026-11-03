"""Python's datetime calendar, arithmetic, ISO 8601, strftime and strptime, on plain data.

Each function builds real ``datetime`` objects and calls the method it names, so
CPython 3.12 is the reference. Values cross as tuples, so a port needs no Python
objects:

- a date is ``(year, month, day)``
- a time is ``(hour, minute, second, microsecond, utcoffset, fold)``
- a datetime is ``(year, month, day, hour, minute, second, microsecond, utcoffset, fold)``
- a timedelta is ``(days, seconds, microseconds)``

``utcoffset`` is whole seconds east of UTC, or None for a naive value. Microseconds
in offsets aren't supported. Functions that depend on the machine's local time zone
are left out, because their results would differ between machines. For the same
reason the process is pinned to UTC on import: ``strptime``'s ``%Z`` accepts the local
zone's names, so "CST" would parse on one machine and fail on another.
"""

import datetime as _dt
import os as _os
import time as _time

_os.environ["TZ"] = "UTC"
_time.tzset()

__all__ = [
    "make_date", "make_time", "make_datetime", "make_timedelta",
    "timedelta_str", "timedelta_repr", "timedelta_total_seconds", "timedelta_multiply", "timedelta_divide",
    "timedelta_floor_divide", "date_toordinal", "date_fromordinal", "weekday", "isoweekday", "isocalendar",
    "date_fromisocalendar", "date_add", "datetime_add", "datetime_subtract", "datetime_compare",
    "date_isoformat", "time_isoformat", "datetime_isoformat", "date_fromisoformat", "time_fromisoformat",
    "datetime_fromisoformat", "strftime", "strptime", "datetime_str", "datetime_repr", "timezone_name",
    "datetime_timestamp", "datetime_fromtimestamp", "datetime_astimezone",
]


def _tz(offset):
    if offset is None:
        return None
    return _dt.timezone(_dt.timedelta(seconds=offset))


def _offset(value):
    delta = value.utcoffset()
    return None if delta is None else delta.days * 86400 + delta.seconds


def _date_out(d): return (d.year, d.month, d.day)
def _time_out(t): return (t.hour, t.minute, t.second, t.microsecond, _offset(t), t.fold)


def _dt_out(d):
    return (d.year, d.month, d.day, d.hour, d.minute, d.second, d.microsecond, _offset(d), d.fold)


def _td_out(td): return (td.days, td.seconds, td.microseconds)
def _date_in(d): return _dt.date(*d)
def _time_in(t): return _dt.time(t[0], t[1], t[2], t[3], _tz(t[4]), fold=t[5])
def _dt_in(d): return _dt.datetime(d[0], d[1], d[2], d[3], d[4], d[5], d[6], _tz(d[7]), fold=d[8])
def _td_in(td): return _dt.timedelta(days=td[0], seconds=td[1], microseconds=td[2])


def make_date(year, month, day):
    """``date(year, month, day)``, including its range errors."""
    return _date_out(_dt.date(year, month, day))


def make_time(hour=0, minute=0, second=0, microsecond=0, utcoffset=None, fold=0):
    return _time_out(_dt.time(hour, minute, second, microsecond, _tz(utcoffset), fold=fold))


def make_datetime(year, month, day, hour=0, minute=0, second=0, microsecond=0, utcoffset=None, fold=0):
    return _dt_out(_dt.datetime(year, month, day, hour, minute, second, microsecond, _tz(utcoffset), fold=fold))


def make_timedelta(days=0, seconds=0, microseconds=0, milliseconds=0, minutes=0, hours=0, weeks=0):
    """``timedelta(...)`` normalized to (days, seconds, microseconds). Floats round half to even."""
    return _td_out(_dt.timedelta(days, seconds, microseconds, milliseconds, minutes, hours, weeks))


def timedelta_str(td): return str(_td_in(td))
def timedelta_repr(td): return repr(_td_in(td))
def timedelta_total_seconds(td): return _td_in(td).total_seconds()
def timedelta_multiply(td, factor): return _td_out(_td_in(td) * factor)


def timedelta_divide(td, divisor):
    """``td / divisor``: a timedelta for a number, a float for a timedelta given as a tuple."""
    result = _td_in(td) / (_td_in(divisor) if isinstance(divisor, tuple) else divisor)
    return _td_out(result) if isinstance(result, _dt.timedelta) else result


def timedelta_floor_divide(td, divisor):
    result = _td_in(td) // (_td_in(divisor) if isinstance(divisor, tuple) else divisor)
    return _td_out(result) if isinstance(result, _dt.timedelta) else result


def date_toordinal(d): return _date_in(d).toordinal()
def date_fromordinal(n): return _date_out(_dt.date.fromordinal(n))
def weekday(d): return _date_in(d).weekday()
def isoweekday(d): return _date_in(d).isoweekday()
def isocalendar(d): return tuple(_date_in(d).isocalendar())
def date_fromisocalendar(year, week, day): return _date_out(_dt.date.fromisocalendar(year, week, day))
def date_add(d, td): return _date_out(_date_in(d) + _td_in(td))
def datetime_add(d, td): return _dt_out(_dt_in(d) + _td_in(td))


def datetime_subtract(a, b):
    """``a - b`` for two datetimes: a timedelta, or TypeError when one is naive."""
    return _td_out(_dt_in(a) - _dt_in(b))


def datetime_compare(a, b):
    """-1, 0 or 1 by ``<`` and ``==``. Ordering a naive and an aware value raises TypeError."""
    x, y = _dt_in(a), _dt_in(b)
    if x == y:
        return 0
    return -1 if x < y else 1


def date_isoformat(d): return _date_in(d).isoformat()
def time_isoformat(t, timespec="auto"): return _time_in(t).isoformat(timespec)
def datetime_isoformat(d, sep="T", timespec="auto"): return _dt_in(d).isoformat(sep, timespec)
def date_fromisoformat(text): return _date_out(_dt.date.fromisoformat(text))
def time_fromisoformat(text): return _time_out(_dt.time.fromisoformat(text))
def datetime_fromisoformat(text): return _dt_out(_dt.datetime.fromisoformat(text))


def strftime(d, fmt):
    """``datetime.strftime(fmt)`` in the C locale, which Python leaves LC_TIME at."""
    return _dt_in(d).strftime(fmt)


def strptime(text, fmt):
    """``datetime.strptime(text, fmt)``."""
    return _dt_out(_dt.datetime.strptime(text, fmt))


def datetime_str(d): return str(_dt_in(d))
def datetime_repr(d): return repr(_dt_in(d))
def timezone_name(utcoffset): return _tz(utcoffset).tzname(None)


def datetime_timestamp(d):
    """``timestamp()`` for an aware datetime. Naive values would use local time, so they're refused."""
    if d[7] is None:
        raise ValueError("naive datetimes depend on the local time zone and aren't supported")
    return _dt_in(d).timestamp()


def datetime_fromtimestamp(timestamp, utcoffset):
    """``datetime.fromtimestamp(timestamp, timezone(offset))``."""
    if utcoffset is None:
        raise ValueError("a UTC offset is required; local time isn't supported")
    return _dt_out(_dt.datetime.fromtimestamp(timestamp, _tz(utcoffset)))


def datetime_astimezone(d, utcoffset):
    """``astimezone(timezone(offset))`` for an aware datetime."""
    if d[7] is None or utcoffset is None:
        raise ValueError("both offsets are required; local time isn't supported")
    return _dt_out(_dt_in(d).astimezone(_tz(utcoffset)))
