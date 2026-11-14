"""Checks for pc_datetime. Inputs only; the Python library supplies every expected value."""
import calendar
import datetime as _dt
from decimal import Decimal

from hypothesis import given, settings, HealthCheck, strategies as st, assume
from portwright.compare import same, PythonExpression
from portwright.compare.strategies import text

settings.register_profile(
    "pc", deadline=None, max_examples=150,
    suppress_health_check=[HealthCheck.too_slow, HealthCheck.filter_too_much, HealthCheck.data_too_large],
)
settings.load_profile("pc")

# ---------------------------------------------------------------- building blocks

EDGE_YEARS = [1, 2, 4, 99, 100, 400, 999, 1000, 1582, 1600, 1900, 1969, 1970, 2000, 2004,
              2020, 2021, 2024, 2100, 9998, 9999]
valid_year = st.one_of(st.integers(1, 9999), st.sampled_from(EDGE_YEARS))
any_year = st.one_of(valid_year, valid_year, valid_year,
                     st.sampled_from([0, -1, 10000, 2**31, -2**63, 2**70]))


@st.composite
def valid_date(draw):
    y = draw(valid_year)
    m = draw(st.integers(1, 12))
    last = calendar.monthrange(y, m)[1]
    d = draw(st.one_of(st.integers(1, last), st.sampled_from([1, last])))
    return (y, m, d)


@st.composite
def maybe_date(draw):
    if draw(st.integers(0, 4)):
        return draw(valid_date())
    return (draw(any_year), draw(st.integers(-1, 14)), draw(st.integers(-1, 33)))


offset_valid = st.one_of(
    st.just(0),
    st.integers(-24, 24).map(lambda h: h * 3600) .filter(lambda o: abs(o) < 86400),
    st.integers(-1439, 1439).map(lambda m: m * 60),
    st.integers(-86399, 86399),
    st.sampled_from([19800, -34200, 50400, 45, -1, 86399, -86399, 20700, 3601]),
)
offset_or_none = st.one_of(st.none(), offset_valid)
offset_any = st.one_of(offset_or_none, offset_or_none, offset_or_none,
                       st.sampled_from([86400, -86400, 90000, 2**40]))

time_parts = st.tuples(
    st.one_of(st.integers(0, 23), st.sampled_from([0, 23])),
    st.one_of(st.integers(0, 59), st.sampled_from([0, 59])),
    st.one_of(st.integers(0, 59), st.sampled_from([0, 59])),
    st.one_of(st.integers(0, 999999), st.sampled_from([0, 1, 999, 1000, 500000, 999999, 123456])),
)


@st.composite
def valid_time(draw, offsets=offset_or_none):
    h, mi, s, us = draw(time_parts)
    return (h, mi, s, us, draw(offsets), draw(st.sampled_from([0, 0, 1])))


@st.composite
def valid_datetime(draw, offsets=offset_or_none):
    y, m, d = draw(valid_date())
    h, mi, s, us = draw(time_parts)
    return (y, m, d, h, mi, s, us, draw(offsets), draw(st.sampled_from([0, 0, 1])))


@st.composite
def aware_datetime(draw):
    return draw(valid_datetime(offsets=offset_valid))


@st.composite
def maybe_datetime(draw):
    if draw(st.integers(0, 5)):
        return draw(valid_datetime())
    base = list(draw(valid_datetime()))
    i = draw(st.integers(0, 8))
    base[i] = draw(st.sampled_from([-1, 0, 13, 24, 32, 60, 10**6, 10000, 2, 86400, None, 1.0, "1"]))
    return tuple(base)


MAXD = 999999999
valid_td = st.one_of(
    st.tuples(st.integers(-MAXD, MAXD), st.integers(0, 86399), st.integers(0, 999999)),
    st.tuples(st.integers(-1000, 1000), st.integers(0, 86399), st.integers(0, 999999)),
    st.sampled_from([(0, 0, 0), (1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, 0, 1), (-1, 86399, 999999),
                     (MAXD, 86399, 999999), (-MAXD, 0, 0), (2, 3661, 500000), (-3, 7200, 5),
                     (0, 86399, 0), (100, 0, 1), (-1, 0, 1), (0, 59, 999999), (10, 0, 0)]),
)
# unnormalized and float components flow through timedelta(days=, seconds=, microseconds=)
loose_td = st.one_of(
    valid_td, valid_td,
    st.tuples(st.integers(-10**6, 10**6), st.integers(-10**12, 10**12), st.integers(-10**13, 10**13)),
    st.tuples(st.floats(-1e6, 1e6), st.floats(-1e9, 1e9), st.floats(-1e9, 1e9)),
    st.tuples(st.just(MAXD), st.integers(86399, 86401), st.integers(999999, 1000001)),
)
small_td = st.tuples(st.integers(-800000, 800000), st.integers(0, 86399), st.integers(0, 999999))

bad_scalar = st.sampled_from([Decimal("1"), Decimal("NaN"), "1", 1.5, None, b"1", True, [1], 2**80])

# ---------------------------------------------------------------- constructors


@given(maybe_date())
def test_make_date(d):
    same("make_date", *d)


@given(st.sampled_from([(2024.0, 1, 1), ("2024", 1, 1), (2024, True, 1), (Decimal(2024), 1, 1),
                        (True, True, True), (2024, 2, 29.0), (None, 1, 1), (2**64, 1, 1),
                        (2024, 2), (bytearray(b"x"), 1, 1)]))
def test_make_date_bad_types(args):
    same("make_date", *args)


@given(st.integers(-2, 25), st.integers(-1, 61), st.integers(-1, 61),
       st.sampled_from([0, 1, 999999, 10**6, -1, 500000]), offset_any, st.sampled_from([0, 1, 2, -1]))
def test_make_time_grid(h, m, s, us, off, fold):
    same("make_time", h, m, s, us, off, fold)


@given(valid_time(offsets=offset_any))
def test_make_time_valid(t):
    same("make_time", *t)


@given(st.data())
def test_make_time_defaults(data):
    t = data.draw(valid_time())
    n = data.draw(st.integers(0, 6))
    same("make_time", *t[:n])
    keys = ["hour", "minute", "second", "microsecond", "utcoffset", "fold"]
    kw = {k: v for k, v in zip(keys, t) if data.draw(st.booleans())}
    same("make_time", **kw)


@given(valid_datetime(offsets=offset_any))
def test_make_datetime_valid(d):
    same("make_datetime", *d)


@given(maybe_datetime(), st.integers(3, 9))
def test_make_datetime_partial(d, n):
    same("make_datetime", *d[:n])


@given(valid_datetime(), st.data())
def test_make_datetime_kwargs(d, data):
    keys = ["hour", "minute", "second", "microsecond", "utcoffset", "fold"]
    kw = {k: v for k, v in zip(keys, d[3:]) if data.draw(st.booleans())}
    same("make_datetime", d[0], d[1], d[2], **kw)
    same("make_datetime", year=d[0], month=d[1], day=d[2], **kw)


@given(st.integers(0, 8), bad_scalar)
def test_make_datetime_bad_types(i, v):
    d = [2024, 2, 29, 13, 5, 9, 123456, 3600, 0]
    d[i] = v
    same("make_datetime", *d)


td_num = st.one_of(
    st.integers(-10**6, 10**6),
    st.integers(-10**12, 10**12),
    st.floats(-1e7, 1e7, allow_nan=False),
    st.sampled_from([0, 1, -1, 0.5, -0.5, 1.5, 2.5, -2.5, 1e-6, 5e-7, -5e-7, 1.5e-6, 2.5e-6, 0.4999999,
                     1 / 3, -2 / 3, 86399.9999995, 1e9, 999999999, 1e10, -1e10, 1e15, 0.0, -0.0,
                     float("inf"), float("-inf"), float("nan"), 2**63, -2**63, 10**20, True, False]),
)


@given(st.lists(td_num, max_size=7))
def test_make_timedelta_positional(args):
    same("make_timedelta", *args)


@given(st.dictionaries(st.sampled_from(["days", "seconds", "microseconds", "milliseconds",
                                        "minutes", "hours", "weeks"]), td_num, max_size=7))
def test_make_timedelta_kwargs(kw):
    same("make_timedelta", **kw)


@given(st.sampled_from(["days", "seconds", "microseconds", "milliseconds", "minutes", "hours", "weeks"]),
       st.one_of(st.floats(-1e3, 1e3, allow_nan=False),
                 st.integers(-1000, 1000).map(lambda n: n / 2),
                 st.integers(-10**7, 10**7).map(lambda n: n * 1e-7 + 0.5e-6)))
def test_make_timedelta_float_rounding(field, v):
    same("make_timedelta", **{field: v})


@given(st.sampled_from(["days", "seconds", "microseconds", "weeks"]), bad_scalar)
def test_make_timedelta_bad(field, v):
    same("make_timedelta", **{field: v})


@given(st.integers(-MAXD - 2, MAXD + 2), st.sampled_from([0, 1, -1, 86399, 86400, 999999, 10**6]))
def test_make_timedelta_range_edges(days, extra):
    same("make_timedelta", days, extra)
    same("make_timedelta", days, 0, extra)

# ---------------------------------------------------------------- timedelta ops


@given(loose_td)
def test_timedelta_text(td):
    same("timedelta_str", td)
    same("timedelta_repr", td)
    same("timedelta_total_seconds", td)


factor = st.one_of(
    st.integers(-10, 10), st.integers(-10**9, 10**9),
    st.floats(-1e3, 1e3), st.floats(allow_nan=True, allow_infinity=True),
    st.sampled_from([0.5, -0.5, 1.5, 2.5, 1 / 3, 1e-7, 0.0, -0.0, float("nan"), float("inf"), True, 10**30]),
)


@given(st.one_of(small_td, valid_td), factor)
def test_timedelta_multiply(td, f):
    same("timedelta_multiply", td, f)


@given(small_td, st.sampled_from([Decimal("2"), "2", None, (1, 0, 0), [2], 2j if False else b"2"]))
def test_timedelta_multiply_bad(td, f):
    same("timedelta_multiply", td, f)


divisor = st.one_of(
    st.integers(-10, 10), st.integers(-10**12, 10**12),
    st.floats(-1e3, 1e3), st.floats(allow_nan=True, allow_infinity=True),
    small_td, valid_td,
    st.sampled_from([0, 0.0, -0.0, (0, 0, 0), (0, 0, 1), (0, 1, 0), (-1, 86399, 999999),
                     float("nan"), float("inf"), 3, 7, 1e-7, Decimal(2), "2", None, [1, 0, 0]]),
)


@given(st.one_of(small_td, valid_td), divisor)
def test_timedelta_divide(td, dv):
    same("timedelta_divide", td, dv)
    same("timedelta_floor_divide", td, dv)

# ---------------------------------------------------------------- calendar


@given(st.one_of(st.integers(-5, 3652065), st.sampled_from([1, 2, 365, 366, 719163, 730120, 3652059,
                                                             3652060, 0, -1, 2**63, 1.0, "1", None])))
def test_date_fromordinal(n):
    same("date_fromordinal", n)


@given(maybe_date())
def test_calendar_queries(d):
    same("date_toordinal", d)
    same("weekday", d)
    same("isoweekday", d)
    same("isocalendar", d)
    same("date_isoformat", d)


@given(st.one_of(valid_year, any_year), st.one_of(st.integers(1, 53), st.integers(-1, 55)),
       st.one_of(st.integers(1, 7), st.integers(-1, 9)))
def test_date_fromisocalendar(y, w, d):
    same("date_fromisocalendar", y, w, d)


@given(st.sampled_from([(2024.0, 1, 1), ("2024", 1, 1), (2024, 1.0, 1), (None, 1, 1), (2024, 1, True)]))
def test_date_fromisocalendar_types(args):
    same("date_fromisocalendar", *args)


@given(maybe_date(), loose_td)
def test_date_add(d, td):
    same("date_add", d, td)


@given(valid_date(), small_td)
def test_date_add_small(d, td):
    same("date_add", d, td)


@given(maybe_datetime(), loose_td)
def test_datetime_add(d, td):
    same("datetime_add", d, td)


@given(valid_datetime(), small_td)
def test_datetime_add_small(d, td):
    same("datetime_add", d, td)


@st.composite
def dt_pair(draw):
    a = draw(valid_datetime())
    kind = draw(st.integers(0, 4))
    if kind == 0:
        b = draw(valid_datetime())
    elif kind == 1:  # same instant expressed in another offset or with different fold
        b = list(a)
        b[8] = 1 - a[8]
        if a[7] is not None and draw(st.booleans()):
            b[7] = draw(offset_valid)
        b = tuple(b)
    elif kind == 2:
        b = list(a)
        b[draw(st.integers(0, 6))] += draw(st.sampled_from([-1, 1])) if False else 0
        b[6] = min(999999, max(0, a[6] + draw(st.sampled_from([-1, 0, 1]))))
        b = tuple(b)
    elif kind == 3:  # both aware
        a = draw(aware_datetime())
        b = draw(aware_datetime())
    else:  # both naive
        a = draw(valid_datetime(offsets=st.none()))
        b = draw(valid_datetime(offsets=st.none()))
    if draw(st.booleans()):
        a, b = b, a
    return a, b


@given(dt_pair())
def test_datetime_subtract_compare(p):
    a, b = p
    same("datetime_subtract", a, b)
    same("datetime_compare", a, b)
    same("datetime_compare", a, a)


@given(aware_datetime())
def test_datetime_compare_equal_instants(a):
    # same instant, different offset: must compare equal
    other = _dt.datetime(*a[:7], tzinfo=_dt.timezone(_dt.timedelta(seconds=a[7])))
    for off in (0, 3600, -34200):
        try:
            c = other.astimezone(_dt.timezone(_dt.timedelta(seconds=off)))
        except OverflowError:
            continue
        b = (c.year, c.month, c.day, c.hour, c.minute, c.second, c.microsecond, off, 0)
        same("datetime_compare", a, b)
        same("datetime_subtract", a, b)

# ---------------------------------------------------------------- isoformat

TIMESPECS = ["auto", "hours", "minutes", "seconds", "milliseconds", "microseconds", "nanoseconds", "",
             "AUTO", "Hours"]
seps = st.one_of(st.sampled_from(["T", " ", "x", "日", "", "ab", "\ud800", "\x00"]), text(min_size=0, max_size=2))


@given(valid_datetime(offsets=offset_any), seps, st.sampled_from(TIMESPECS), st.integers(0, 3))
def test_datetime_isoformat(d, sep, spec, mode):
    if mode == 0:
        same("datetime_isoformat", d)
    elif mode == 1:
        same("datetime_isoformat", d, sep)
    elif mode == 2:
        same("datetime_isoformat", d, timespec=spec)
    else:
        same("datetime_isoformat", d, sep, spec)


@given(valid_time(offsets=offset_any), st.sampled_from(TIMESPECS), st.booleans())
def test_time_isoformat(t, spec, use_default):
    if use_default:
        same("time_isoformat", t)
    else:
        same("time_isoformat", t, spec)


@given(st.sampled_from([3, 7, None, b"T"]))
def test_isoformat_bad_args(x):
    same("datetime_isoformat", (2024, 1, 2, 3, 4, 5, 6, None, 0), x)
    same("time_isoformat", (3, 4, 5, 6, None, 0), x)

# ---------------------------------------------------------------- fromisoformat


def _num(draw, width, lo, hi):
    return str(draw(st.integers(lo, hi))).zfill(width)


@st.composite
def iso_date_text(draw):
    y = _num(draw, 4, 0, 9999)
    m = _num(draw, 2, 0, 13)
    d = _num(draw, 2, 0, 32)
    form = draw(st.integers(0, 5))
    if form == 0:
        return f"{y}-{m}-{d}"
    if form == 1:
        return f"{y}{m}{d}"
    w = _num(draw, 2, 0, 54)
    wd = str(draw(st.integers(0, 8)))
    if form == 2:
        return f"{y}-W{w}-{wd}"
    if form == 3:
        return f"{y}W{w}{wd}"
    if form == 4:
        return f"{y}-{_num(draw, 3, 0, 367)}"
    return f"{y}{_num(draw, 3, 0, 367)}"


@st.composite
def iso_tz_text(draw):
    form = draw(st.integers(0, 6))
    if form == 0:
        return ""
    if form == 1:
        return draw(st.sampled_from(["Z", "z"]))
    sign = draw(st.sampled_from(["+", "-"]))
    hh = _num(draw, 2, 0, 24)
    mm = _num(draw, 2, 0, 60)
    ss = _num(draw, 2, 0, 60)
    colon = draw(st.sampled_from([":", ""]))
    if form == 2:
        return f"{sign}{hh}"
    if form == 3:
        return f"{sign}{hh}{colon}{mm}"
    if form == 4:
        return f"{sign}{hh}{colon}{mm}{colon}{ss}"
    frac = "".join(draw(st.lists(st.sampled_from("0123456789"), min_size=1, max_size=7)))
    return f"{sign}{hh}{colon}{mm}{colon}{ss}.{frac}"


@st.composite
def iso_time_text(draw):
    hh = _num(draw, 2, 0, 25)
    mm = _num(draw, 2, 0, 60)
    ss = _num(draw, 2, 0, 61)
    colon = draw(st.sampled_from([":", ":", ""]))
    form = draw(st.integers(0, 3))
    if form == 0:
        t = hh
    elif form == 1:
        t = f"{hh}{colon}{mm}"
    else:
        t = f"{hh}{colon}{mm}{colon}{ss}"
        if form == 3:
            frac = "".join(draw(st.lists(st.sampled_from("0123456789"), min_size=1, max_size=10)))
            t += draw(st.sampled_from([".", ","])) + frac
    if draw(st.integers(0, 3)) == 0:
        t = draw(st.sampled_from(["24:00", "24:00:00", "24:00:00.000000", "24", "2400", "24:00:01",
                                  "23:59:60", "00:00:00.000000"]))
    return t + draw(iso_tz_text())


@st.composite
def mutate(draw, s):
    if not s or draw(st.integers(0, 3)):
        return s
    i = draw(st.integers(0, len(s)))
    op = draw(st.integers(0, 3))
    ch = draw(st.one_of(text(min_size=1, max_size=1), st.sampled_from(list("0123456789-:T.+Z W"))))
    if op == 0:
        return s[:i] + ch + s[i:]
    if op == 1:
        return s[:i] + s[i + 1:]
    if op == 2:
        return s[:i] + ch + s[i + 1:]
    return s[:i]


@st.composite
def iso_datetime_text(draw):
    d = draw(iso_date_text())
    if draw(st.integers(0, 4)) == 0:
        return draw(mutate(d))
    sep = draw(st.sampled_from(["T", "T", " ", "t", "x", "　", "日", "\ud800", "-", "W"]))
    return draw(mutate(d + sep + draw(iso_time_text())))


@st.composite
def iso_from_value(draw):
    d = draw(valid_datetime())
    spec = draw(st.sampled_from(TIMESPECS[:6]))
    sep = draw(st.sampled_from(["T", " ", "x"]))
    tz = _dt.timezone(_dt.timedelta(seconds=d[7])) if d[7] is not None else None
    v = _dt.datetime(*d[:7], tzinfo=tz, fold=d[8])
    return draw(mutate(v.isoformat(sep, spec)))


iso_any = st.one_of(iso_date_text(), iso_time_text(), iso_datetime_text(), iso_from_value(),
                    text(max_size=12),
                    st.sampled_from(["", "nope", "２０２４-02-29", "2024-02-29T13:05:09+05:30:15.5",
                                     "2024-02-29T13:05:09,5", "2024-02-29T", "T13:05", "\ud800",
                                     "2024-02-29T13:05:09.1234567", "2024-W53-1", "2020-W53-7",
                                     "0000-01-01", "9999-12-31T23:59:59.999999-23:59"]))


@given(iso_any)
def test_fromisoformat(s):
    same("datetime_fromisoformat", s)
    same("date_fromisoformat", s)
    same("time_fromisoformat", s)


@given(iso_datetime_text())
def test_datetime_fromisoformat_structured(s):
    same("datetime_fromisoformat", s)


@given(iso_time_text())
def test_time_fromisoformat_structured(s):
    same("time_fromisoformat", s)
    same("time_fromisoformat", "T" + s)


@given(iso_date_text())
def test_date_fromisoformat_structured(s):
    same("date_fromisoformat", s)


@given(st.sampled_from([b"2024-01-01", bytearray(b"2024-01-01"), 20240101, None, ["2024-01-01"]]))
def test_fromisoformat_bad_types(v):
    same("datetime_fromisoformat", v)
    same("date_fromisoformat", v)
    same("time_fromisoformat", v)

# ---------------------------------------------------------------- strftime / strptime

DIRECTIVES = list("aAbBcdfHIjmMpSUwWxXyYzZGuVeCDFgknrRtThl%") + [
    ":z", "Q", "E", "O", "Ey", "Od", "-d", "_H", "0e", "^a", "#b", "Ec", "EY", "Ou", "s", "P", "+", "v"]
literal = st.one_of(st.sampled_from(["", " ", "-", ":", "/", "T", ".", ",", "日", "%%", "x", "  "]),
                    text(max_size=3).map(lambda s: s.replace("%", "%%")))


@st.composite
def strftime_fmt(draw):
    parts = []
    for _ in range(draw(st.integers(0, 6))):
        parts.append(draw(literal))
        parts.append("%" + draw(st.sampled_from(DIRECTIVES)))
    parts.append(draw(literal))
    if draw(st.integers(0, 9)) == 0:
        parts.append("%")
    return "".join(parts)


@given(valid_datetime(offsets=offset_any), strftime_fmt())
def test_strftime(d, fmt):
    same("strftime", d, fmt)


@given(valid_datetime(), st.sampled_from(DIRECTIVES))
def test_strftime_single(d, directive):
    same("strftime", d, "%" + directive)


@given(valid_datetime(), text(max_size=8))
def test_strftime_any_text(d, fmt):
    same("strftime", d, fmt)


@given(st.sampled_from([(1, 1, 1, 0, 0, 0, 0, None, 0), (99, 1, 1, 0, 0, 0, 0, None, 0),
                        (999, 7, 4, 9, 30, 0, 0, None, 0), (9999, 12, 31, 23, 59, 59, 999999, 0, 0)]),
       st.sampled_from(["%Y", "%y", "%G", "%C", "%F", "%c", "%x", "%g", "%-Y", "%04Y", "%D"]))
def test_strftime_small_years(d, fmt):
    same("strftime", d, fmt)


@given(valid_datetime(), st.sampled_from([None, 5, b"%Y"]))
def test_strftime_bad_fmt(d, fmt):
    same("strftime", d, fmt)


PARSE_DIRECTIVES = list("aAbBcdfHIjmMpSUwWxXyYzZGuV%") + ["y"]


@st.composite
def strptime_case(draw):
    d = draw(valid_datetime(offsets=st.one_of(st.none(), st.integers(-1439, 1439).map(lambda m: m * 60),
                                               offset_valid)))
    tz = _dt.timezone(_dt.timedelta(seconds=d[7])) if d[7] is not None else None
    v = _dt.datetime(*d[:7], tzinfo=tz)
    sets = [
        ["Y", "m", "d"], ["Y", "m", "d", "H", "M", "S"], ["Y", "m", "d", "H", "M", "S", "f"],
        ["y", "m", "d"], ["Y", "j"], ["G", "V", "u"], ["Y", "U", "w"], ["Y", "W", "w"],
        ["Y", "U", "a"], ["Y", "W", "A"],
        ["d", "b", "Y"], ["A", "d", "B", "Y"], ["I", "M", "p"], ["Y", "m", "d", "H", "M", "z"],
        ["c"], ["x"], ["X"], ["x", "X"], ["H", "M", "S", "z"], ["Y", "m", "d", "Z"],
        ["Y", "m", "d", "H", "M", "S", "f", "z"],
    ]
    if draw(st.integers(0, 3)):
        keys = draw(st.sampled_from(sets))
    else:
        keys = draw(st.lists(st.sampled_from(PARSE_DIRECTIVES), min_size=1, max_size=5))
    seps = [draw(st.sampled_from(["-", " ", ":", "/", "", "T", ".", "  "])) for _ in keys]
    fmt = "".join(s + "%" + k for s, k in zip(seps, keys))[len(seps[0]):]
    try:
        s = v.strftime(fmt)
    except Exception:
        s = ""
    tweak = draw(st.integers(0, 7))
    if tweak == 0:
        s = s.upper()
    elif tweak == 1:
        s = s.lower()
    elif tweak == 2:
        s = draw(mutate(s))
    elif tweak == 3 and "%z" in fmt and v.tzinfo is not None:
        # colon form of the offset
        z = v.strftime("%z")
        colon = z[:3] + ":" + z[3:5] + ((":" + z[5:]) if len(z) > 5 else "")
        s = s.replace(z, draw(st.sampled_from([colon, "Z", z])))
    elif tweak == 4:
        s = " " + s if draw(st.booleans()) else s + draw(st.sampled_from([" ", "x", "0"]))
    return s, fmt


@given(strptime_case())
def test_strptime_roundtrip(case):
    same("strptime", *case)


STRPTIME_FIXED = [
    ("2024-02-30", "%Y-%m-%d"), ("69", "%y"), ("68", "%y"), ("00", "%y"), ("99", "%y"),
    ("12:00 am", "%I:%M %p"), ("12:00 PM", "%I:%M %p"), ("13:00 PM", "%I:%M %p"), ("0:00 AM", "%I:%M %p"),
    ("2024 366", "%Y %j"), ("2023 366", "%Y %j"), ("2023 000", "%Y %j"), ("2024 09", "%G %V"),
    ("2024 3", "%G %u"), ("2024 53 7", "%G %V %u"), ("2020 53 7", "%G %V %u"),
    ("2024 0 0", "%Y %U %w"), ("2024 53 6", "%Y %W %w"), ("2024 54 1", "%Y %U %w"),
    ("13:05:09.1234567", "%H:%M:%S.%f"), ("13:05:09.", "%H:%M:%S.%f"),
    ("+0530", "%z"), ("-08:00", "%z"), ("Z", "%z"), ("z", "%z"), ("+05:30:15", "%z"),
    ("+05:30:15.123", "%z"), ("+053015.5", "%z"), ("+2400", "%z"), ("+0560", "%z"), ("+05:3015", "%z"),
    ("+0530 UTC", "%z %Z"), ("GMT", "%Z"), ("CST", "%Z"), ("utc", "%Z"), ("UTC", "%Z"), ("gmt", "%Z"),
    ("February", "%B"), ("feb", "%b"), ("FEB", "%b"), ("Febr", "%b"), ("Thursday", "%A"), ("thu", "%a"),
    ("2024-02-29 extra", "%Y-%m-%d"), ("2024", "%Y%"), ("x", "%Q"), ("2024-02-29", "%Y-%m-%d %H"),
    ("  2024", "%Y"), ("2024   2", "%Y %d"), ("1", "%d"), ("0", "%d"), ("2024 3", "%Y %m %m"),
    ("%", "%%"), ("١٢", "%d"), ("10000", "%Y"), ("0999", "%Y"), ("999", "%Y"), ("0", "%Y"),
    ("", ""), ("abc", "abc"), ("ABC", "abc"), ("a.c", "a.c"), ("a(c", "a(c"), ("a\\c", "a\\c"),
    ("[x]", "[x]"), ("29 2 2024", "%d %m %Y"), ("Feb 29", "%b %d"), ("29", "%d"), ("2 29", "%m %d"),
    ("1900 2 29", "%Y %m %d"), ("Mon 2024 1", "%a %Y %W"), ("Thu Feb 29 13:05:09 2024", "%c"),
    ("02/29/24", "%x"), ("13:05:09", "%X"), ("12/31/99 23:59:59", "%x %X"), ("1 2", "%H%M"),
    ("123", "%H%M"), ("2024-02-29", "%G-%m-%d"), ("2024 5", "%V"), ("5", "%u"), ("2024 5", "%Y %u"),
    ("13", "%I"), ("PM", "%p"), ("2024-02-29\t13", "%Y-%m-%d %H"), ("2024-02-29 13", "%Y-%m-%d\t%H"),
    ("2024 02", "%Y %m"), ("2024", "%E"), ("2024", "%:z"), ("+05:30", "%:z"),
]


@given(st.sampled_from(STRPTIME_FIXED))
def test_strptime_fixed(case):
    same("strptime", *case)


@given(text(max_size=10), st.sampled_from(["%Y", "%d", "%b", "%z", "%Z", "%H:%M", "%p", "%a", "x%", ""]))
def test_strptime_garbage(s, fmt):
    same("strptime", s, fmt)


@given(st.sampled_from(["2024", "%Y"]), text(max_size=6))
def test_strptime_weird_fmt(s, fmt):
    same("strptime", s, fmt)
    same("strptime", s, "%Y" + fmt)


@given(st.sampled_from([(b"2024", "%Y"), ("2024", b"%Y"), (2024, "%Y"), ("2024", None)]))
def test_strptime_bad_types(case):
    same("strptime", *case)

# ---------------------------------------------------------------- str / repr / zones / timestamps


@given(maybe_datetime())
def test_datetime_str_repr(d):
    same("datetime_str", d)
    same("datetime_repr", d)


@given(offset_any)
def test_timezone_name(off):
    same("timezone_name", off)


@given(st.sampled_from(["0", 1.0, Decimal(0), True]))
def test_timezone_name_types(off):
    same("timezone_name", off)


@given(st.one_of(valid_datetime(offsets=offset_or_none), maybe_datetime()))
def test_datetime_timestamp(d):
    same("datetime_timestamp", d)


@given(aware_datetime())
def test_datetime_timestamp_aware(d):
    same("datetime_timestamp", d)


stamp = st.one_of(
    st.integers(-62135596800 - 86400, 253402300800 + 86400),
    st.floats(-62135596800 - 86400, 253402300800 + 86400),
    st.integers(-10**10, 10**10).map(lambda n: n / 1000),
    st.integers(-10**12, 10**12).map(lambda n: n / 10**6),
    st.integers(-10**8, 10**8).map(lambda n: n + 0.5e-6),
    st.sampled_from([0, 1.5, -1.5, 1e9, 1709211909.123456, 5e-7, -5e-7, 1.5e-6, -62135596800,
                     253402300800, 253402300799.9999995, 1e20, -1e20, float("nan"), float("inf"),
                     -0.0, 2**63, -2**63, 10**30, True]),
)


@given(stamp, offset_any)
def test_datetime_fromtimestamp(ts, off):
    same("datetime_fromtimestamp", ts, off)


@given(st.sampled_from(["0", None, Decimal(0), b"0", [0]]), offset_valid)
def test_datetime_fromtimestamp_types(ts, off):
    same("datetime_fromtimestamp", ts, off)


@given(st.one_of(aware_datetime(), aware_datetime(), valid_datetime(offsets=offset_or_none)), offset_any)
def test_datetime_astimezone(d, off):
    same("datetime_astimezone", d, off)


@given(st.sampled_from([(1, 1, 1, 0, 0, 0, 0, 3600, 0), (1, 1, 1, 0, 59, 59, 999999, 3600, 0),
                        (9999, 12, 31, 23, 0, 0, 0, -3600, 0), (9999, 12, 31, 23, 59, 59, 999999, 0, 1)]),
       offset_any)
def test_astimezone_edges(d, off):
    same("datetime_astimezone", d, off)
    same("datetime_timestamp", d)

# ---------------------------------------------------------------- wire-type traps


@given(st.sampled_from([[2024, 1, 1], (2024, 1), (2024, 1, 1, 1), (Decimal(2024), 1, 1), "2024-01-01",
                        None, (True, 1, 1), {2024: 1}]))
def test_shape_traps(d):
    same("date_toordinal", d)
    same("date_isoformat", d)
    same("weekday", d)
    same("date_add", d, (1, 0, 0))
    same("isocalendar", d)


@given(st.sampled_from([[1, 0, 0], (1, 0), (1,), (Decimal(1), 0, 0), (1.5, 0.5, 0.5), (True, False, True),
                        ("1", 0, 0), (1, 0, 0, 0)]))
def test_td_shape_traps(td):
    same("timedelta_str", td)
    same("timedelta_total_seconds", td)
    same("timedelta_multiply", td, 2)
    same("datetime_add", (2024, 1, 1, 0, 0, 0, 0, None, 0), td)


# ---------------------------------------------------------------- strengthen: arity messages

ALL_APIS = ["date_add", "date_fromisocalendar", "date_fromisoformat", "date_fromordinal",
            "date_isoformat", "date_toordinal", "datetime_add", "datetime_astimezone",
            "datetime_compare", "datetime_fromisoformat", "datetime_fromtimestamp",
            "datetime_isoformat", "datetime_repr", "datetime_str", "datetime_subtract",
            "datetime_timestamp", "isocalendar", "isoweekday", "make_date", "make_datetime",
            "make_time", "make_timedelta", "strftime", "strptime", "time_fromisoformat",
            "time_isoformat", "timedelta_divide", "timedelta_floor_divide",
            "timedelta_multiply", "timedelta_repr", "timedelta_str",
            "timedelta_total_seconds", "timezone_name", "weekday"]


@given(st.sampled_from(ALL_APIS), st.integers(0, 12))
def test_arity_messages(name, n):
    # Too few / too many positional args: "missing N required positional argument(s)",
    # "takes N positional argument(s) but M was/were given".
    same(name, *([1] * n))


# ---------------------------------------------------------------- strengthen: ISO week basic form

@st.composite
def iso_week_basic_long(draw):
    y = _num(draw, 4, 0, 9999)
    w = _num(draw, 2, 0, 54)
    tail = "".join(draw(st.lists(st.sampled_from("0123456789"), min_size=0, max_size=10)))
    s = f"{y}W{w}{tail}"
    if draw(st.integers(0, 2)) == 0:
        s += draw(st.sampled_from(["", "T12", "T1230", ":30", "Z", "+0100", ".5", "-01:00"]))
    return s


@given(iso_week_basic_long())
def test_fromisoformat_week_basic_long(s):
    same("datetime_fromisoformat", s)
    same("date_fromisoformat", s)


# ---------------------------------------------------------------- strengthen: strftime buffer limits

@st.composite
def wide_strftime_fmt(draw):
    # One ASCII run whose output is near time.strftime's 256-bytes-per-format-byte cap.
    pad = draw(st.sampled_from(["", "x", "xx", "xxx", "xxxxxxxx", "xxxxxxxxxxxxxxxx"]))
    flag = draw(st.sampled_from(["", "0", "_", "-", "00"]))
    width = draw(st.one_of(st.integers(900, 9000),
                           st.sampled_from([1023, 1024, 1025, 2040, 2047, 2048, 2049, 3000,
                                            4095, 4096, 4097, 8192])))
    directive = draw(st.sampled_from(list("YdHmSjyaAbB")))
    run = f"{pad}%{flag}{width}{directive}"
    pre = draw(st.sampled_from(["", "日", "%%", "x%Y日"]))
    post = draw(st.sampled_from(["", "日", "日%Y", "日x%01500d"]))
    return pre + run + post


@given(valid_datetime(), wide_strftime_fmt())
def test_strftime_wide(d, fmt):
    same("strftime", d, fmt)


def test_strftime_wide_fixed():
    d = (2020, 1, 1, 0, 0, 0, 0, None, 0)
    for fmt in ["x%03000Y", "x%02040Y", "%03000Y", "xxxx%3000Y", "x%Y日x%03000Y", "%1500Y日x%03000Y",
                "x%04090Y", "xxxxxxxxxxxxxxx%5000Y", "%05000Y"]:
        same("strftime", d, fmt)


def test_fromisoformat_week_basic_fixed():
    for s in ["2020W01123", "2020W011234", "2020W0112", "2020W01112", "2020W011120",
              "2020W0112345", "2020W01123456"]:
        same("datetime_fromisoformat", s)


# ---------------------------------------------------------------- strengthen: ISO week 53 / year boundaries

@given(valid_year, st.integers(-1, 9), st.sampled_from(["{y}-W53-{d}", "{y}W53{d}", "{y}-W53", "{y}W53",
                                                         "{y}-W52-{d}", "{y}W01{d}", "{y}-W53-{d}T12:30"]))
def test_fromisoformat_week53(y, d, form):
    s = form.format(y=str(y).zfill(4), d=d)
    same("date_fromisoformat", s)
    same("datetime_fromisoformat", s)


def test_fromisoformat_week53_all_years_sample():
    for y in list(range(1, 60)) + list(range(1990, 2040)) + [9990, 9991, 9992, 9993, 9994, 9995, 9996,
                                                            9997, 9998, 9999]:
        same("date_fromisoformat", f"{y:04d}-W53-1")
        same("date_fromisoformat", f"{y:04d}W537")


@st.composite
def year_edge_date(draw):
    y = draw(valid_year)
    if draw(st.booleans()):
        return (y, 12, draw(st.integers(20, 31)))
    return (y, 1, draw(st.integers(1, 7)))


@given(year_edge_date())
def test_isocalendar_year_edges(d):
    same("isocalendar", d)
    same("weekday", d)
    same("strftime", (*d, 0, 0, 0, 0, None, 0), "%G-%V-%u %Y-%U-%W-%j")


def test_isocalendar_year_edges_fixed():
    for y in list(range(1, 30)) + list(range(1995, 2035)) + [9997, 9998, 9999]:
        for day in range(20, 32):
            same("isocalendar", (y, 12, day))
        for day in range(1, 8):
            same("isocalendar", (y, 1, day))


@given(valid_year, st.sampled_from([52, 53, 1]), st.integers(1, 7))
def test_date_fromisocalendar_week_edges(y, w, d):
    same("date_fromisocalendar", y, w, d)


# ---------------------------------------------------------------- strengthen: %s with width/padding

@given(valid_datetime(offsets=offset_or_none),
       st.sampled_from(["", "0", "_", "-", "^", "#"]),
       st.one_of(st.just(""), st.integers(0, 25).map(str)),
       st.sampled_from(["", "x", "日"]))
def test_strftime_epoch_seconds_padded(d, flag, width, pre):
    same("strftime", d, f"{pre}%{flag}{width}s")


def test_strftime_epoch_seconds_negative_fixed():
    for d in [(1969, 12, 31, 23, 59, 59, 0, None, 0), (1969, 12, 31, 23, 59, 50, 0, None, 0),
              (1960, 1, 1, 0, 0, 0, 0, None, 0), (1, 1, 1, 0, 0, 0, 0, None, 0),
              (1900, 6, 1, 0, 0, 0, 0, 3600, 0), (1970, 1, 1, 0, 0, 0, 0, None, 0)]:
        for fmt in ["%s", "%5s", "%05s", "%_5s", "%12s", "%012s", "%_12s", "%-12s", "%15s", "%015s",
                    "%_15s", "%3s", "%03s", "%013s", "%014s"]:
            same("strftime", d, fmt)


# ---------------------------------------------------------------- strengthen: Unicode digits in strptime

import unicodedata as _ud
import sys as _sys

DIGIT_ZEROS = [c for c in range(_sys.maxunicode + 1)
               if _ud.category(chr(c)) == "Nd" and _ud.digit(chr(c), None) == 0
               and all(_ud.digit(chr(c + i), None) == i for i in range(10))]


def _to_block(s, zeros):
    out = []
    for i, ch in enumerate(s):
        if "0" <= ch <= "9":
            out.append(chr(zeros[i % len(zeros)] + ord(ch) - 48))
        else:
            out.append(ch)
    return "".join(out)


@st.composite
def unicode_digit_case(draw):
    d = draw(valid_datetime(offsets=st.none()))
    v = _dt.datetime(*d[:7])
    fmt = draw(st.sampled_from(["%Y-%m-%d", "%Y-%m-%d %H:%M:%S", "%d/%m/%y", "%Y %j", "%H:%M:%S.%f",
                                "%G %V %u", "%Y%m%d"]))
    zeros = draw(st.lists(st.sampled_from(DIGIT_ZEROS), min_size=1, max_size=3))
    return _to_block(v.strftime(fmt), zeros), fmt


@given(unicode_digit_case())
def test_strptime_unicode_digits(case):
    same("strptime", *case)


def test_strptime_unicode_digits_every_block():
    for z in DIGIT_ZEROS:
        same("strptime", _to_block("2024-02-29 13:05:09", [z]), "%Y-%m-%d %H:%M:%S")
        same("strptime", _to_block("1987", [z]), "%Y")
