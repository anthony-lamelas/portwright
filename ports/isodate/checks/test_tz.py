from hypothesis import given, strategies as st

from portwright.compare import same, get, PythonExpression
from portwright.compare.strategies import text

from iso_inputs import DT, ISO, datetime_expr, local_datetime_expr, time_expr

UTC_SRC = f"{ISO}.UTC"
LOCAL_SRC = f"{ISO}.LOCAL"
NEW_UTC = f"{ISO}.tzinfo.Utc()"
NEW_LOCAL = f"{ISO}.tzinfo.LocalTimezone()"

dt_args = st.one_of(st.none(), datetime_expr(), time_expr())


@given(st.one_of(st.integers(-30, 30), st.integers(-200, 200), st.floats(-30, 30, allow_nan=False)),
       st.one_of(st.integers(-90, 90), st.floats(-90, 90, allow_nan=False)),
       text(max_size=8), dt_args, st.integers(0, 3))
def test_fixed_offset(h, m, name, dt, mode):
    if mode == 0:
        tz = same("FixedOffset")
    elif mode == 1:
        tz = same("FixedOffset", h)
    elif mode == 2:
        tz = same("FixedOffset", h, m)
    else:
        tz = same("FixedOffset", offset_hours=h, offset_minutes=m, name=name)
    if tz is None:
        return
    tz.call("utcoffset", dt)
    tz.call("tzname", dt)
    tz.call("dst", dt)
    tz.call("__repr__")
    for fmt in ("%Z", "%z", "%h"):
        same("tz_isoformat", PythonExpression(
            f"type('W', ({DT}.datetime,), {{}})(2000, 1, 1, tzinfo={ISO}.FixedOffset({h!r}, {m!r}, 'n'))"), fmt)


def test_fixed_offset_bad():
    same("FixedOffset", "x")
    same("FixedOffset", None)
    same("FixedOffset", 1, "x")
    same("FixedOffset", 10**12)
    same("FixedOffset", 1, 2, None).call("tzname", None)
    same("FixedOffset", 1, 2, 5).call("__repr__")


@given(dt_args)
def test_utc(dt):
    u = same("tzinfo.Utc")
    u.call("utcoffset", dt)
    u.call("tzname", dt)
    u.call("dst", dt)
    u.call("__reduce__")
    for src in (UTC_SRC, NEW_UTC):
        inst = PythonExpression(src)
        same("tzinfo.Utc.utcoffset", inst, dt)
        same("tzinfo.Utc.tzname", inst, dt)
        same("tzinfo.Utc.dst", inst, dt)
    g = get("UTC")
    g.call("utcoffset", dt)
    g.call("tzname", dt)
    g.call("dst", dt)


@given(local_datetime_expr())
def test_local(dt):
    for src in (LOCAL_SRC, NEW_LOCAL):
        inst = PythonExpression(src)
        same("tzinfo.LocalTimezone.utcoffset", inst, dt)
        same("tzinfo.LocalTimezone.tzname", inst, dt)
        same("tzinfo.LocalTimezone.dst", inst, dt)
    l = same("tzinfo.LocalTimezone")
    l.call("utcoffset", dt)
    l.call("tzname", dt)
    l.call("dst", dt)
    g = get("LOCAL")
    g.call("utcoffset", dt)
    same("tz_isoformat", PythonExpression(
        f"{DT}.datetime(2000, 7, 1, 12, tzinfo={ISO}.LOCAL)"))


def test_local_bad():
    inst = PythonExpression(LOCAL_SRC)
    same("tzinfo.LocalTimezone.utcoffset", inst, None)
    same("tzinfo.LocalTimezone.dst", inst, PythonExpression(f"{DT}.time(1, 2)"))
    same("tzinfo.LocalTimezone.tzname", inst, PythonExpression(f"{DT}.date(2000, 1, 1)"))


@given(st.one_of(st.none(), st.sampled_from(["", "Z", "+01:00", "-0530", "+05", "x"]), text(max_size=5)),
       st.sampled_from(["+", "-", None, "x"]), st.integers(-30, 30), st.integers(-90, 90), st.integers(0, 4))
def test_build_tzinfo(name, sign, h, m, mode):
    if mode == 0:
        tz = same("isotzinfo.build_tzinfo", name)
    elif mode == 1:
        tz = same("isotzinfo.build_tzinfo", name, sign)
    elif mode == 2:
        tz = same("isotzinfo.build_tzinfo", name, sign, h)
    else:
        tz = same("isotzinfo.build_tzinfo", name, sign, h, m)
    if tz is not None and not isinstance(tz, dict):
        tz.call("utcoffset", None)
        tz.call("tzname", None)


def test_build_regexps():
    for args in [(), (4,), (4, False), (4, True), (2,), (6,), (6, False), (3, True), (5,), (1,), (0,)]:
        same("isodates.build_date_regexps", *args)
    same("isodates.build_date_regexps", yeardigits=6, expanded=True)
    same("isotime.build_time_regexps")
    same("isotime.build_time_regexps")


def test_iso8601error():
    get("ISO8601Error")
    same("ISO8601Error", "boom")
    same("ISO8601Error")
    same("ISO8601Error", "a", 1)


def test_tz_isoformat_boundaries():
    W = f"type('W', ({DT}.datetime,), {{}})"
    for h, m in [(99, 59), (100, 0), (-99, -59), (-100, 0), (99, 0), (0, 0), (0, 1), (0, -1), (-1, 30),
                 (1, -30), (23, 59), (-23, -59), (24, 0), (5, 45), (-5, -45), (0, 60), (0, 59.5)]:
        for fmt in ("%Z", "%z", "%h", "%x", ""):
            same("tz_isoformat", PythonExpression(f"{W}(2000, 1, 1, tzinfo={ISO}.FixedOffset({h}, {m}, 'n'))"), fmt)
        same("tz_isoformat", PythonExpression(f"{W}(2000, 1, 1, tzinfo={ISO}.FixedOffset({h}, {m}, 'n'))"))
    for secs in [-86399, -1, 0, 1, 59, 61, 3599, 3600, 86399, -3601]:
        e = PythonExpression(f"{DT}.datetime(2000, 1, 1, tzinfo={DT}.timezone({DT}.timedelta(seconds={secs})))")
        same("tz_isoformat", e)
        same("datetime_isoformat", e)
        same("strftime", e, "%h|%z|%Z")
    same("tz_isoformat", PythonExpression(f"{DT}.datetime(2000, 1, 1, tzinfo={DT}.timezone.utc)"))
    same("tz_isoformat", PythonExpression(f"{DT}.time(1, 2, tzinfo={ISO}.FixedOffset(-3, 0, 'x'))"), "%z")
