"""format(), str.format() and the % operator, compared with the real built-ins.

Run against the shim these pass trivially. Run against a port, each case checks that
the port gives the same value, or raises the same exception with the same message.
"""

import itertools

import pytest

import pc_format as pc

INF = float("inf")
NAN = float("nan")


def outcome(func, *args):
    try:
        value = func(*args)
    except Exception as exc:  # the exception is the behavior being compared
        return ("raised", type(exc).__name__, str(exc))
    return ("returned", type(value).__name__, repr(value))


def same(port_func, builtin, *args):
    assert outcome(port_func, *args) == outcome(builtin, *args), args


INTS = [0, 1, -1, 7, -7, 42, 255, -255, 1000, 1234567, -1234567, 10**15, 2**53 + 1, 2**64, -(2**70), 10**40]
FLOATS = [0.0, -0.0, 0.5, 1.5, 2.5, -2.5, 0.125, 0.1, 1 / 3, 2 / 3, 123456.789, -123456.789, 1e16, 1e15, 9.999999e15,
          1e-4, 1e-5, 1e-7, 1.7976931348623157e308, 5e-324, 2.2250738585072014e-308, 1e22, 1e23, INF, -INF, NAN,
          0.000123456, 99.995, 1.005, 2.675, 0.045, 1e100, 12345678901234567890.0]
STRS = ["", "a", "abc", "héllo", "日本語", "x" * 20, " pad ", "tab\there"]


@pytest.mark.parametrize("spec", ["", "d", "b", "o", "x", "X", "#b", "#o", "#x", "#X", "c", "n", "e", "f", "g", "%",
                                  "5d", "-5d", "+d", " d", "05d", "010x", "#010x", "<6", ">6", "^7", "=+8", "*^9",
                                  ",", "_", ",d", "_x", "_b", "_o", "12,", "+,", ".2f", "10.3e", "#g", ".0f", "s"])
def test_format_ints(spec):
    for value in INTS:
        same(pc.format_value, format, value, spec)


@pytest.mark.parametrize("spec", ["", "e", "E", "f", "F", "g", "G", "%", "n", ".0f", ".1f", ".2f", ".3f", ".10f", ".17g",
                                  ".0e", ".3e", ".20e", ".0g", ".1g", ".3g", "#.0f", "#.0e", "#g", "#.3g", "+f", " f",
                                  "-g", "z", "z.1f", "zf", "+z.2f", "08.2f", "010.3e", "=+12.4f", "*^14.2%", "<10", ">10",
                                  "^10.1", ",", ",.2f", "_.3f", ",g", ".3", ".0", "12", ".1%", "d", "x", "s", ".100f",
                                  ".60e"])
def test_format_floats(spec):
    for value in FLOATS:
        same(pc.format_value, format, value, spec)


@pytest.mark.parametrize("spec", ["", "s", "5", "<5", ">5", "^5", "^6", "*>8", "0<5", ".2", ".0", "10.3", "^9.4s",
                                  "=5", "+s", "d", ",", "#", "z", "x"])
def test_format_strings(spec):
    for value in STRS:
        same(pc.format_value, format, value, spec)


@pytest.mark.parametrize("spec", ["", "d", "x", "s", "5", ">5", "f", ".1f", "^6", "n", "%", "e", "c", "b", "#x"])
def test_format_bools(spec):
    for value in (True, False):
        same(pc.format_value, format, value, spec)


def test_format_fill_and_width_combinations():
    for fill, align, width in itertools.product(["", "x", "0", " ", "{", "日"], ["", "<", ">", "^", "="], ["", "0", "1", "9", "15"]):
        spec = (fill + align if align else "") + width
        for value in (5, -5, 3.25, -3.25, "ab"):
            same(pc.format_value, format, value, spec)


def test_format_grouping_with_every_type():
    for group, kind in itertools.product([",", "_"], ["", "d", "b", "o", "x", "X", "e", "f", "g", "%", "n", "c", "s"]):
        for value in (0, 7, 1234567, -98765432109876, 1234567.891, -0.001):
            same(pc.format_value, format, value, f"{group}{kind}")
            same(pc.format_value, format, value, f"015{group}.2{kind}")


def test_format_spec_errors():
    # Widths past what Python accepts raise before allocating anything.
    bad = ["~", "..", ".f.", "1.2.3", "<<<", ",,", ",_", "_,", "99999999999999999999", ".99999999999999999999d",
           "y", "dd", "#s", "+c", ",c", "z", "zd", "=s", "0s", "{", "}"]
    for spec, value in itertools.product(bad, [1, 1.5, "s", True]):
        same(pc.format_value, format, value, spec)


def test_format_char_type_range():
    for value in (0, 65, 0x10FFFF, 0x110000, -1, 2**40):
        same(pc.format_value, format, value, "c")
        same(pc.format_value, format, value, ">4c")


@pytest.mark.parametrize("template", ["{}", "{0}", "{0}{1}", "{1}{0}", "{a}", "{a}{b}", "{}{}", "{{}}", "{{{0}}}",
                                      "x{0:>5}y", "{0!r}", "{0!s}", "{0!a}", "{0:{1}}", "{:{}}", "{0:{1}.{2}f}",
                                      "{0[0]}", "{0[1]}", "{a[b]}", "{0.real}", "{0.imag}", "{0.real:5}", "{!r:>8}",
                                      "{0:%}", "{a!r:^10}", "{0}{0}{0}"])
def test_str_format_fields(template):
    args_options = [(1, 2, 3), (1.5, 6, 2), ("ab", "cd", "ef"), ([10, 20], {"k": 1}, 0), ("é", 4, 1)]
    kwargs_options = [{}, {"a": 5, "b": "x"}, {"a": {"b": 7}, "b": 2.5}, {"a": "日本"}]
    for args, kwargs in itertools.product(args_options, kwargs_options):
        same(pc.str_format, lambda t, a, k: t.format(*a, **k), template, args, kwargs)


def test_str_format_errors():
    templates = ["{", "}", "{0", "0}", "{}{0}", "{0}{}", "{5}", "{x}", "{0!}", "{0!z}", "{0!r!s}", "{0:{1:{2}}}",
                 "{0[}", "{0.}", "{0..real}", "{0[0]x}", "{:d}", "{:s}", "{0.nope}", "{0[9]}", "{a[zz]}", "{ 0}",
                 "{-1}", "{0:{}}", "{0}{1}", "{!r}{!s}{!a}"]
    for template in templates:
        for args, kwargs in [((1,), {}), (("s", 2), {"a": {"b": 1}}), (([1],), {"a": {}})]:
            same(pc.str_format, lambda t, a, k: t.format(*a, **k), template, args, kwargs)


@pytest.mark.parametrize("template", ["%s", "%r", "%a", "%d", "%i", "%u", "%x", "%X", "%o", "%e", "%E", "%f", "%F",
                                      "%g", "%G", "%c", "%%", "%5d", "%-5d", "%05d", "%+d", "% d", "%#x", "%#o", "%#X",
                                      "%.2f", "%10.3f", "%-10.3e", "%+.0f", "%#.0f", "%#g", "%.3g", "%.0e", "%5s",
                                      "%-5s", "%.2s", "%5.1s", "%*d", "%.*f", "%*.*f", "%ld", "%hd", "%Lf", "x%sy%%"])
def test_percent_single_values(template):
    for value in INTS[:10] + FLOATS[:16] + STRS[:5] + [True, None, 3, (4,), (5, 6), (7, 8, 9), "s", 65]:
        same(pc.percent_format, lambda t, v: t % v, template, value)


def test_percent_mappings():
    templates = ["%(a)s", "%(a)d %(b)s", "%(a)5.2f", "%(a)r", "%(missing)s", "%(a)s %s", "%(a", "%()s", "%(a)*d",
                 "%(b)c", "%(a)%", "%%(a)s"]
    for template in templates:
        for mapping in [{"a": 1, "b": "x"}, {"a": 2.5, "b": 66}, {"a": "s", "b": "c"}, {"": 3, "a": 0}]:
            same(pc.percent_format, lambda t, v: t % v, template, mapping)


def test_percent_errors():
    templates = ["%", "%z", "%d %d", "%s", "%c", "%(a)s", "% ", "%5", "%.", "%*", "%.*", "%d", "%x", "%e", "abc",
                 "%s %s %s", "%-", "%#"]
    for template in templates:
        for values in [(), (1,), (1, 2), ("a",), (1.5,), "str", 3, {"a": 1}, (2**70,), (0x110000,), ([],), (None,)]:
            same(pc.percent_format, lambda t, v: t % v, template, values)


def test_percent_with_infinities_and_huge_values():
    for template in ["%f", "%e", "%g", "%d", "%x", "%.3f", "%10.2e", "%+g"]:
        for value in (INF, -INF, NAN, 1e308, -1e-308, 2**100, -(2**100), 10**25):
            same(pc.percent_format, lambda t, v: t % v, template, value)
