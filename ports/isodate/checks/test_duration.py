from decimal import Decimal

from hypothesis import given, strategies as st

from portwright.compare import same, PythonExpression
from portwright.compare.strategies import text

from iso_inputs import (
    DT, ISO, date_expr, datetime_expr, duration_kwargs, duration_source, month_year_values, td_values,
    timedelta_expr,
)


def is_pair(v):
    return v is not None and not isinstance(v, dict)


def finite(kw):
    for k in ("months", "years"):
        v = kw.get(k, 0)
        try:
            if v != v or abs(v) == float("inf"):
                return False
        except Exception:
            return False
    return True


def make(kw, positional):
    if positional:
        order = ["days", "seconds", "microseconds", "milliseconds", "minutes", "hours", "weeks", "months", "years"]
        args = [kw.get(name, 0) for name in order]
        while args and args[-1] == 0 and type(args[-1]) is int:
            args.pop()
        return same("Duration", *args)
    return same("Duration", **kw)


def inspect(d, kw):
    for attr in ("years", "months", "tdelta", "days", "seconds", "microseconds"):
        d.get(attr)
    d.call("__str__")
    d.call("__repr__")
    d.call("total_seconds")
    d.call("__getstate__")
    if finite(kw):
        d.call("__hash__")


@given(duration_kwargs(finite=False), st.booleans())
def test_duration_construct(kw, positional):
    d = make(kw, positional)
    if is_pair(d):
        inspect(d, kw)
        d.call("__neg__")


@given(st.sampled_from(["x", None, [1], b"1"]), st.sampled_from(["days", "months", "years", "hours"]))
def test_duration_bad_args(v, name):
    same("Duration", **{name: v})


@given(duration_kwargs(), duration_kwargs(), st.integers(-5, 5) | st.integers(-10**6, 10**6))
def test_duration_arith(kw1, kw2, k):
    a = make(kw1, False)
    b = make(kw2, False)
    if not (is_pair(a) and is_pair(b)):
        return
    for op in ("__add__", "__radd__", "__sub__", "__rsub__", "__eq__", "__ne__"):
        a.call(op, b)
    s = a.call("__add__", b)
    if is_pair(s):
        inspect(s, {})
    diff = a.call("__sub__", b)
    if is_pair(diff):
        inspect(diff, {})
    for op in ("__mul__", "__rmul__"):
        m = a.call(op, k)
        if is_pair(m):
            m.call("__repr__")
            m.call("__str__")
    a.call("__mul__", 1.5)
    a.call("__mul__", True)
    a.call("__neg__")
    a.call("__eq__", a.call("__neg__"))


@given(duration_kwargs(), timedelta_expr())
def test_duration_timedelta(kw, td):
    a = make(kw, False)
    if not is_pair(a):
        return
    for op in ("__add__", "__radd__", "__sub__", "__rsub__", "__eq__", "__ne__"):
        r = a.call(op, td)
        if is_pair(r):
            r.call("__repr__")
            r.call("__str__")


@given(duration_kwargs(), st.one_of(date_expr(), datetime_expr()))
def test_duration_dates(kw, d):
    a = make(kw, False)
    if not is_pair(a):
        return
    a.call("__add__", d)
    a.call("__radd__", d)
    a.call("__rsub__", d)
    a.call("__sub__", d)
    a.call("totimedelta", d)
    a.call("totimedelta", start=d)
    a.call("totimedelta", end=d)
    a.call("totimedelta", None, d)


@given(duration_kwargs())
def test_duration_totimedelta_errors(kw):
    a = make(kw, False)
    if not is_pair(a):
        return
    a.call("totimedelta")
    d = PythonExpression(f"{DT}.date(2000, 2, 29)")
    a.call("totimedelta", d, d)
    a.call("totimedelta", start=d)


@given(duration_kwargs(), st.one_of(st.integers(), st.none(), text(max_size=3), st.floats(allow_nan=False),
                                     st.just([1]), st.just(Decimal("1"))))
def test_duration_other_types(kw, other):
    a = make(kw, False)
    if not is_pair(a):
        return
    for op in ("__add__", "__sub__", "__rsub__", "__eq__", "__ne__", "__mul__"):
        a.call(op, other)


def test_duration_examples():
    pairs = [
        ("P5Y7M1DT9H45M16.72S", "PT27M24.68S"),
        ("PT28M12.73S", "PT56M29.92S"),
        ("P3Y7M23DT5H25M0.33S", "PT1H1.95S"),
        ("P1332DT55M0.33S", "PT1H1.95S"),
        ("P1Y1M", "P1M"),
        ("P12M", "P1Y"),
        ("P1M", "P31D"),
    ]
    for s1, s2 in pairs:
        a = same("parse_duration", s1, False)
        b = same("parse_duration", s2, False)
        for op in ("__add__", "__sub__", "__eq__", "__ne__"):
            r = a.call(op, b)
            if is_pair(r):
                r.call("__repr__")
                r.call("__str__")
    dates = ["date(2000, 2, 29)", "date(2001, 1, 31)", "date(1, 1, 1)", "date(9999, 12, 31)",
             "datetime(2004, 1, 31, 23, 59, 59, 999999)", "datetime(2003, 3, 31, 0, 0, tzinfo=__import__('isodate').UTC)"]
    durs = ["P1M", "P1Y", "-P1M", "P13M", "-P13M", "P1Y1M1DT1H1M1S", "P0.5Y", "PT1S", "-PT1S", "P1M1D",
            "P10000Y", "-P1Y"]
    for dd in dates:
        for du in durs:
            a = same("parse_duration", du, False)
            e = PythonExpression(f"{DT}.{dd}")
            a.call("__radd__", e)
            a.call("__rsub__", e)
            a.call("totimedelta", e)
            a.call("totimedelta", end=e)
    tds = ["P1D", "PT1H", "P7D", "PT0S", "-P1D"]
    for t in tds:
        a = same("Duration", 0)
        b = same("parse_duration", t)
        a.call("__eq__", b)
    same("Duration", 1, 2, 3, 4, 5, 6, 7, 8, 9).call("__str__")
    same("Duration", months=1).call("__str__")
    same("Duration", months=2).call("__str__")
    same("Duration", months=-3).call("__str__")
    same("Duration", years=Decimal("NaN")).call("__str__")
    same("Duration").call("__setstate__", {"years": Decimal(3)})


# ------------------------------------------------------------ helper functions


decimals = st.one_of(
    st.integers(-10**6, 10**6).map(Decimal),
    st.decimals(allow_nan=False, allow_infinity=False, places=3, min_value=-10**6, max_value=10**6),
    st.sampled_from([Decimal("13"), Decimal("1"), Decimal("0"), Decimal("12.5"), Decimal("-0"), Decimal("1E+3")]),
)


@given(decimals, st.one_of(st.just(Decimal(1)), decimals), st.one_of(st.just(Decimal(13)), decimals))
def test_fquotmod(val, low, high):
    same("duration.fquotmod", val, low, high)


def test_fquotmod_examples():
    for v in range(-30, 30):
        same("duration.fquotmod", Decimal(v), Decimal(1), Decimal(13))
    same("duration.fquotmod", Decimal(5), Decimal(1), Decimal(1))
    same("duration.fquotmod", 5, 1, 13)
    same("duration.fquotmod", Decimal(5), 1, 13)
    same("duration.fquotmod", Decimal("NaN"), Decimal(1), Decimal(13))
    same("duration.fquotmod", 5.5, Decimal(1), Decimal(13))


@given(st.one_of(st.integers(-3000, 3000), st.integers(), st.sampled_from([1900, 2000, 2100, 2400, 0, -100, 4, -4])),
       st.one_of(st.integers(0, 14), st.integers()))
def test_max_days_in_month(year, month):
    same("duration.max_days_in_month", year, month)


def test_max_days_examples():
    for year in list(range(-800, 2801, 100)) + [1, 2, 3, 4, 1996, 1999, 2004, -4, -1]:
        for month in (1, 2, 3, 4, 13, 0):
            same("duration.max_days_in_month", year, month)
    for start in ["date(2000, 1, 31)", "date(1900, 1, 31)", "date(2100, 1, 31)", "date(2400, 3, 31)", "date(2004, 1, 30)"]:
        for months in (1, 13, -1, -11, 25):
            d = same("Duration", months=months)
            d.call("__radd__", PythonExpression(f"{DT}.{start}"))
            d.call("__rsub__", PythonExpression(f"{DT}.{start}"))
    same("duration.max_days_in_month", 2000, Decimal(2))
    same("duration.max_days_in_month", Decimal(2000), 2)
    same("duration.max_days_in_month", 2001.0, 2)
    same("duration.max_days_in_month", "x", 2)
    same("duration.max_days_in_month", 2004, 1.0)
