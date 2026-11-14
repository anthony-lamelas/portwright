"""Review stage 3, finding 2: kwarg names are lossily converted (lone surrogate -> U+FFFD).
Reach every API: valid positional args + an odd keyword, and keyword-only valid calls."""
from hypothesis import given, example, strategies as st
from portwright.compare import same
from portwright.compare.strategies import text

D = (2024, 2, 29)
T = (1, 2, 3, 4, 60, 0)
DT = (2024, 2, 29, 1, 2, 3, 4, 60, 0)
TD = (1, 2, 3)
# api -> (positional args, the same args by keyword name)
CALLS = {
    "date_add": ((D, TD), {"d": D, "td": TD}),
    "date_fromisocalendar": ((2024, 9, 4), {"year": 2024, "week": 9, "day": 4}),
    "date_fromisoformat": (("2024-02-29",), {"text": "2024-02-29"}),
    "date_fromordinal": ((738945,), {"n": 738945}),
    "date_isoformat": ((D,), {"d": D}),
    "date_toordinal": ((D,), {"d": D}),
    "datetime_add": ((DT, TD), {"d": DT, "td": TD}),
    "datetime_astimezone": ((DT, -120), {"d": DT, "utcoffset": -120}),
    "datetime_compare": ((DT, DT), {"a": DT, "b": DT}),
    "datetime_fromisoformat": (("2024-02-29T01:02:03",), {"text": "2024-02-29T01:02:03"}),
    "datetime_fromtimestamp": ((1e9, 0), {"timestamp": 1e9, "utcoffset": 0}),
    "datetime_isoformat": ((DT,), {"d": DT, "sep": " ", "timespec": "seconds"}),
    "datetime_repr": ((DT,), {"d": DT}),
    "datetime_str": ((DT,), {"d": DT}),
    "datetime_subtract": ((DT, DT), {"a": DT, "b": DT}),
    "datetime_timestamp": ((DT,), {"d": DT}),
    "isocalendar": ((D,), {"d": D}),
    "isoweekday": ((D,), {"d": D}),
    "make_date": ((2024, 2, 29), {"year": 2024, "month": 2, "day": 29}),
    "make_datetime": ((2024, 2, 29), {"year": 2024, "month": 2, "day": 29, "fold": 1}),
    "make_time": ((1,), {"hour": 1, "utcoffset": 30}),
    "make_timedelta": ((1,), {"weeks": 1, "milliseconds": 5}),
    "strftime": ((DT, "%Y-%m-%d"), {"d": DT, "fmt": "%Y-%m-%d"}),
    "strptime": (("2024", "%Y"), {"text": "2024", "fmt": "%Y"}),
    "time_fromisoformat": (("01:02:03",), {"text": "01:02:03"}),
    "time_isoformat": ((T,), {"t": T, "timespec": "minutes"}),
    "timedelta_divide": ((TD, 2), {"td": TD, "divisor": 2}),
    "timedelta_floor_divide": ((TD, 2), {"td": TD, "divisor": 2}),
    "timedelta_multiply": ((TD, 2), {"td": TD, "factor": 2}),
    "timedelta_repr": ((TD,), {"td": TD}),
    "timedelta_str": ((TD,), {"td": TD}),
    "timedelta_total_seconds": ((TD,), {"td": TD}),
    "timezone_name": ((60,), {"utcoffset": 60}),
    "weekday": ((D,), {"d": D}),
}
APIS = st.sampled_from(sorted(CALLS))
KEYS = st.one_of(text(min_size=1, max_size=4),
                 st.sampled_from(["\udc80", "\ud800", "\udfff", "a\udc80", "\udc80\udc81", "�", "d\udc80",
                                  "tėxt", "ｔd", "\x00"]))


@given(APIS, KEYS, st.booleans())
@example("make_date", "\udc80", False)
@example("timezone_name", "\udc80", False)
@example("strptime", "\ud800", True)
@example("timedelta_multiply", "�", False)
def test_surrogate_kwarg_every_api(api, key, by_keyword):
    pos, kw = CALLS[api]
    if by_keyword:
        same(api, **kw)                      # valid keyword-only call
        if key not in kw:
            same(api, **{**kw, key: 1})      # bad key among valid keywords
    else:
        same(api, *pos)                      # valid positional call
        same(api, *pos, **{key: 1})


@given(APIS, st.lists(KEYS, min_size=2, max_size=3, unique=True))
@example("weekday", ["\udc80", "\udc81"])
@example("strftime", ["\udc80", "�"])
def test_two_odd_kwargs_every_api(api, keys):
    pos, _ = CALLS[api]
    same(api, *pos, **{k: i for i, k in enumerate(keys)})
