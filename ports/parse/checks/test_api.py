"""Direct checks of the helper functions and classes."""
import datetime as dt

from hypothesis import given, strategies as st
from portwright.compare import Pair, PythonExpression, same

from pw_helpers import converter, inspect_result, re_match

# ---------------------------------------------------------------- int_convert
digit_strings = st.one_of(
    st.from_regex(r"\A[-+]?(0[bBoOxX])?[0-9a-fA-F,._ ]{1,12}\Z"),
    st.integers().map(str),
    st.integers().map(hex),
    st.integers().map(bin),
    st.integers().map(oct),
    st.text(alphabet="0123456789abcdefxXoObB-+,", min_size=1, max_size=8),
)


@given(digit_strings, st.sampled_from([None, 2, 8, 10, 16, 36, 3]), st.booleans())
def test_int_convert(s, base, omit):
    conv = same("int_convert") if omit else same("int_convert", base)
    if isinstance(conv, Pair):
        conv.get("base")
        conv.call("__call__", s, None)


def test_int_convert_errors():
    conv = same("int_convert", 10)
    for s in ["", "-", "+", "abc", "-0x"]:
        conv.call("__call__", s, None)


# ---------------------------------------------------------------- convert_first / percentage
@given(st.text(alphabet="0123456789.-+e ", min_size=1, max_size=6),
       st.sampled_from(["int", "float", "lambda s: s[::-1]", "str.upper", "len"]))
def test_convert_first(s, fn):
    c = same("convert_first", PythonExpression(fn))
    if isinstance(c, Pair):
        c.call("__call__", s, None)


@given(st.one_of(st.floats(-1e9, 1e9).map(lambda f: "%r%%" % f),
                 st.text(alphabet="0123456789.%-", min_size=1, max_size=6),
                 st.sampled_from(["inf%", "nan%", "%", "1e3%", "50"])))
def test_percentage(s):
    same("percentage", s, None)


# ---------------------------------------------------------------- FixedTzOffset
@given(st.integers(-1500, 1500), st.text(max_size=6), st.integers(-1500, 1500), st.text(max_size=3))
def test_fixed_tz(off, name, off2, name2):
    tz = same("FixedTzOffset", off, name)
    if not isinstance(tz, Pair):
        return
    tz.call("utcoffset", None)
    tz.call("tzname", None)
    tz.call("dst", None)
    tz.call("__repr__")
    tz.call("utcoffset", PythonExpression("__import__('datetime').datetime(2020, 1, 1)"))
    tz.call("__eq__", same("FixedTzOffset", off, name))
    tz.call("__eq__", same("FixedTzOffset", off2, name2))
    tz.call("__eq__", 5)


def test_fixed_tz_float():
    for off in [1.5, -0.0, 90.25]:
        tz = same("FixedTzOffset", off, "x")
        tz.call("utcoffset", None)


# ---------------------------------------------------------------- with_pattern
@given(st.sampled_from([r"\d+", r"[a-z]+", r"(\d)(\d)", r"x|y", r"\w"]),
       st.sampled_from([None, 0, 1, 2]), st.text(alphabet="12ab xy", max_size=6), st.booleans())
def test_with_pattern(pattern, gc, s, omit):
    deco = same("with_pattern", pattern) if omit else same("with_pattern", pattern, gc)
    fn = deco.call("__call__", PythonExpression("lambda s: (s, len(s))"))
    fn.get("pattern")
    fn.get("regex_group_count")
    fn.call("__call__", "q")
    inspect_result(same("parse", "<{:Q}>{}", "<%s>end" % s, {"Q": fn}))
    inspect_result(same("search", "{:Q}-{x:d}", s + "-5", extra_types={"Q": fn}))
    same("findall", "{:Q}", s, extra_types={"Q": fn})


@given(st.text(alphabet="12ab -", max_size=8))
def test_extra_types(s):
    et = {
        "Num": converter("int(s)", r"\d+"),
        "Pair": converter("s.split('-')", r"(\d)-(\d)", 2),
        "Any": converter("s.upper()"),
        "d": converter("'custom:' + s", r"\d"),  # overrides builtin
        "Bad": converter("1/0", r"\d"),
    }
    for fmt in ["{:Num}", "{:Pair} {}", "{a:Any}", "{:d}{:d}", "{:Bad}", "{:Pair}{x:Num}", "{:Num}x"]:
        inspect_result(same("parse", fmt, s, et))
        inspect_result(same("search", fmt, s, extra_types=et))
        same("findall", fmt, s, extra_types=et)


def test_bad_custom_regex():
    et = {"Bad": converter("s", "(")}
    same("parse", "{:Bad}", "x", et)
    same("search", "{:Bad}", "x", extra_types=et)
    same("parse", "{:Bad}", "(", et)


# ---------------------------------------------------------------- extract_format
SPEC = st.one_of(
    st.from_regex(r"\A(.?[<>=^])?[-+ ]?0?[0-9]{0,3}[,_]?(\.[0-9]{0,3})?(d|f|s|n|x|ti|tg|%Y|%H:%M|w|l|%|)\Z"),
    st.text(min_size=1, max_size=6),
    st.sampled_from(["d", "<", "=5d", "x>10.3f", "^", "Q", "10", ".", ",", "_x", "0", "%%", "tq",
                     "%Y-%m", "3.d", "+", "-", " ", "^^", "<<", "abc%dxyz"]),
)


@given(SPEC, st.sampled_from([{}, {"Q": None}, {"abc": 1}]))
def test_extract_format(spec, extra):
    same("extract_format", spec, extra)


def test_extract_format_empty():
    same("extract_format", "", {})
    same("extract_format", "d", {})


# ---------------------------------------------------------------- datetime helpers
DIRECTIVES = ["%Y", "%y", "%m", "%d", "%H", "%M", "%S", "%f", "%b", "%B", "%a", "%A", "%j",
              "%I", "%p", "%w", "%U", "%W", "%z", "%Z", "%%", "%c", "-", " ", ":", "x", "(", "%"]


@given(st.lists(st.sampled_from(DIRECTIVES), max_size=6).map("".join))
def test_get_regex_for_datetime_format(fmt):
    same("get_regex_for_datetime_format", fmt)


@given(st.datetimes(min_value=dt.datetime(1900, 1, 1), max_value=dt.datetime(2100, 1, 1)),
       st.lists(st.sampled_from(["%Y", "%y", "%m", "%d", "%H", "%M", "%S", "%f", "%b", "%B", "%a",
                                 "%A", "%j", "%I", "%p", "%z", "-", " ", ":"]), min_size=1, max_size=5),
       st.booleans())
def test_strf_date_convert(d, parts, corrupt):
    fmt = "".join(parts)
    s = d.strftime(fmt.replace("%z", "+0200"))
    if corrupt:
        s = s + "x"
    same("strf_date_convert", s, None, fmt)
    same("strf_date_convert", s, None, type=fmt)


def test_strf_date_convert_edges():
    for s, f in [("29 Feb", "%d %b"), ("366", "%j"), ("10:30", "%H:%M"), ("x", "x"), ("", ""),
                 ("12", "%H"), ("2020", "%Y"), ("Mon", "%a"), ("31 Feb", "%d %b")]:
        same("strf_date_convert", s, None, f)


DATE_CASES = [
    # (regex, string, kwargs)
    (r"(\d{4}-\d\d-\d\d)((\s+|T)(\d{1,2}:\d{1,2}(:\d{1,2}(\.\d+)?)?))?(Z|\s*[-+]\d\d:?\d\d)?",
     ["2020-01-02", "2020-01-02T10:11", "2020-01-02 10:11:12.123+0130", "2020-13-01", "2020-01-02T1:2:3z",
      "2020-02-30", "2020-01-02T10:11-05:00", "2020-01-02T10:11+0099"],
     {"ymd": 1, "hms": 4, "tz": 7}),
    (r"((\d{1,2}|[A-Za-z]+)[-/]\d{1,2}[-/]\d{4})(\s+(\d{1,2}:\d{1,2}(:\d{1,2}(\.\d+)?)?))?(\s+[AP]M)?(\s+[-+]\d\d?:?\d\d)?",
     ["1/2/2020", "Feb/3/2020 10:00 pm", "12/31/1999 12:00 AM +1:00", "march-1-2020", "1/2/2020 12:30 PM -0530",
      "1/2/2020 13:00 PM", "Foo/1/2020"],
     {"mdy": 1, "hms": 4, "am": 7, "tz": 8}),
    (r"(\d{1,2}[-/](\d{1,2}|[A-Za-z]+)[-/]\d{4})(\s+(\d{1,2}:\d{1,2}(:\d{1,2}(\.\d+)?)?))?(\s+[AP]M)?(\s+[-+]\d\d?:?\d\d)?",
     ["2/1/2020", "3/feb/2020 1:02:03.5 AM", "31/12/1999 23:59 +130"],
     {"dmy": 1, "hms": 4, "am": 7, "tz": 8}),
    (r"(\w+)\s+(\w+)\s+(\d{1,2})\s+(\d{1,2}:\d{1,2}(:\d{1,2})?)\s+(\d{4})",
     ["Mon Mar 2 10:00:00 2020", "Sun jan 31 1:1 1999"],
     {"d_m_y": (3, 2, 6), "hms": 4}),
    (r"([A-Za-z]{3})(\s+)(\d+)(\s+)(\d{1,2}:\d{1,2}:\d{1,2})?", ["Mar 3 10:00:00", "Feb 29 ", "jan 1 1:2:3"],
     {"mm": 1, "dd": 3, "hms": 5}),
    (r"(\d{1,2}:\d{1,2}(:\d{1,2}(\.\d+)?)?)(\s+[AP]M)?(\s+[-+]\d\d?:?\d\d)?",
     ["10:00", "12:00 AM", "12:59 PM", "1:2:3.1234567 PM +0100", "23:00 -2:30", "25:00", "10:00 +0060"],
     {"hms": 0, "am": 3, "tz": 4}),
]


def test_date_convert():
    for pattern, strings, kw in DATE_CASES:
        for s in strings:
            m = re_match(pattern, s, fn="fullmatch")
            same("date_convert", s, m, **kw)
    m = re_match(r"(\d+):(\d+)( [A-Z]+)?", "10:30 EST")
    same("date_convert", "x", m, hms=None, tz=3)
    same("date_convert", "x", m, tz=3)
    same("date_convert", "x", m)
    same("date_convert", "x", re_match(r"(Z)", "z"), tz=1)
    same("date_convert", "x", re_match(r"(\d\d:\d\d)( [ap]m)", "12:05 am"), hms=1, am=2)


# ---------------------------------------------------------------- Result / Match / ResultIterator
@given(st.lists(st.one_of(st.integers(), st.text(max_size=3)), max_size=4).map(tuple),
       st.dictionaries(st.text(max_size=3), st.integers(), max_size=3),
       st.one_of(st.integers(-6, 6), st.text(max_size=3)),
       st.integers(-5, 5), st.integers(-5, 5))
def test_result(fixed, named, key, lo, hi):
    spans = {k: (0, 1) for k in named}
    r = same("Result", fixed, named, spans)
    r.get("fixed")
    r.get("named")
    r.get("spans")
    r.call("__repr__")
    r.call("__getitem__", key)
    r.call("__getitem__", lo)
    r.call("__getitem__", PythonExpression("slice(%d, %d)" % (lo, hi)))
    for k in list(named)[:1]:
        r.call("__getitem__", k)
    r.call("__contains__", key)
    r.call("__contains__", next(iter(named), "zz"))


PARSER_CASES = [
    ("{} {}", r"\A(.+?) (.+?)\Z", ["a b", "a b c", "ab"]),
    ("{a:d}-{b}", r"\A(?P<a>[-+ ]?[0-9]+)-(?P<b>.+?)\Z", ["1-x", "12-yy", "-3-z"]),
    ("{x.y}{}", r"\A(?P<x_y>.+?)(.+?)\Z", ["ab", "abc"]),
]


def test_parser_evaluate_result_and_match():
    for fmt, regex, strings in PARSER_CASES:
        p = same("Parser", fmt)
        for s in strings:
            m = re_match(regex, s)
            inspect_result(p.call("evaluate_result", m))
            mm = same("Match", p, m)
            inspect_result(mm.call("evaluate_result"))
            inspect_result(mm)
            # via parse(evaluate_result=False)
            inspect_result(p.call("parse", s, False))
            inspect_result(p.call("search", s, 0, None, False))
            inspect_result(same("parse", fmt, s, None, False))
            inspect_result(same("search", fmt, s, evaluate_result=False))
            same("findall", fmt, s, evaluate_result=False)
            same("ResultIterator", p, s, 0, len(s))
            same("ResultIterator", p, s, 1, len(s), evaluate_result=False)
            same("ResultIterator", p, s, 0, len(s), True)


@given(st.text(alphabet="ab1 2-", max_size=12), st.integers(-3, 14), st.one_of(st.none(), st.integers(-3, 14)))
def test_result_iterator(s, pos, endpos):
    for fmt in ["{:d}", "{}", "a{x}", "{:w}-{:w}", "{:^d}"]:
        p = same("Parser", fmt)
        same("ResultIterator", p, s, pos, len(s) if endpos is None else endpos)
        p.call("findall", s, pos, endpos)
        same("findall", fmt, s, pos, endpos)
        inspect_result(p.call("search", s, pos, endpos))
        inspect_result(same("search", fmt, s, pos, endpos))
        inspect_result(same("search", fmt, s, pos=pos))


def test_exceptions():
    for name in ["RepeatedNameError", "TooManyFields"]:
        same(name, "msg")
        same(name)
        same(name, "a", 1)
    same("parse", "{a:d} {a:w}", "1 x")
    same("compile", "{a} {a:d}")
    same("compile", "{a_b_}{a[b]}")
    same("compile", "{:qq}")
    same("Parser", "{:d}", None, True)


def test_parser_misc():
    for fmt in ["", "x" * 30, "{a}" * 30, "{}" * 120, "{a%d}" % 0 + "".join("{f%d}" % i for i in range(120)),
                "{a.b}{a_b}{a__b}{a-b}", "{0}{a}{1}{b[c]}", "{:%Y-%m}{}", "{:ti}{x:tg}{}"]:
        p = same("Parser", fmt)
        p.call("__repr__")
        p.get("named_fields")
        p.get("fixed_fields")
        p.get("format")
        s = "".join("x%d" % i for i in range(120))
        inspect_result(p.call("parse", s))
        inspect_result(p.call("search", s))
        p.call("findall", s)
    p = same("Parser", "{:d}", {}, False)
    p.call("findall", "1 2 3", 0, None, None, False)
    same("compile", "{:d}", case_sensitive=True)
    same("compile", "x")


def test_bad_inputs():
    same("parse", None, "x")
    same("parse", "{}", None)
    same("parse", "{}", b"x")
    same("parse", 5, "x")
    same("search", "{}", "x", "a")
    same("compile", ["{}"])
    same("findall", "{}", 3)
