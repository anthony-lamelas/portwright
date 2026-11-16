"""Review findings: pickling compatibility (reduce shape, pickle bytes, loading
upstream-made pickles) and big-int str conversion honouring sys.set_int_max_str_digits.

Values that aren't returned by an isodate API are passed through ``str`` via
``<api>.__name__.__class__`` (str) for the API the probe is about, so
each side evaluates the expression with its own library and the text is compared."""

from contextlib import contextmanager

from hypothesis import example, given, settings, strategies as st

from portwright.compare import same, PythonExpression
from portwright.compare.strategies import text

from iso_inputs import ISO

PK = "__import__('pickle')"
CP = "__import__('copy')"
SYS = "__import__('sys')"


def api_for(src):
    """The isodate API a probe is about; its text is routed through that API's
    ``__name__.__class__`` (str), so a difference is filed under it."""
    for key, api in (("parse_duration", "parse_duration"), ("parse_datetime", "parse_datetime"),
                     ("Duration", "Duration"), ("tdelta", "Duration"), ("FixedOffset", "FixedOffset"),
                     ("LocalTimezone", "LOCAL.__class__"), ("LOCAL", "LOCAL.__class__"),
                     ("Utc", "UTC.__class__"), ("UTC", "UTC.__class__")):
        if key in src:
            return api
    return "duration_isoformat"


def show(src, api=None):
    """str() of an expression evaluated on each side."""
    return same(f"{api or api_for(src)}.__name__.__class__", PythonExpression(
        f"__import__('re').sub(r' at 0x[0-9a-fA-F]+', ' at 0x', str({src}))"))


# ------------------------------------------------------------------ pickling

# Upstream builds Duration years/months from strings; keep the args as source text.
dur_src = st.builds(
    lambda y, m, d, s: f"{ISO}.Duration(days={d}, seconds={s}, years={y}, months={m})",
    st.sampled_from(["0", "3", "-2", "1.5", "__import__('decimal').Decimal('2.25')", "10**30"]),
    st.sampled_from(["0", "7", "-1", "0.5", "__import__('decimal').Decimal('-1.5')"]),
    st.integers(-1000, 1000),
    st.integers(-90000, 90000),
)

tz_src = st.one_of(
    st.builds(lambda h, m, n: f"{ISO}.FixedOffset({h}, {m}, {n!r})",
              st.integers(-14, 14), st.integers(-59, 59), text(max_size=6)),
    st.sampled_from([f"{ISO}.UTC", f"{ISO}.tzinfo.Utc()", f"{ISO}.LOCAL", f"{ISO}.tzinfo.LocalTimezone()",
                     f"{ISO}.FixedOffset()", f"{ISO}.tzinfo.FixedOffset(0, 0, 'UTC')"]),
)

obj_src = st.one_of(dur_src, tz_src, st.sampled_from([f"{ISO}.parse_duration('P1Y2M3DT4H5.5S')",
                                                      f"{ISO}.parse_datetime('2020-01-02T03:04:05+02:30')"]))


@settings(max_examples=60)
@example(f"{ISO}.Duration(years=3, days=2)", 2)
@example(f"{ISO}.Duration(years=3, days=2)", 0)
@example(f"{ISO}.FixedOffset(2, 30, 'x')", 2)
@example(f"{ISO}.FixedOffset(2, 30, 'x')", 0)
@example(f"{ISO}.LOCAL", 2)
@example(f"{ISO}.UTC", 2)
@given(obj_src, st.integers(0, 5))
def test_reduce_ex(src, proto):
    # the reduce tuple: callable (by qualname), args and state must match upstream's
    show(f"(lambda r: (getattr(r[0], '__module__', None), getattr(r[0], '__qualname__', r[0]), r[1:]))"
         f"({src}.__reduce_ex__({proto}))")
    show(f"(lambda r: r if isinstance(r, str) else (getattr(r[0], '__module__', None), "
         f"getattr(r[0], '__qualname__', r[0]), r[1:]))({src}.__reduce__())")
    # pickle bytes name the same module/callables as upstream
    show(f"{PK}.dumps({src}, {proto})")
    # round trips on each side
    show(f"repr({PK}.loads({PK}.dumps({src}, {proto})))")
    show(f"repr({CP}.copy({src})) + repr({CP}.deepcopy({src}))")
    show(f"sorted(getattr({PK}.loads({PK}.dumps({src}, {proto})), '__dict__', {{}}).items())")


# Pickles produced by upstream isodate (inputs only: these are byte strings any
# isodate user may have stored). Each side must load them with its own library.
UPSTREAM_PICKLES = [
    # FixedOffset(2, 30, 'x'), protocol 0
    b"cisodate.tzinfo\nFixedOffset\np0\n(tRp1\n(dp2\nV_FixedOffset__offset\np3\ncdatetime\ntimedelta\np4\n(I0\nI9000\nI0\ntp5\nRp6\nsV_FixedOffset__name\np7\nVx\np8\nsb.",
    # Duration(years=3, days=2), protocol 0
    b"ccopy_reg\n_reconstructor\np0\n(cisodate.duration\nDuration\np1\nc__builtin__\nobject\np2\nNtp3\nRp4\n(dp5\nVmonths\np6\ncdecimal\nDecimal\np7\n(V0\np8\ntp9\nRp10\nsVyears\np11\ng7\n(V3\np12\ntp13\nRp14\nsVtdelta\np15\ncdatetime\ntimedelta\np16\n(I2\nI0\nI0\ntp17\nRp18\nsb.",
]


def test_load_upstream_pickles():
    for data in UPSTREAM_PICKLES:
        show(f"repr({PK}.loads({data!r}))")
        show(f"(lambda o: (repr(o), o.utcoffset(None) if hasattr(o, 'utcoffset') else o.tdelta, "
             f"{ISO}.duration_isoformat(o) if hasattr(o, 'tdelta') else {ISO}.tz_isoformat("
             f"__import__('datetime').datetime(2000, 1, 1, tzinfo=o))))({PK}.loads({data!r}))")


@given(st.sampled_from(["FixedOffset", "Duration", "tzinfo.FixedOffset", "duration.Duration", "tzinfo.Utc",
                        "tzinfo.LocalTimezone", "tzinfo._Utc", "tzinfo.FixedOffset.__reduce__"]))
def test_pickle_names_reachable(name):
    # names upstream pickles reference must resolve through the submodule import path
    mod, _, attr = name.partition(".") if name.count(".") else ("", "", name)
    show(f"__import__('importlib').import_module('isodate' + ('.' + {mod!r} if {mod!r} else '')).__name__ + "
         f"str(hasattr(__import__('importlib').import_module('isodate' + ('.' + {mod!r} if {mod!r} else '')), "
         f"{attr.split('.')[0]!r}))")


fixed_src = st.builds(lambda h, m, n: f"{ISO}.FixedOffset({h}, {m}, {n!r})",
                      st.integers(-14, 14), st.integers(-59, 59), text(max_size=6))


@example(f"{ISO}.FixedOffset(2, 30, 'x')", 2, 1)
@given(fixed_src, st.integers(0, 5), st.integers(-3, 3))
def test_fixed_offset_pickle_state_setstate(src, proto, hours):
    # pickle loading of a FixedOffset goes through instance __dict__
    show(f"(lambda o: (o.__dict__.update({{'_FixedOffset__offset': __import__('datetime').timedelta(hours={hours})}}),"
         f" repr(o), o.utcoffset(None))[1:])({PK}.loads({PK}.dumps({src}, {proto})))")


# ------------------------------------------------------------- int str limit

@contextmanager
def int_digits(limit):
    set_digits(limit)
    try:
        yield
    finally:
        set_digits(4300)


def set_digits(limit):
    show(f"{SYS}.set_int_max_str_digits({limit})", "duration_isoformat")


FIELDS = st.sampled_from(["years", "months"])
SIGNS = st.sampled_from(["", "-"])
EXTRAS = st.sampled_from([{}, {"days": 1}, {"months": 3}, {"seconds": 5}, {"years": 2}])
DIGIT_FORMATS = st.sampled_from(["%Y", "%m", "P%P", "P%p", "P%Y-%m-%dT%H:%M:%S", "P%Y%m%dT%H%M%S", "%Y %m %%"])
LIMITS = st.sampled_from([0, 0, 640, 4300, 5000, 7000])


def check_big(d, fmt):
    same("duration_isoformat", d, fmt)
    same("duration_isoformat", d)
    same("strftime", d, fmt)
    d.call("__str__")
    d.call("__repr__")
    d.call("__neg__")


@settings(max_examples=40)
@example(0, 0, "years", 5000, "", {}, "%Y")
@example(0, 0, "years", 5000, "", {}, "P%P")
@example(0, 4300, "years", 5000, "", {}, "%Y")
@example(0, 4300, "years", 5000, "", {}, "P%P")
@example(0, 0, "months", 5000, "-", {}, "%m")
@example(5000, 5000, "years", 4999, "", {}, "%Y")
@example(5000, 4300, "years", 4999, "", {}, "%Y")
@example(4300, 4300, "years", 4299, "", {}, "%Y")
@example(4300, 4300, "years", 4299, "", {}, "P%P")
@example(5000, 4300, "months", 4299, "", {"years": 2}, "P%P")
# n >= 3914: the worker shows ints under 13000 bits with str(), which would itself hit the limit
@given(LIMITS.filter(lambda x: x != 640), LIMITS, FIELDS, st.integers(3914, 7100), SIGNS, EXTRAS, DIGIT_FORMATS)
def test_int_max_str_digits(build_limit, use_limit, field, n, sign, extra, fmt):
    # Huge years/months: construction and formatting follow the interpreter's current
    # int->str limit (upstream stores Decimal(str(years)), formats with %d / str()).
    with int_digits(build_limit):
        kwargs = dict(extra)
        kwargs[field] = PythonExpression(f"{sign}10**{n}")
        d = same("Duration", **kwargs)
        if d is None:
            return
        set_digits(use_limit)
        check_big(d, fmt)


@example(0, 0, 6000)
@example(0, 4300, 6000)
@given(LIMITS.filter(lambda x: x != 640), LIMITS, st.integers(4000, 6500))
def test_int_max_str_digits_multiply(build_limit, use_limit, n):
    # Duration * big int, then formatted
    with int_digits(build_limit):
        base = same("Duration", years=1, months=2)
        d = base.call("__mul__", PythonExpression(f"10**{n}"))
        r = base.call("__rmul__", PythonExpression(f"-10**{n}"))
        set_digits(use_limit)
        for x in (d, r):
            if x is not None:
                check_big(x, "P%P")
                same("duration_isoformat", x, "%Y-%m")
