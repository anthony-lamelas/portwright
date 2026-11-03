"""datetime on plain data, compared with the real module.

Each test builds the expected value with ``datetime`` directly. The process is pinned
to UTC, as the shim is, so local-zone names can't make results differ between machines.
"""

import datetime as dt
import itertools
import os
import time

import pytest

import pc_datetime as pc

os.environ["TZ"] = "UTC"
time.tzset()


def tz(offset):
    return None if offset is None else dt.timezone(dt.timedelta(seconds=offset))


def off(value):
    delta = value.utcoffset()
    return None if delta is None else delta.days * 86400 + delta.seconds


def d_out(d): return (d.year, d.month, d.day)
def t_out(t): return (t.hour, t.minute, t.second, t.microsecond, off(t), t.fold)
def dt_out(d): return (d.year, d.month, d.day, d.hour, d.minute, d.second, d.microsecond, off(d), d.fold)
def td_out(td): return (td.days, td.seconds, td.microseconds)
def dt_in(d): return dt.datetime(d[0], d[1], d[2], d[3], d[4], d[5], d[6], tz(d[7]), fold=d[8])
def td_in(td): return dt.timedelta(days=td[0], seconds=td[1], microseconds=td[2])


def outcome(func, *args):
    try:
        value = func(*args)
    except Exception as exc:
        return ("raised", type(exc).__name__, str(exc))
    return ("returned", type(value).__name__, repr(value))


def same(port_func, expected, *args):
    assert outcome(port_func, *args) == outcome(expected, *args), args


YEARS = [1, 2, 4, 99, 100, 999, 1000, 1582, 1600, 1900, 1970, 1999, 2000, 2004, 2024, 2100, 9999, 0, 10000, -1]
OFFSETS = [None, 0, 3600, -3600, 19800, -34200, 50400, -86399, 86399, 86400, 45, -1]
DATETIMES = [(2024, 2, 29, 13, 5, 9, 123456, None, 0), (1, 1, 1, 0, 0, 0, 0, None, 0), (9999, 12, 31, 23, 59, 59, 999999, 0, 0),
             (2000, 1, 1, 0, 0, 0, 0, 3600, 0), (1970, 1, 1, 0, 0, 0, 0, 0, 0), (2023, 12, 31, 12, 0, 0, 5, -18000, 1),
             (999, 7, 4, 9, 30, 0, 0, None, 0), (2021, 1, 3, 23, 59, 59, 500000, 19800, 0), (1900, 2, 28, 1, 2, 3, 4, -34200, 0)]


def test_make_date_and_its_errors():
    for year, month, day in itertools.product(YEARS, [0, 1, 2, 12, 13], [0, 1, 28, 29, 30, 31, 32]):
        same(pc.make_date, lambda y, m, d: d_out(dt.date(y, m, d)), year, month, day)
    for bad in [(2024.0, 1, 1), ("2024", 1, 1), (2024, True, 1)]:
        same(pc.make_date, lambda y, m, d: d_out(dt.date(y, m, d)), *bad)


def test_make_time_and_datetime():
    for hour, minute, second, micro, offset, fold in itertools.product([0, 23, 24, -1], [0, 59, 60], [0, 59, 60], [0, 999999, 10**6], OFFSETS[:8], [0, 1, 2]):
        same(pc.make_time, lambda *a: t_out(dt.time(a[0], a[1], a[2], a[3], tz(a[4]), fold=a[5])), hour, minute, second, micro, offset, fold)
    for args in DATETIMES:
        same(pc.make_datetime, lambda *a: dt_out(dt_in(a)), *args)


def test_bad_offsets():
    for offset in [86400, -86400, 90000]:
        same(pc.make_datetime, lambda *a: dt_out(dt_in(a)), 2024, 1, 1, 0, 0, 0, 0, offset, 0)


@pytest.mark.parametrize("field", ["days", "seconds", "microseconds", "milliseconds", "minutes", "hours", "weeks"])
def test_make_timedelta(field):
    values = [0, 1, -1, 0.5, -0.5, 1.5, 2.5, 1e-6, 0.4999999, 1 / 3, -2 / 3, 86399.9999995, 1e9, 999999999, 1e10,
              -1e10, 1e15, float("inf"), float("nan"), 2**63]
    for value in values:
        same(lambda v: pc.make_timedelta(**{field: v}), lambda v: td_out(dt.timedelta(**{field: v})), value)


def test_timedelta_mixed_float_rounding():
    for args in itertools.product([0, 0.5, -1.5], [0.25, -0.75, 1.5], [0.5, 1.5, -0.5], [0, 0.0005], [0.5]):
        same(lambda *a: pc.make_timedelta(*a), lambda *a: td_out(dt.timedelta(*a)), *args)


TIMEDELTAS = [(0, 0, 0), (1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, 0, 1), (-1, 86399, 999999), (999999999, 86399, 999999),
              (-999999999, 0, 0), (2, 3661, 500000), (-3, 7200, 5), (0, 86399, 0), (100, 0, 1)]


def test_timedelta_text_and_seconds():
    for td in TIMEDELTAS:
        same(pc.timedelta_str, lambda t: str(td_in(t)), td)
        same(pc.timedelta_repr, lambda t: repr(td_in(t)), td)
        same(pc.timedelta_total_seconds, lambda t: td_in(t).total_seconds(), td)


def test_timedelta_arithmetic():
    for td, factor in itertools.product(TIMEDELTAS, [0, 1, -1, 2, 0.5, 1.5, -0.25, 1 / 3, 1e10, float("nan"), True]):
        same(pc.timedelta_multiply, lambda t, f: td_out(td_in(t) * f), td, factor)
        same(pc.timedelta_divide, lambda t, f: td_out(td_in(t) / f), td, factor)
        same(pc.timedelta_floor_divide, lambda t, f: td_out(td_in(t) // f), td, factor if isinstance(factor, int) else 3)
    for a, b in itertools.product(TIMEDELTAS[:8], TIMEDELTAS[:8]):
        same(pc.timedelta_divide, lambda x, y: td_in(x) / td_in(y), a, b)
        same(pc.timedelta_floor_divide, lambda x, y: td_in(x) // td_in(y), a, b)


def test_ordinals_and_weekdays():
    for n in [1, 2, 365, 366, 719163, 730120, 3652059, 0, 3652060, -5]:
        same(pc.date_fromordinal, lambda v: d_out(dt.date.fromordinal(v)), n)
    for year, month, day in itertools.product([1, 4, 100, 400, 1900, 2000, 2023, 2024, 9999], [1, 2, 3, 12], [1, 28, 29, 31]):
        if day <= 28 or (month, day) in [(1, 29), (1, 31), (3, 31), (12, 31), (3, 29), (12, 29)]:
            d = (year, month, day)
            same(pc.date_toordinal, lambda v: dt.date(*v).toordinal(), d)
            same(pc.weekday, lambda v: dt.date(*v).weekday(), d)
            same(pc.isoweekday, lambda v: dt.date(*v).isoweekday(), d)
            same(pc.isocalendar, lambda v: tuple(dt.date(*v).isocalendar()), d)


def test_fromisocalendar():
    for year, week, day in itertools.product([1, 2004, 2009, 2015, 2020, 2021, 9999, 0, 10000], [0, 1, 52, 53, 54], [0, 1, 7, 8]):
        same(pc.date_fromisocalendar, lambda *a: d_out(dt.date.fromisocalendar(*a)), year, week, day)


def test_adding_and_subtracting():
    for d, td in itertools.product(DATETIMES, TIMEDELTAS):
        same(pc.datetime_add, lambda a, b: dt_out(dt_in(a) + td_in(b)), d, td)
        same(pc.date_add, lambda a, b: d_out(dt.date(*a[:3]) + td_in(b)), d[:3], td)
    for a, b in itertools.product(DATETIMES, DATETIMES):
        same(pc.datetime_subtract, lambda x, y: td_out(dt_in(x) - dt_in(y)), a, b)


def test_comparing():
    def compare(a, b):
        x, y = dt_in(a), dt_in(b)
        return 0 if x == y else (-1 if x < y else 1)
    pairs = list(itertools.product(DATETIMES, DATETIMES)) + [((2024, 1, 1, 1, 0, 0, 0, 3600, 0), (2024, 1, 1, 0, 0, 0, 0, 0, 0)),
                                                            ((2024, 1, 1, 0, 0, 0, 0, None, 0), (2024, 1, 1, 0, 0, 0, 0, None, 1))]
    for a, b in pairs:
        same(pc.datetime_compare, compare, a, b)


@pytest.mark.parametrize("timespec", ["auto", "hours", "minutes", "seconds", "milliseconds", "microseconds", "nanoseconds", ""])
def test_isoformat(timespec):
    for d in DATETIMES:
        for sep in ["T", " ", "x", "日"]:
            same(pc.datetime_isoformat, lambda a, s, t: dt_in(a).isoformat(s, t), d, sep, timespec)
        same(pc.time_isoformat, lambda a, t: dt.time(a[0], a[1], a[2], a[3], tz(a[4]), fold=a[5]).isoformat(t),
             (d[3], d[4], d[5], d[6], d[7], d[8]), timespec)
        same(pc.date_isoformat, lambda a: dt.date(*a).isoformat(), d[:3])
    for bad_sep in ["", "ab"]:
        same(pc.datetime_isoformat, lambda a, s: dt_in(a).isoformat(s), DATETIMES[0], bad_sep)


ISO_TEXTS = ["2024-02-29", "2024-02-30", "20240229", "2024-W09-4", "2024W094", "2024-060", "0001-01-01", "9999-12-31",
             "2024-02-29T13:05:09", "2024-02-29 13:05:09.123", "2024-02-29T13:05:09.123456", "2024-02-29T13:05:09.1234567",
             "2024-02-29T13:05:09+05:30", "2024-02-29T13:05:09-0800", "2024-02-29T13:05:09Z", "2024-02-29T13:05:09+05",
             "2024-02-29T13:05:09+05:30:15", "2024-02-29T13:05:09+05:30:15.5", "2024-02-29T24:00:00", "2024-02-29T13",
             "2024-02-29T1305", "2024-02-29T13:05:09,5", "2024-02-29T", "2024-2-29", "", "nope", "2024-02-29T13:60",
             "13:05:09", "13:05", "13", "130509", "13:05:09.5+01:00", "T13:05", "25:00", "13:05:09.123456789",
             "2024-02-29T13:05:09+24:00", "2024-02-29　13:05", "２０２４-02-29"]


def test_fromisoformat():
    for text in ISO_TEXTS:
        same(pc.datetime_fromisoformat, lambda s: dt_out(dt.datetime.fromisoformat(s)), text)
        same(pc.date_fromisoformat, lambda s: d_out(dt.date.fromisoformat(s)), text)
        same(pc.time_fromisoformat, lambda s: t_out(dt.time.fromisoformat(s)), text)


DIRECTIVES = list("aAbBcdfHIjmMpSUwWxXyYzZGuVeCDFgknrRtThl%") + [":z", "Q", "E", "O", "Ey", "Od", "-d", "_H", "0e", "^a", "#b"]


def test_strftime_every_directive():
    for d, directive in itertools.product(DATETIMES, DIRECTIVES):
        same(pc.strftime, lambda a, f: dt_in(a).strftime(f), d, f"%{directive}")
    for d in DATETIMES:
        for fmt in ["", "plain", "%", "%%Y", "100%", "%Y-%m-%dT%H:%M:%S.%f%z", "%c|%x|%X", "%%%Y%%", "日%Y", "x\ud800%d", "\x00%d"]:
            same(pc.strftime, lambda a, f: dt_in(a).strftime(f), d, fmt)


STRPTIME = [("2024-02-29", "%Y-%m-%d"), ("2024-02-30", "%Y-%m-%d"), ("24-2-9", "%y-%m-%d"), ("69", "%y"), ("68", "%y"),
            ("Thu Feb 29 13:05:09 2024", "%c"), ("02/29/24", "%x"), ("13:05:09", "%X"), ("1:05 PM", "%I:%M %p"),
            ("12:00 am", "%I:%M %p"), ("2024 060", "%Y %j"), ("2024 366", "%Y %j"), ("2023 366", "%Y %j"),
            ("2024 09 4", "%G %V %u"), ("2024 09", "%G %V"), ("2024 10 3", "%Y %U %w"), ("2024 10 3", "%Y %W %w"),
            ("13:05:09.5", "%H:%M:%S.%f"), ("13:05:09.1234567", "%H:%M:%S.%f"), ("+0530", "%z"), ("-08:00", "%z"),
            ("Z", "%z"), ("+05:30:15", "%z"), ("+0530 UTC", "%z %Z"), ("GMT", "%Z"), ("CST", "%Z"), ("utc", "%Z"),
            ("February", "%B"), ("feb", "%b"), ("FEB", "%b"), ("Thursday", "%A"), ("2024-02-29 extra", "%Y-%m-%d"),
            ("2024", "%Y%"), ("x", "%Q"), ("2024-02-29", "%Y-%m-%d %H"), ("  2024", "%Y"), ("2024 2", "%Y %d"),
            ("31/12/1999 23:59", "%d/%m/%Y %H:%M"), ("1", "%d"), ("0", "%d"), ("2024 3", "%Y %m %m"), ("%", "%%"),
            ("١٢", "%d"), ("2024-02-29T13:05", "%Y-%m-%dT%H:%M"), ("10000", "%Y"), ("0999", "%Y"), ("999", "%Y")]


def test_strptime():
    for text, fmt in STRPTIME:
        same(pc.strptime, lambda s, f: dt_out(dt.datetime.strptime(s, f)), text, fmt)


def test_str_repr_and_zone_names():
    for d in DATETIMES:
        same(pc.datetime_str, lambda a: str(dt_in(a)), d)
        same(pc.datetime_repr, lambda a: repr(dt_in(a)), d)
    for offset in [0, 3600, -3600, 19800, -34200, 45, 86399, -86399, 86400]:
        same(pc.timezone_name, lambda o: tz(o).tzname(None), offset)


def test_timestamps_and_zone_changes():
    for d in [x for x in DATETIMES if x[7] is not None]:
        same(pc.datetime_timestamp, lambda a: dt_in(a).timestamp(), d)
        for offset in [0, 3600, -34200, 86399]:
            same(pc.datetime_astimezone, lambda a, o: dt_out(dt_in(a).astimezone(tz(o))), d, offset)
    for stamp, offset in itertools.product([0, 1.5, -1.5, 1e9, 1709211909.123456, 0.0000005, -62135596800, 253402300800, 1e20, float("nan")], [0, 3600, -34200]):
        same(pc.datetime_fromtimestamp, lambda s, o: dt_out(dt.datetime.fromtimestamp(s, tz(o))), stamp, offset)
