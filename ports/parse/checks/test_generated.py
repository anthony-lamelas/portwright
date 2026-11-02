"""Build valid format/string pairs from field specs, then near-misses."""
import datetime as dt

from hypothesis import given, strategies as st

from pw_helpers import all_entry_points

LIT_CHARS = list("ab XYZ-_:/.,;()[]*+?^$|\\!#@é\n\tß") + ["{{", "}}"]
literal = st.lists(st.sampled_from(LIT_CHARS), max_size=4).map("".join)

ints = st.one_of(st.integers(-10**6, 10**6), st.integers(-2**70, 2**70), st.sampled_from([0, -1, 1]))
floats = st.one_of(st.floats(allow_nan=True, allow_infinity=True, width=64),
                   st.floats(-1e6, 1e6), st.sampled_from([0.0, -0.0, 0.5, 1e-7]))
dts = st.datetimes(min_value=dt.datetime(1000, 1, 1), max_value=dt.datetime(9999, 12, 31))

align = st.sampled_from(["", "<", ">", "^", "=", "x<", "*>", "0=", "|^", ".<"])
sign = st.sampled_from(["", "+", "-", " "])
width = st.sampled_from(["", "1", "3", "8", "12"])


def num_spec(draw, type_, allow_group=True, allow_eq=True):
    a = draw(align)
    if not allow_eq and a.endswith("="):
        a = ""
    s = draw(sign)
    z = draw(st.sampled_from(["", "0"])) if not a else ""
    w = draw(width)
    g = draw(st.sampled_from(["", ",", "_"])) if allow_group else ""
    return a + s + z + w + g + type_


@st.composite
def field(draw):
    kind = draw(st.sampled_from(
        ["str", "d", "n", "box", "f", "e", "g", "%", "cls", "date", "strf", "plainwidth"]))
    if kind == "str":
        return "", draw(st.text(alphabet="abcXYZ 123é-", min_size=1, max_size=6))
    if kind == "plainwidth":
        spec = draw(st.sampled_from(["5", ".3", "2.4", "<4", ">6", "^7", "*^7", "<", ">", "^"]))
        v = draw(st.text(alphabet="abc xyz", min_size=1, max_size=5))
        try:
            return spec, format(v, spec)
        except ValueError:
            return spec, v
    if kind == "d":
        spec = num_spec(draw, "d")
        v = draw(ints)
        try:
            return spec, format(v, spec)
        except ValueError:
            return spec, str(v)
    if kind == "n":
        v = draw(st.integers(0, 10**12))
        return draw(st.sampled_from(["n", ">15n", "=n"])), format(v, ",").replace(",", draw(st.sampled_from([",", "."])))
    if kind == "box":
        t = draw(st.sampled_from("boxX"))
        v = draw(st.integers(0, 2**80))
        body = format(v, t)
        pre = draw(st.sampled_from(["", "0" + t, "0" + t.upper(), "-", "+"]))
        return num_spec(draw, t, allow_group=False), pre + body
    if kind == "f":
        t = draw(st.sampled_from("fF"))
        prec = draw(st.sampled_from(["", ".0", ".1", ".3", ".7"]))
        v = draw(floats)
        spec = num_spec(draw, "", allow_group=False) + prec + t
        try:
            return spec, format(v, spec)
        except ValueError:
            return spec, repr(v)
    if kind == "e":
        t = draw(st.sampled_from("eE"))
        v = draw(floats)
        return t, format(v, draw(st.sampled_from(["e", ".2e", "E", ".0E"])))
    if kind == "g":
        t = draw(st.sampled_from("gG"))
        v = draw(floats)
        return t, format(v, draw(st.sampled_from(["g", ".3g", "G", "", "f"])))
    if kind == "%":
        v = draw(st.floats(-100, 100))
        return "%", format(v, draw(st.sampled_from([".0%", ".2%", "%"])))
    if kind == "cls":
        t, alpha = draw(st.sampled_from([
            ("w", "ab_1é"), ("W", "!@ -"), ("s", " \t\n"), ("S", "ab!1"), ("D", "ab -"), ("l", "aBz")]))
        return t, draw(st.text(alphabet=alpha, min_size=1, max_size=5))
    if kind == "date":
        d = draw(dts)
        t, fmt = draw(st.sampled_from([
            ("ti", "%Y-%m-%d"), ("ti", "%Y-%m-%dT%H:%M"), ("ti", "%Y-%m-%d %H:%M:%S.%f"),
            ("ti", "%Y-%m-%dT%H:%M:%SZ"), ("ti", "%Y-%m-%dT%H:%M:%S+05:30"), ("ti", "%Y-%m-%d -0800"),
            ("tg", "%d/%m/%Y"), ("tg", "%d-%b-%Y %H:%M"), ("tg", "%d/%B/%Y %I:%M %p +1:00"),
            ("tg", "%d/%m/%Y %I:%M:%S PM -0530"),
            ("ta", "%m/%d/%Y"), ("ta", "%b/%d/%Y %I:%M %p"), ("ta", "%m-%d-%Y %H:%M:%S +1030"),
            ("te", "%a, %d %b %Y %H:%M:%S +1000"), ("te", "%d %b %Y %H:%M -0000"),
            ("th", "%d/%b/%Y:%H:%M:%S +0000"), ("tc", "%a %b %d %H:%M:%S %Y"),
            ("tt", "%H:%M"), ("tt", "%I:%M %p"), ("tt", "%H:%M:%S.%f +0100"), ("tt", "%I:%M:%S AM -0:30"),
            ("ts", "%b %d %H:%M:%S"), ("ts", "%b %d "),
        ]))
        s = d.strftime(fmt)
        if draw(st.booleans()):
            s = s.lower()
        return t, s
    # strftime directives
    d = draw(dts)
    parts = draw(st.lists(st.sampled_from(
        ["%Y", "%y", "%m", "%d", "%H", "%M", "%S", "%f", "%b", "%B", "%a", "%A", "%j", "%I", "%p",
         "%w", "%U", "%W", "%z", "-", "/", " ", ":", "T", "."]), min_size=1, max_size=6))
    fmt = "".join(parts)
    if "%" not in fmt:
        fmt = "%Y" + fmt
    s = d.strftime(fmt.replace("%z", "+0100"))
    return fmt, s


NAMES = st.one_of(st.none(), st.sampled_from(
    ["a", "b", "name", "a.b", "a_b", "x[y]", "x[z]", "q-r", "Ü", "a1", "_u", "9", "0"]))


@st.composite
def fmt_and_string(draw):
    n = draw(st.integers(0, 4))
    fmt = [draw(literal)]
    s = [fmt[0].replace("{{", "{").replace("}}", "}")]
    used = {}
    for _ in range(n):
        spec, text = draw(field())
        name = draw(NAMES)
        if name and name in used and draw(st.booleans()):
            # repeated name, same spec -> backreference (maybe differing text)
            spec, text = used[name][0], draw(st.sampled_from([used[name][1], text]))
        if name:
            used.setdefault(name, (spec, text))
        fmt.append("{%s%s}" % (name or "", ":" + spec if spec else ""))
        s.append(text)
        lit = draw(literal)
        fmt.append(lit)
        s.append(lit.replace("{{", "{").replace("}}", "}"))
    return "".join(fmt), "".join(s)


@given(fmt_and_string(), st.sampled_from([None, False, True]))
def test_generated(pair, cs):
    fmt, s = pair
    all_entry_points(fmt, s, case_sensitive=cs)


@given(fmt_and_string(), st.data())
def test_generated_near_miss(pair, data):
    fmt, s = pair
    if s:
        i = data.draw(st.integers(0, len(s) - 1))
        repl = data.draw(st.sampled_from(["", "x", "9", " ", "-", ".", "Z", "\n"]))
        s = s[:i] + repl + s[i + 1:]
    all_entry_points(fmt, s)


@given(fmt_and_string(), fmt_and_string(), st.text(alphabet="ab \n9", max_size=3))
def test_embedded(p1, p2, junk):
    # format occurs inside a longer string, perhaps more than once
    fmt, s = p1
    hay = p2[1] + junk + s + junk + s
    all_entry_points(fmt, hay)
