from hypothesis import given, strategies as st

from portwright.compare import same, get, PythonExpression
from portwright.compare.strategies import text

from iso_inputs import (
    DT, ISO, date_expr, datetime_expr, duration_expr, local_datetime_expr, time_expr, timedelta_expr,
)

CONSTANTS = [
    "DATE_BAS_COMPLETE", "DATE_BAS_MONTH", "DATE_BAS_ORD_COMPLETE", "DATE_BAS_WEEK", "DATE_BAS_WEEK_COMPLETE",
    "DATE_CENTURY", "DATE_EXT_COMPLETE", "DATE_EXT_MONTH", "DATE_EXT_ORD_COMPLETE", "DATE_EXT_WEEK",
    "DATE_EXT_WEEK_COMPLETE", "DATE_YEAR", "DT_BAS_COMPLETE", "DT_BAS_ORD_COMPLETE", "DT_BAS_WEEK_COMPLETE",
    "DT_EXT_COMPLETE", "DT_EXT_ORD_COMPLETE", "DT_EXT_WEEK_COMPLETE", "D_ALT_BAS", "D_ALT_BAS_ORD",
    "D_ALT_EXT", "D_ALT_EXT_ORD", "D_DEFAULT", "D_WEEK", "TIME_BAS_COMPLETE", "TIME_BAS_MINUTE",
    "TIME_EXT_COMPLETE", "TIME_EXT_MINUTE", "TIME_HOUR", "TZ_BAS", "TZ_EXT", "TZ_HOUR", "__version__",
]

# the literal values, so formats can be built locally from them
FORMATS = [
    "%Y%m%d", "%Y-%m-%d", "%YW%W%w", "%Y-W%W-%w", "%Y%j", "%Y-%j", "%YW%W", "%Y-W%W", "%Y%m", "%Y-%m", "%Y",
    "%C", "%H%M%S", "%H:%M:%S", "%H%M", "%H:%M", "%H", "%z", "%Z", "%h",
    "%Y-%m-%dT%H:%M:%S%Z", "%Y%m%dT%H%M%S%z", "%Y-%jT%H:%M:%S%Z", "%Y%jT%H%M%S%z",
    "%Y-W%W-%wT%H:%M:%S%Z", "%YW%W%wT%H%M%S%z", "P%P", "P%p", "P%Y-%m-%dT%H:%M:%S", "P%Y%m%dT%H%M%S",
    "P%Y-%jT%H:%M:%S", "P%Y%jT%H%M%S",
]

DIRECTIVES = ["%d", "%f", "%H", "%j", "%m", "%M", "%S", "%w", "%W", "%Y", "%C", "%z", "%Z", "%h", "%%",
              "%P", "%p", "%a", "%", "%%Y", "%y", "T", "-", ":", " "]

format_strings = st.one_of(
    st.sampled_from(FORMATS),
    st.lists(st.one_of(st.sampled_from(DIRECTIVES), text(max_size=3)), max_size=8).map("".join),
)

yeardigits = st.sampled_from([4, 4, 4, 2, 3, 5, 6, 1, 0, -1, 10])

anything = st.one_of(
    date_expr(), datetime_expr(), time_expr(), timedelta_expr(), duration_expr(), duration_expr(finite=False),
)


def test_constants():
    for name in CONSTANTS + ["UTC", "LOCAL", "ISO8601Error"]:
        get(name)


@given(anything, format_strings, st.one_of(st.none(), yeardigits))
def test_strftime(obj, fmt, yd):
    if yd is None:
        same("strftime", obj, fmt)
    else:
        same("strftime", obj, fmt, yd)


@given(datetime_expr(wild_tz=True), st.sampled_from(FORMATS + ["%Y-%m-%dT%H:%M:%S.%f%Z", "%C%%%j"]))
def test_strftime_datetime(obj, fmt):
    same("strftime", obj, fmt)


@given(st.one_of(duration_expr(), timedelta_expr()), st.sampled_from(FORMATS + ["%d %f %H %m %M %S %W %Y %C %%", "P%P%p"]))
def test_strftime_durations(obj, fmt):
    same("strftime", obj, fmt)
    same("strftime", obj, fmt, 6)


@given(st.one_of(date_expr(), datetime_expr()), st.one_of(st.none(), st.sampled_from(FORMATS[:12]), format_strings),
       st.one_of(st.none(), yeardigits))
def test_date_isoformat(obj, fmt, yd):
    kwargs = {}
    if fmt is not None:
        kwargs["format"] = fmt
    if yd is not None:
        kwargs["yeardigits"] = yd
    same("date_isoformat", obj, **kwargs)


@given(st.one_of(time_expr(wild_tz=True), datetime_expr()), st.one_of(st.none(), st.sampled_from(FORMATS[12:20]), format_strings))
def test_time_isoformat(obj, fmt):
    if fmt is None:
        same("time_isoformat", obj)
    else:
        same("time_isoformat", obj, fmt)


@given(datetime_expr(wild_tz=True), st.one_of(st.none(), st.sampled_from(FORMATS[20:26]), format_strings))
def test_datetime_isoformat(obj, fmt):
    if fmt is None:
        same("datetime_isoformat", obj)
    else:
        same("datetime_isoformat", obj, fmt)


@given(st.one_of(duration_expr(), timedelta_expr(), duration_expr(finite=False)),
       st.one_of(st.none(), st.sampled_from(FORMATS[26:]), format_strings))
def test_duration_isoformat(obj, fmt):
    if fmt is None:
        same("duration_isoformat", obj)
    else:
        same("duration_isoformat", obj, fmt)


@given(st.one_of(datetime_expr(wild_tz=True), time_expr(wild_tz=True), local_datetime_expr()),
       st.one_of(st.none(), st.sampled_from(["%Z", "%z", "%h", "%Z", "%z", "%h", "", "%H", "Z"])))
def test_tz_isoformat(obj, fmt):
    if fmt is None:
        same("tz_isoformat", obj)
    else:
        same("tz_isoformat", obj, fmt)


def test_format_examples():
    for d in ["date(1985, 4, 12)", "date(2009, 12, 31)", "date(2010, 1, 3)", "date(1, 1, 1)", "date(9999, 12, 31)",
              "datetime(2012, 10, 12, 8, 29, 46, 69178)", "time(23, 59, 59, 999999)"]:
        for f in FORMATS:
            same("strftime", PythonExpression(f"{DT}.{d}"), f)
            same("strftime", PythonExpression(f"{DT}.{d}"), f, 6)
    for dur in ["P18Y9M4DT11H9M8S", "P2W", "-P2W", "-P2.2W", "PT22.22S", "P0.5Y", "-P3Y6M4DT12H30M5S",
                "P1Y", "P1M", "PT0S", "-P1DT2H3M4S", "P0018-09-04T11:09:08", "PT0.000001S", "P1.5M"]:
        expr = PythonExpression(f"{ISO}.parse_duration({dur!r})")
        for f in ["P%P", "P%p", "P%Y-%m-%dT%H:%M:%S", "P%Y%m%dT%H%M%S", "%C %W %f %d %%"]:
            same("duration_isoformat", expr, f)
        same("duration_isoformat", expr)
    # pickle round trip on each side
    same("duration_isoformat", PythonExpression(
        f"__import__('pickle').loads(__import__('pickle').dumps({ISO}.parse_duration('P1Y2M3DT4H5.5S')))"))
    same("duration.max_days_in_month", 2001, PythonExpression(
        f"1 + (__import__('pickle').loads(__import__('pickle').dumps({ISO}.UTC)) is {ISO}.UTC)"))
    for bad in [None, 5, "2020-01-01"]:
        same("strftime", bad, "%Y")
        same("tz_isoformat", bad)
        same("duration_isoformat", bad)
