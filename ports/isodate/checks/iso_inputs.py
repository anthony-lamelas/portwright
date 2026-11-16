"""Shared input builders for the isodate checks.

Values that aren't plain data (dates, timedeltas, tzinfos, Durations) are sent as
PythonExpression source, so each side builds them with its own library.
"""

from decimal import Decimal

from hypothesis import strategies as st

from portwright.compare import PythonExpression
from portwright.compare.strategies import text

DT = "__import__('datetime')"
ISO = "__import__('isodate')"

# ---------------------------------------------------------------- tz expressions


@st.composite
def tz_source(draw, allow_none=True, wild=False):
    """Source text for a tzinfo (or None)."""
    choices = ["utc", "fixed", "timezone", "local", "fixed_utc"]
    if allow_none:
        choices.append("none")
    kind = draw(st.sampled_from(choices))
    if kind == "none":
        return "None"
    if kind == "utc":
        return f"{ISO}.UTC"
    if kind == "local":
        return f"{ISO}.LOCAL"
    if kind == "fixed_utc":
        return f"{ISO}.FixedOffset()"
    if kind == "fixed":
        hmax = 120 if wild else 23
        h = draw(st.integers(-hmax, hmax))
        m = draw(st.integers(-59, 59) if wild else st.integers(0, 59))
        name = draw(st.sampled_from(["+01:00", "X", "", "EST"]))
        return f"{ISO}.FixedOffset({h}, {m}, {name!r})"
    seconds = draw(st.integers(-86399, 86399))
    if draw(st.booleans()):
        seconds = int(seconds / 60) * 60
    return f"{DT}.timezone({DT}.timedelta(seconds={seconds}))"


# ---------------------------------------------------------- date/time expressions

years = st.one_of(st.integers(1, 9999), st.sampled_from([1, 99, 100, 999, 1000, 1582, 1900, 2000, 2004, 9999]))


@st.composite
def date_parts(draw):
    y = draw(years)
    m = draw(st.integers(1, 12))
    d = draw(st.integers(1, 28)) if draw(st.integers(0, 3)) else draw(st.integers(28, 31))
    # clamp invalid day by stepping back
    import calendar

    d = min(d, calendar.monthrange(y, m)[1])
    return y, m, d


@st.composite
def date_expr(draw):
    y, m, d = draw(date_parts())
    return PythonExpression(f"{DT}.date({y}, {m}, {d})")


@st.composite
def time_parts(draw):
    return (
        draw(st.integers(0, 23)),
        draw(st.integers(0, 59)),
        draw(st.integers(0, 59)),
        draw(st.one_of(st.just(0), st.integers(0, 999999))),
    )


@st.composite
def datetime_expr(draw, wild_tz=False):
    y, m, d = draw(date_parts())
    H, M, S, us = draw(time_parts())
    tz = draw(tz_source(wild=wild_tz))
    cls = f"{DT}.datetime"
    if wild_tz or "LOCAL" in tz:
        # a subclass, so the worker describes it by repr instead of calling utcoffset
        cls = f"type('WildDateTime', ({DT}.datetime,), {{}})"
    return PythonExpression(f"{cls}({y}, {m}, {d}, {H}, {M}, {S}, {us}, tzinfo={tz})")


@st.composite
def local_datetime_expr(draw):
    """Datetimes in a range mktime handles everywhere."""
    y = draw(st.integers(1971, 2037))
    m = draw(st.integers(1, 12))
    d = draw(st.integers(1, 28))
    H, M, S, us = draw(time_parts())
    tz = draw(tz_source())
    return PythonExpression(f"{DT}.datetime({y}, {m}, {d}, {H}, {M}, {S}, {us}, tzinfo={tz})")


@st.composite
def time_expr(draw, wild_tz=False):
    H, M, S, us = draw(time_parts())
    tz = draw(tz_source(wild=wild_tz))
    cls = f"{DT}.time"
    if wild_tz or "LOCAL" in tz:
        # time.isoformat() would call LOCAL.utcoffset(None) or reject the offset
        cls = f"type('WildTime', ({DT}.time,), {{}})"
    return PythonExpression(f"{cls}({H}, {M}, {S}, {us}, tzinfo={tz})")


@st.composite
def timedelta_expr(draw):
    days = draw(st.one_of(st.integers(-1000, 1000), st.integers(-999999999, 999999999), st.just(0)))
    secs = draw(st.integers(-200000, 200000))
    us = draw(st.integers(-2000000, 2000000))
    return PythonExpression(f"{DT}.timedelta({days}, {secs}, {us})")


# ---------------------------------------------------------- Duration arguments

decimal_nums = st.one_of(
    st.integers(-30, 30).map(Decimal),
    st.sampled_from([Decimal("0.5"), Decimal("-1.5"), Decimal("1E+1"), Decimal("-0"), Decimal("12.0"),
                     Decimal("2.25"), Decimal("100000000000000000000")]),
)


def month_year_values(finite=True):
    base = [
        st.integers(-40, 40),
        st.integers(-10**6, 10**6),
        decimal_nums,
        st.sampled_from([0.5, -0.5, 1.0, 2.0, 1e20, -0.0, 1.25, 3.0]),
    ]
    if not finite:
        base.append(st.sampled_from([float("nan"), float("inf"), Decimal("NaN"), Decimal("-Infinity")]))
    return st.one_of(*base)


td_values = st.one_of(
    st.integers(-400, 400),
    st.integers(-10**6, 10**6),
    st.sampled_from([0.5, -0.25, 1.5, 0.001, 1e-7, 2.7]),
)


@st.composite
def duration_kwargs(draw, finite=True):
    kw = {}
    for name in ["days", "seconds", "microseconds", "milliseconds", "minutes", "hours", "weeks"]:
        if draw(st.integers(0, 3)) == 0:
            kw[name] = draw(td_values)
    for name in ["months", "years"]:
        if draw(st.integers(0, 2)) == 0:
            kw[name] = draw(month_year_values(finite))
    return kw


def _lit(v):
    if isinstance(v, Decimal):
        return f"__import__('decimal').Decimal({str(v)!r})"
    if isinstance(v, float):
        if v != v:
            return "float('nan')"
        if v in (float("inf"), float("-inf")):
            return f"float({str(v)!r})"
    return repr(v)


def duration_source(kw):
    inner = ", ".join(f"{k}={_lit(v)}" for k, v in kw.items())
    return f"{ISO}.Duration({inner})"


@st.composite
def duration_expr(draw, finite=True):
    return PythonExpression(duration_source(draw(duration_kwargs(finite))))


# ---------------------------------------------------------- ISO strings


def digits(n_min, n_max):
    return st.integers(n_min, n_max)


def padded(value, width):
    return str(value).zfill(width)


@st.composite
def frac(draw):
    """Optional decimal fraction like ',5' or '.000001'."""
    if draw(st.integers(0, 2)):
        return ""
    sep = draw(st.sampled_from([".", ","]))
    n = draw(st.one_of(st.integers(1, 7), st.integers(8, 35)))
    ds = draw(st.text(alphabet="0123456789", min_size=n, max_size=n))
    if draw(st.integers(0, 4)) == 0:
        ds = draw(st.sampled_from(["5", "999999", "9999999", "0000005", "4999995", "99999999999", "1"]))
    return sep + ds


@st.composite
def date_string(draw, yeardigits=4, expanded=False):
    """A date in one of the ISO forms parse_date knows, with near-miss values."""
    sign = ""
    if expanded or yeardigits != 4:
        sign = draw(st.sampled_from(["+", "-", "+", "+"]))
    elif draw(st.integers(0, 10)) == 0:
        sign = draw(st.sampled_from(["+", "-"]))
    yd = max(yeardigits, 0)
    width = yd if draw(st.integers(0, 8)) else max(0, yd + draw(st.sampled_from([-1, 1])))
    year = draw(st.integers(0, 10**max(width, 1) - 1))
    if draw(st.integers(0, 3)):
        year = draw(st.integers(1, min(9999, 10**max(width, 1) - 1)))
    Y = padded(year, width) if width else ""
    MM = padded(draw(st.integers(1, 12) if draw(st.integers(0, 5)) else st.integers(0, 99)), 2)
    DD = padded(draw(st.integers(1, 28) if draw(st.integers(0, 4)) else st.integers(0, 39)), 2)
    WW = padded(draw(st.integers(1, 52) if draw(st.integers(0, 4)) else st.integers(0, 99)), 2)
    D = str(draw(st.integers(1, 7) if draw(st.integers(0, 4)) else st.integers(0, 9)))
    DDD = padded(draw(st.integers(1, 365) if draw(st.integers(0, 4)) else st.integers(0, 999)), 3)
    cw = max(0, yd - 2)
    CC = padded(draw(st.integers(0, 10**max(cw, 1) - 1)), cw) if cw else ""
    form = draw(st.integers(0, 11))
    body = [
        f"{Y}-{MM}-{DD}", f"{Y}{MM}{DD}", f"{Y}-W{WW}-{D}", f"{Y}W{WW}{D}", f"{Y}-{DDD}", f"{Y}{DDD}",
        f"{Y}-W{WW}", f"{Y}W{WW}", f"{Y}-{MM}", f"{Y}{MM}", f"{Y}", f"{CC}",
    ][form]
    return sign + body


@st.composite
def tz_string(draw):
    kind = draw(st.integers(0, 5))
    if kind == 0:
        return ""
    if kind == 1:
        return "Z"
    sign = draw(st.sampled_from(["+", "-"]))
    hh = padded(draw(st.integers(0, 14) if draw(st.integers(0, 4)) else st.integers(0, 99)), 2)
    mm = padded(draw(st.sampled_from([0, 30, 45]) if draw(st.integers(0, 3)) else st.integers(0, 99)), 2)
    return [f"{sign}{hh}", f"{sign}{hh}{mm}", f"{sign}{hh}:{mm}", f"{sign}{hh}:{mm}"][kind - 2]


@st.composite
def time_string(draw, allow_t=True):
    hh = padded(draw(st.integers(0, 23) if draw(st.integers(0, 6)) else st.integers(0, 99)), 2)
    mm = padded(draw(st.integers(0, 59) if draw(st.integers(0, 6)) else st.integers(0, 99)), 2)
    ss = padded(draw(st.integers(0, 59) if draw(st.integers(0, 6)) else st.integers(0, 99)), 2)
    f = draw(frac())
    form = draw(st.integers(0, 4))
    body = [f"{hh}:{mm}:{ss}{f}", f"{hh}{mm}{ss}{f}", f"{hh}:{mm}{f}", f"{hh}{mm}{f}", f"{hh}{f}"][form]
    prefix = "T" if allow_t and draw(st.integers(0, 4)) == 0 else ""
    return prefix + body + draw(tz_string())


def mutate(base):
    """Near misses: insert, delete, or replace a character."""

    @st.composite
    def go(draw):
        s = draw(base)
        op = draw(st.integers(0, 4))
        if op == 0 or not s:
            i = draw(st.integers(0, len(s)))
            c = draw(st.one_of(st.sampled_from(list("0123456789-+:TWZ.,PYMDHS ")), text(min_size=1, max_size=1)))
            return s[:i] + c + s[i:]
        i = draw(st.integers(0, len(s) - 1))
        if op == 1:
            return s[:i] + s[i + 1:]
        if op == 2:
            c = draw(st.one_of(st.sampled_from(list("0123456789-+:TWZ.,")), text(min_size=1, max_size=1)))
            return s[:i] + c + s[i + 1:]
        if op == 3:
            return s.lower()
        return s + draw(text(max_size=3))

    return go()


@st.composite
def duration_string(draw):
    def num(big=False):
        n = draw(st.integers(0, 10**30) if big else st.integers(0, 400))
        return str(n) + draw(frac())

    sign = draw(st.sampled_from(["", "", "", "-", "+"]))
    parts = ["P"]
    for unit in "YMWD":
        if draw(st.integers(0, 2)) == 0:
            parts.append(num(big=draw(st.integers(0, 15)) == 0 and unit in "YM") + unit)
    if draw(st.booleans()):
        parts.append("T")
        for unit in "HMS":
            if draw(st.integers(0, 2)) == 0:
                parts.append(num() + unit)
    return sign + "".join(parts)


@st.composite
def alt_duration_string(draw):
    y = padded(draw(st.integers(0, 30)), 4)
    mo = padded(draw(st.integers(0, 13)), 2)
    d = padded(draw(st.integers(0, 31)), 2)
    ddd = padded(draw(st.integers(0, 370)), 3)
    t = draw(time_string(allow_t=False))
    form = draw(st.integers(0, 3))
    return "P" + [f"{y}-{mo}-{d}T{t}", f"{y}{mo}{d}T{t}", f"{y}-{ddd}T{t}", f"{y}{ddd}T{t}"][form]
