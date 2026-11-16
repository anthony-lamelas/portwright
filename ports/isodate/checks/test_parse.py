from hypothesis import given, strategies as st

from portwright.compare import same, PythonExpression
from portwright.compare.strategies import text

from iso_inputs import (
    alt_duration_string, date_string, duration_string, mutate, time_string, tz_string,
)


def inspect_duration(result):
    """parse_duration may return a Duration object; look inside it."""
    if result is None or isinstance(result, dict):
        return
    result.get("years")
    result.get("months")
    result.get("tdelta")
    result.call("__str__")
    result.call("__repr__")


# ---------------------------------------------------------------- parse_date


@given(date_string())
def test_parse_date_default(s):
    same("parse_date", s)


@given(st.data())
def test_parse_date_yeardigits(data):
    yd = data.draw(st.sampled_from([4, 4, 2, 3, 5, 6, 1, 0, 8]))
    expanded = data.draw(st.booleans())
    s = data.draw(date_string(yd, expanded))
    kwargs = {}
    if data.draw(st.booleans()):
        kwargs["defaultmonth"] = data.draw(st.integers(1, 12) | st.integers(-1, 14))
    if data.draw(st.booleans()):
        kwargs["defaultday"] = data.draw(st.integers(1, 28) | st.integers(-1, 32))
    if data.draw(st.booleans()):
        same("parse_date", s, yd, expanded, **kwargs)
    else:
        same("parse_date", s, yeardigits=yd, expanded=expanded, **kwargs)


@given(mutate(date_string()))
def test_parse_date_near_miss(s):
    same("parse_date", s)


@given(text(max_size=12))
def test_parse_date_text(s):
    same("parse_date", s)


@given(st.sampled_from([None, b"2020-01-01", 20200101, 1.5, ["2020"]]))
def test_parse_date_bad_type(v):
    same("parse_date", v)


def test_parse_date_examples():
    for s in ["1985-04-12", "19850412", "1985-W15-5", "1985W155", "1985-102", "1985102", "1985-W15",
              "1985W15", "1985-04", "198504", "1985", "19", "2009-W01-1", "2009-W53-7", "2008-W01-1",
              "2004-W53-7", "2010-W01-1", "0000-01-01", "0001-01-01", "9999-12-31", "1985-W00-0",
              "", "+1985-04-12", "00", "99"]:
        same("parse_date", s)
    for s in ["+001985-04-12", "-001985-04-12", "+0019", "-0019", "+001985W155", "+001985", "+"]:
        same("parse_date", s, 6)
        same("parse_date", s, 6, True, 3, 4)
    same("parse_date", "+", 2)
    same("parse_date", "+1985-04-12", 4, True)
    same("parse_date", "1985", defaultmonth=2, defaultday=29)
    same("parse_date", "1984", defaultmonth=2, defaultday=29)
    same("parse_date", "1985-02", defaultday=29)
    same("parse_date", "19", defaultmonth=12, defaultday=31)


# ---------------------------------------------------------------- parse_time


@given(time_string())
def test_parse_time(s):
    r = same("parse_time", s)
    if r is not None and not isinstance(r, dict):
        r.call("isoformat")


@given(mutate(time_string()))
def test_parse_time_near_miss(s):
    same("parse_time", s)


@given(text(max_size=12))
def test_parse_time_text(s):
    same("parse_time", s)


def test_parse_time_examples():
    for s in ["232050", "23:20:50", "2320", "23:20", "23", "232050,5", "23:20:50.5", "2320,8", "23:20,8",
              "23,3", "152746+0100", "152746-0500", "152746+01", "152746-05", "15:27:46+01:00",
              "15:27:46-05:00", "15:27:46+01", "15:27:46-05", "15:27:46-05:30", "15:27:46-0545",
              "1:17:30", "T23:20:50", "23:59:59.9999999", "23:59.9999999999", "23.9999999999999",
              "12.5", "12:30.5", "00:00:00.0000005", "24:00:00", "23:60", "23:59:60", "12Z", "12+99",
              "12-99:99", "12:00:00.1234567890123456789012345678901234567890", "", "T", "15:27:46+1",
              "10:20:30Zabc", "1230,000001", "13.000000001", "10:00:00,999999999"]:
        same("parse_time", s)
    fracs = ["5", "25", "999999", "9999995", "9999994", "0000005", "0000004", "0000015", "1", "01", "001",
             "000001", "0000009", "123456789", "9999999999999999", "4999995", "5000005", "99999949", "99999951"]
    for f in fracs:
        for sep in ".,":
            for body in ["12:30:45", "123045", "12:30", "1230", "12", "23:59:59", "23:59", "23", "00:00:00", "00"]:
                same("parse_time", f"{body}{sep}{f}")
    for bad in [None, b"12:00", 12]:
        same("parse_time", bad)


# ---------------------------------------------------------------- parse_tzinfo


@given(tz_string())
def test_parse_tzinfo(s):
    tz = same("parse_tzinfo", s)
    if tz is not None and not isinstance(tz, dict):
        tz.call("utcoffset", None)
        tz.call("tzname", None)
        tz.call("dst", None)


@given(st.one_of(mutate(tz_string()), text(max_size=8)))
def test_parse_tzinfo_text(s):
    same("parse_tzinfo", s)


def test_parse_tzinfo_examples():
    for s in ["", "Z", "+01", "-01", "+0130", "-0130", "+01:30", "-01:30", "+05:3", "Zulu", "x", "+0",
              "+99:99", "-00:00", "+00"]:
        same("parse_tzinfo", s)
    for bad in [None, b"Z", 5]:
        same("parse_tzinfo", bad)


# ---------------------------------------------------------------- parse_datetime


@st.composite
def datetime_string(draw):
    d = draw(date_string())
    t = draw(time_string(allow_t=False))
    sep = "T" if draw(st.integers(0, 10)) else draw(st.sampled_from(["", " ", "TT", "t"]))
    return d + sep + t


@given(datetime_string())
def test_parse_datetime(s):
    r = same("parse_datetime", s)
    if r is not None and not isinstance(r, dict):
        r.call("isoformat")


@given(st.one_of(mutate(datetime_string()), text(max_size=20)))
def test_parse_datetime_near_miss(s):
    same("parse_datetime", s)


def test_parse_datetime_examples():
    for s in ["19850412T1015", "1985-04-12T10:15", "1985102T1015Z", "1985-102T10:15Z", "1985W155T1015+0400",
              "1985-W15-5T10:15+04", "1985-W15-5T10:15-0430", "1985-W15-5T10:15+04:45",
              "20110410T101225.123000Z", "2012-10-12T08:29:46.069178Z", "2012-10-12T08:29:46.691780Z",
              "2012-10-30T08:55:22.1234567Z", "2012-10-30T08:55:22.1234561Z", "2014-08-18 14:55:22.123456Z",
              "1985-04-12", "T10:00", "1985-04-12T10:00T", "1985-04-12T", "0000-01-01T00:00"]:
        same("parse_datetime", s)
    for bad in [None, b"1985-04-12T10:15"]:
        same("parse_datetime", bad)


# ---------------------------------------------------------------- parse_duration


@given(duration_string(), st.sampled_from([None, True, False]))
def test_parse_duration(s, as_td):
    if as_td is None:
        r = same("parse_duration", s)
    else:
        r = same("parse_duration", s, as_td)
    inspect_duration(r)


@given(alt_duration_string(), st.booleans())
def test_parse_duration_alt(s, as_td):
    r = same("parse_duration", s, as_timedelta_if_possible=as_td)
    inspect_duration(r)


@given(st.one_of(mutate(duration_string()), mutate(alt_duration_string()), text(max_size=12)))
def test_parse_duration_near_miss(s):
    same("parse_duration", s)


def test_parse_duration_examples():
    for s in ["P18Y9M4DT11H9M8S", "P2W", "P3Y6M4DT12H30M5S", "P23DT23H", "P4Y", "P1M", "PT1M", "P0.5Y",
              "PT36H", "P1DT12H", "+P11D", "-P2W", "-P2.2W", "P1DT2H3M4S", "PT2.3H", "PT22.22S", "-P2Y",
              "-P3Y6M4DT12H30M5S", "-P1DT2H3M4S", "P0018-09-04T11:09:08", "P", "PT", "P0Y", "P0M", "-PT0S",
              "P1,5Y", "P1.5M", "P0,0001D", "PT0.0000001S", "PT0.0000005S", "PT0.0000015S",
              "P999999999D", "P1000000000D", "P1Y2M3W4DT5H6M7.8S", "P0001-02-03T04:05:06",
              "P00010203T040506", "P0001-034T04:05:06", "P0001034T040506", "P0000-00-00T00:00:00",
              "PT1S2M", "P1D1Y", "PTS", "P.5Y", "P5.Y", "Px", "P1Y\n", "p1y", "-P0.5M", "P1E5Y",
              "P100000000000000000000000000000Y", "P1.00000000000000000000000000001M", "-P0Y1M", "-P0M1Y",
              "-P0Y", "-P1Y0M", "-PT0.5S", "P0Y0M", "-P0,0Y1D", "PT1000000H", "P1000000W", "PT0.9999999S"]:
        r = same("parse_duration", s)
        inspect_duration(r)
        same("parse_duration", s, False)
    for bad in [None, b"P1Y", 5, ["P1Y"]]:
        same("parse_duration", bad)
