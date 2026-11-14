"""Review stage: targets the hand-written strptime regex translator and the ISO clock parser
(reviewer item 1), plus timezone_name(None) -> AttributeError (person-granted change)."""
import datetime as _dt
from hypothesis import given, example, strategies as st
from portwright.compare import same
from portwright.compare.strategies import text

# ---- strptime translator: duplicate group names, case folding of i/İ/ı/K/ſ, Unicode \d and \s,
# regex metacharacters in literals.
TRICKY_LIT = st.sampled_from(["i", "I", "İ", "ı", "k", "K", "K", "s", "ſ", "ß", "SS", "x", ".", "(", ")",
                              "[", "]", "{2}", "^", "$", "|", "\\", "?", "*", "+", " ", "  ", "\t",
                              "　", "\xa0", "\x1c", " ", "-", ":", "T", "é", "É"])
DIRECTIVES = st.sampled_from(["%Y", "%m", "%d", "%H", "%M", "%S", "%f", "%y", "%j", "%b", "%B", "%a",
                              "%A", "%p", "%I", "%z", "%Z", "%U", "%W", "%w", "%u", "%G", "%V", "%%"])


@st.composite
def fmt_and_text(draw):
    parts = draw(st.lists(st.one_of(DIRECTIVES, TRICKY_LIT), min_size=1, max_size=6))
    fmt = "".join(parts)
    dt = draw(st.datetimes(min_value=_dt.datetime(1000, 1, 1), max_value=_dt.datetime(9999, 12, 31),
                           timezones=st.just(_dt.timezone.utc)))
    try:
        s = dt.strftime(fmt)
    except Exception:
        s = fmt
    mode = draw(st.integers(0, 4))
    if mode == 1:
        s = s.upper()
    elif mode == 2:
        s = s.lower()
    elif mode == 3:
        # swap ASCII digits for other Unicode decimal digits (\d is Unicode in CPython re)
        base = draw(st.sampled_from([0x660, 0x6F0, 0x966, 0xFF10, 0x1D7CE]))
        s = "".join(chr(base + ord(c) - 48) if c.isascii() and c.isdigit() else c for c in s)
    elif mode == 4:
        s = s.replace(" ", draw(st.sampled_from(["　", "\xa0", "\x1c", "\x85", "  \t", " "])))
    return s, fmt


@given(fmt_and_text())
@example(("1 2", "%d %d"))
@example(("2024 2024", "%Y %Y"))
@example(("2024 01 02", "%Y %m %Y"))
@example(("1 2 3", "%d %m %d"))
@example(("İ", "i"))
@example(("ı", "I"))
@example(("i", "İ"))
@example(("I", "ı"))
@example(("K", "k"))
@example(("ſ", "s"))
@example(("tİme", "time"))
@example(("2024　2", "%Y %d"))
@example(("2024\xa02", "%Y %d"))
@example(("2024\x1c2", "%Y %d"))
@example(("２０２４", "%Y"))
@example(("١٢", "%d"))
@example(("x", "(?P<d>x)"))
@example(("a{2}", "a{2}"))
@example(("aa", "a{2}"))
@example(("a^b$", "a^b$"))
@example(("ab", "a|b"))
@example(("THURSDAY", "%A"))
@example(("FEBRUARY İ", "%B i"))
def test_strptime_translator(case):
    same("strptime", *case)


@given(text(max_size=8), st.lists(st.one_of(DIRECTIVES, TRICKY_LIT, text(max_size=2)), max_size=4))
def test_strptime_translator_random(s, parts):
    same("strptime", s, "".join(parts))


# ---- ISO clock parser: ',' and '.' fractions, fraction lengths, truncation, basic/extended forms.
@st.composite
def iso_clock(draw):
    ext = draw(st.booleans())
    sep = ":" if ext else ""
    h = "%02d" % draw(st.integers(0, 25))
    parts = [h]
    n = draw(st.integers(0, 2))
    for _ in range(n):
        parts.append("%02d" % draw(st.integers(0, 61)))
    s = sep.join(parts)
    if draw(st.booleans()):
        s += draw(st.sampled_from([".", ","])) + "".join(draw(st.lists(st.sampled_from("0123456789"), max_size=10)))
    tz = draw(st.sampled_from(["", "Z", "+05:30", "-0800", "+05:30:15", "+05:30:15.5", "+053015.123456",
                               "-0", "+", "+05", "z"]))
    s += tz
    cut = draw(st.integers(0, 3))
    if cut and draw(st.booleans()):
        s = s[:-cut]
    return s


@given(iso_clock(), st.sampled_from(["T", " ", "t", "x"]), st.sampled_from(["2024-02-29", "20240229", "2024-W09-4", "2024-060"]))
@example("12:30:45,123", "T", "2024-02-29")
@example("12:30:45.1", "T", "2024-02-29")
@example("12:30:45,", "T", "2024-02-29")
@example("12:30:45.", "T", "2024-02-29")
@example("12:3", "T", "2024-02-29")
@example("12:30:4", "T", "2024-02-29")
@example("1", "T", "2024-02-29")
@example("12:30:45.1234567", "T", "2024-02-29")
@example("12:30:45,123456789", "T", "2024-02-29")
@example("123045,5", "T", "2024-02-29")
@example("12:30:45,5Z", "T", "2024-02-29")
@example("12:30:45.123-0", "T", "2024-02-29")
@example("12:30:45.1234+05:30:15.5", "T", "2024-02-29")
def test_iso_clock(clock, sep, date):
    same("datetime_fromisoformat", date + sep + clock)
    same("time_fromisoformat", clock)
    same("time_fromisoformat", "T" + clock)


# ---- person-granted change: timezone_name(None) raises AttributeError
@given(st.none())
@example(None)
def test_timezone_name_none(off):
    same("timezone_name", off)
