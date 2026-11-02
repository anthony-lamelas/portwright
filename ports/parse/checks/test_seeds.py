import json
import os

import pytest
from hypothesis import given, strategies as st

from pw_helpers import all_entry_points

with open(os.path.join(os.path.dirname(__file__), "seeds.json")) as fh:
    SEEDS = [tuple(x) for x in json.load(fh)]

# Hand-picked traps on top of the upstream examples.
TRAPS = [
    ("{}", "abc\n"), ("{}\n", "abc\n"), ("{}", "a\nb"), ("{} {}", "a  b"),
    ("a{2,3}", "aaa"), ("a{2,3}", "a{2,3}"), ("{:}", "{:}"), ("x{", "x{"), ("}x", "}x"),
    ("{{}}", "{}"), ("{{{}}}", "{a}"), ("{a}{a}", "xx"), ("{a} {a}", "Ab aB"),
    ("{a} {a}", "Ab aB"), ("{a[b]}{a[c]}", "xy"), ("{a}{a[b]}", "xy"), ("{a[b]}-{a}", "x-y"),
    ("{a[b][c]}/{a[b][d]}", "1/2"), ("{a.b}{a_b}", "xy"), ("{a_b_}{a[b]}", "xy"),
    ("{a-b} {a_b}", "x y"), ("{0}{1}", "ab"), ("{_x} {y}", "a b"), ("{.a}", "z"),
    ("{café} {naïve:d}", "x 12"), ("{:d}", "١٢"), ("{:n}", "١٢"), ("{:f}", "١.٥"),
    ("{:g}", "1e5"), ("{:g}", "-inf"), ("{:e}", "+INF"), ("{:e}", "1.5E-3"), ("{:f}", "-.5"),
    ("{:.0f}", "20"), ("{:.0f}", "-20"), ("{:F}", "1.10"), ("{:F}", "nan"), ("{:f}", "NaN"),
    ("{:%}", "50%"), ("{:%}", "12.5%"), ("{:%}", "-3%"), ("{:n}", "1,234,567"),
    ("{:n}", "1.234.567"), ("{:n}", "12,34"), ("{:b}", "0b1010"), ("{:o}", "0o17"),
    ("{:x}", "0xDEAD"), ("{:X}", "beef"), ("{:d}", "-0x1F"), ("{:d}", "+0b11"),
    ("{:d}", " 7"), ("{:d}", "0o0"), ("{:d}", "00"), ("{:d}", "0x"), ("{:,d}", "1,000"),
    ("{:_d}", "1_000"), ("{:3d}", "12345"), ("{:3d}", "  1"), ("{:>5d}", "   42"),
    ("{:=5d}", "-0042"), ("{:x=8d}", "xxxx42"), ("{:*^9d}", "***42***"), ("{:0>4}", "00ab"),
    ("{:|<4}", "ab||"), ("{:.<5}", "ab..."), ("{:\\<3}", "a\\\\"), ("{:{<3}", "a{{"),
    ("{:w}", "abc_1"), ("{:W}", "!!"), ("{:s}", " \t"), ("{:S}", "xyz"), ("{:D}", "abc"),
    ("{:l}", "Straße"), ("{:5}", "ab"), ("{:5}", "abcdefg"), ("{:.3}", "abcd"),
    ("{:2.3}", "abc"), ("{:<10}", "hi        "), ("{:^}", "  hi  "),
    ("{:ti}", "2020-02-30"), ("{:ti}", "2020-01-01T25:00"), ("{:ti}", "2020-01-01 10:00:00.5+05:30"),
    ("{:ti}", "2020-01-01T10:00:00-0800"), ("{:ti}", "2020-01-01T10:00+0560"),
    ("{:tg}", "1/Feb/2020 1:02 pm"), ("{:tg}", "31/12/1999 23:59:59.999999"),
    ("{:ta}", "February/3/2020 12:00 AM +0100"), ("{:ta}", "Jan-3-2020"),
    ("{:te}", "Tue, 3 Mar 2020 10:00:00 -0000"), ("{:th}", "03/mar/2020:10:00:00 +0000"),
    ("{:tc}", "Wed Mar  4 10:00:00 2020"), ("{:tt}", "12:00 AM"), ("{:tt}", "12:30 PM"),
    ("{:tt}", "1:2:3.4567890"), ("{:tt}", "10:00 +0130"), ("{:ts}", "Mar 3 10:00:00"),
    ("{:ts}", "Feb 29 "), ("{:%H:%M}", "10:30"), ("{:%Y}", "2020"), ("{:%y-%m}", "99-12"),
    ("{:%z}", "+0530"), ("{:%z}", "z"), ("{:%I %p}", "11 PM"), ("{:%a %d}", "Mon 05"),
    ("{:%A %B %d %Y}", "Monday March 02 2020"), ("{:%U %W %w}", "1 2 3"), ("{:%Y(x)}", "2020x"),
    ("{:%Y(}", "2020("), ("{:%j %Y}", "400 2020"), ("{:%S}", "61"),
    ("{:q}", "x"), ("{:dd}", "1"), ("{:tq}", "x"), ("{a:d}{a:w}", "1a"),
    ("{:>}", "x"), ("{: d}", " 1"), ("{:+d}", "+1"), ("{:-d}", "-1"), ("{:010.3f}", "000001.500"),
    ("{:,.2f}", "1.50"), ("{:_x}", "ff"), ("{:#x}", "0xff"), ("", ""), ("", "x"),
    ("ΣΑΣ {}", "σας x"), ("K{}", "K1"), ("{} ß", "x SS"), ("{} ß", "x ẞ"),
    ("({})[{}]*+?^$|.", "(a)[b]*+?^$|."), ("a.c{}", "abcx"), ("\\d{}", "\\d1"),
]
CASES = SEEDS + TRAPS


@pytest.mark.parametrize("fmt,string", CASES)
@pytest.mark.parametrize("case_sensitive", [None, True])
def test_seed(fmt, string, case_sensitive):
    all_entry_points(fmt, string, case_sensitive=case_sensitive)


@given(st.sampled_from(CASES), st.data())
def test_seed_mutated(case, data):
    fmt, string = case
    op = data.draw(st.sampled_from(["prefix", "suffix", "delete", "insert", "upper", "double"]))
    if op == "prefix":
        string = data.draw(st.text(max_size=3)) + string
    elif op == "suffix":
        string = string + data.draw(st.text(max_size=3))
    elif op == "delete" and string:
        i = data.draw(st.integers(0, len(string) - 1))
        string = string[:i] + string[i + 1:]
    elif op == "insert":
        i = data.draw(st.integers(0, len(string)))
        string = string[:i] + data.draw(st.sampled_from(list("0 -.:,+aZ\n9/%"))) + string[i:]
    elif op == "upper":
        string = string.upper()
    else:
        string = string + " " + string
        fmt = fmt  # findall/search over repeated content
    all_entry_points(fmt, string, case_sensitive=data.draw(st.sampled_from([None, False, True])))
