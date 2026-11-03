"""String methods and unicodedata, compared with the real built-ins over the whole table."""

import itertools
import unicodedata

import pytest

import pc_unicode as pc

# Every BMP code point, including lone surrogates, and a sample of the rest.
CODE_POINTS = list(range(0x10000)) + list(range(0x10000, 0x110000, 13)) + [0x10FFFF, 0x1F600, 0x2FFFF, 0xE0001]
CHARS = [chr(cp) for cp in CODE_POINTS]
# Characters added or changed after Unicode 15.0, which a newer table would get wrong.
NEWER = [chr(cp) for cp in (0x2FFC, 0x2FFD, 0x31EF, 0x1B132, 0x1B155, 0x1D2C0, 0x1E030, 0x1F6DC, 0x1FA89, 0x2EBF0, 0x31350)]
STRINGS = ["", "a", "Hello World", "hELLO wORLD", "ß", "ŉ", "ǅ", "ǆ", "Ǆ", "ﬁ", "ΣΑΣ", "ὈΔΥΣΣΕΎΣ", "İstanbul", "ı",
           "they're bill's friends", "1st 2nd", "ǰ̌", "aͅ", "x\ud800y", "\udfff", "Ａｂｃ", "ⅷ", "ᾳ", "ﬀ", "ΐ",
           "Ꭰ", "ꭰ", "ꮿ", "ẞ", "ŉ ŉ", "a1b2", "A_B", "ǈ x", "ᵃ", "ⓐ", "𝔄", "İ̇", "ẋ́"]


def outcome(func, *args):
    try:
        value = func(*args)
    except Exception as exc:
        return ("raised", type(exc).__name__, str(exc))
    return ("returned", type(value).__name__, repr(value))


def check_all(port, builtin, inputs):
    mismatches = [args for args in inputs if outcome(port, *args) != outcome(builtin, *args)]
    assert not mismatches, mismatches[:10]


@pytest.mark.parametrize("name", ["upper", "lower", "casefold", "title", "capitalize", "swapcase"])
def test_case_mapping_of_every_character(name):
    check_all(getattr(pc, name), getattr(str, name), [(c,) for c in CHARS + NEWER])


@pytest.mark.parametrize("name", ["upper", "lower", "casefold", "title", "capitalize", "swapcase"])
def test_case_mapping_of_strings(name):
    words = STRINGS + [a + b for a, b in itertools.product(STRINGS[:12], STRINGS[:12])]
    check_all(getattr(pc, name), getattr(str, name), [(s,) for s in words])


@pytest.mark.parametrize("name", ["isalpha", "isdigit", "isdecimal", "isnumeric", "isalnum", "isspace", "isupper",
                                  "islower", "istitle", "isidentifier", "isprintable", "isascii"])
def test_predicates_of_every_character(name):
    check_all(getattr(pc, name), getattr(str, name), [(c,) for c in CHARS + NEWER])


@pytest.mark.parametrize("name", ["isalpha", "isdigit", "isupper", "islower", "istitle", "isidentifier", "isprintable", "isspace"])
def test_predicates_of_strings(name):
    check_all(getattr(pc, name), getattr(str, name), [(s,) for s in STRINGS + ["", " ", "_a1", "1a", "a b", "Ab Cd", "AB cd"]])


WHITESPACE = "".join(c for c in CHARS if c.isspace()) + "\x1c\x1d\x1e\x1f\x85​﻿"


def test_split_and_rsplit():
    texts = ["", " ", "a b", "  a  b  ", "a,b,,c", ",", "a" + WHITESPACE + "b", "　x y\x85z", "a\x00b",
             "x\ud800 y", "one two three four"]
    seps = [None, ",", " ", ",,", "ab", "　"]
    for text, sep, maxsplit in itertools.product(texts, seps, [-1, 0, 1, 2, 100]):
        for name in ("split", "rsplit"):
            assert outcome(getattr(pc, name), text, sep, maxsplit) == outcome(getattr(str, name), text, sep, maxsplit), (name, text, sep, maxsplit)
    for text in texts:
        assert outcome(pc.split, text, "") == outcome(str.split, text, "")


def test_splitlines():
    texts = ["", "a", "a\nb", "a\r\nb", "a\rb", "a\n", "\n\n", "a\x0bb\x0cc\x1cd\x1de\x1ef\x85g h i", "\r\n\r", "x\x1fy"]
    for text, keepends in itertools.product(texts, [False, True]):
        assert outcome(pc.splitlines, text, keepends) == outcome(str.splitlines, text, keepends)


def test_strip_family():
    texts = ["", "  a  ", WHITESPACE + "a" + WHITESPACE, "xxaxx", "abcba", "​a​", "﻿a"]
    for text, chars in itertools.product(texts, [None, "", "x", "ab", " x", "​"]):
        for name in ("strip", "lstrip", "rstrip"):
            assert outcome(getattr(pc, name), text, chars) == outcome(getattr(str, name), text, chars), (name, text, chars)


def test_padding():
    for text, width, fill in itertools.product(["", "a", "ab", "abc", "日本", "-5", "+5", "x\ud800"], [-1, 0, 1, 2, 5, 6, 9], [" ", "*", "日"]):
        for name in ("center", "ljust", "rjust"):
            assert outcome(getattr(pc, name), text, width, fill) == outcome(getattr(str, name), text, width, fill)
        assert outcome(pc.zfill, text, width) == outcome(str.zfill, text, width)
    for bad in ["", "ab"]:
        assert outcome(pc.center, "a", 5, bad) == outcome(str.center, "a", 5, bad)


def test_expandtabs():
    for text, size in itertools.product(["", "\t", "a\tb", "ab\t\tc", "a\nb\tc", "a\rb\tc", "日\tx", "\t\t\t"], [-1, 0, 1, 2, 4, 8, 9]):
        assert outcome(pc.expandtabs, text, size) == outcome(str.expandtabs, text, size)


@pytest.mark.parametrize("form", ["NFC", "NFD", "NFKC", "NFKD"])
def test_normalize(form):
    samples = STRINGS + ["".join(CHARS[i:i + 6]) for i in range(0xA0, 0x3000, 37)] + [
        "ẛ̣", "각", "ཱི", "ﷺ", "⿀", "̈́", "á̖", "각"]
    check_all(lambda s: pc.normalize(form, s), lambda s: unicodedata.normalize(form, s), [(s,) for s in samples])
    check_all(lambda s: pc.is_normalized(form, s), lambda s: unicodedata.is_normalized(form, s), [(s,) for s in samples])


def test_normalize_rejects_bad_forms():
    for form in ["nfc", "NFX", ""]:
        assert outcome(pc.normalize, form, "a") == outcome(unicodedata.normalize, form, "a")


@pytest.mark.parametrize("name", ["category", "bidirectional", "combining", "east_asian_width", "mirrored", "decomposition"])
def test_properties_of_every_character(name):
    check_all(getattr(pc, name), getattr(unicodedata, name), [(c,) for c in CHARS + NEWER])


def test_names_and_lookup():
    check_all(lambda c: pc.char_name(c, "?"), lambda c: unicodedata.name(c, "?"), [(c,) for c in CHARS + NEWER])
    for c in ["a", "\x00", "가", "\U00020000", "\ud800", ""]:
        assert outcome(pc.char_name, c) == outcome(unicodedata.name, c)
    names = ["LATIN SMALL LETTER A", "latin small letter a", "HANGUL SYLLABLE GA", "CJK UNIFIED IDEOGRAPH-4E00",
             "CJK UNIFIED IDEOGRAPH-2EBF0", "EM DASH", "NOT A NAME", "", "LINE FEED", "BYTE ORDER MARK", "SPACE ",
             "hangul syllable gag", "TANGUT IDEOGRAPH-17000", "KHITAN SMALL SCRIPT CHARACTER-18B00"]
    for name in names:
        assert outcome(pc.lookup, name) == outcome(unicodedata.lookup, name), name


@pytest.mark.parametrize("name", ["numeric_value", "decimal_value", "digit_value"])
def test_numeric_values(name):
    builtin = getattr(unicodedata, name.split("_")[0])
    check_all(lambda c: getattr(pc, name)(c, -1.5), lambda c: builtin(c, -1.5), [(c,) for c in CHARS + NEWER])
    for c in ["5", "a", "½", "Ⅻ", "٣"]:
        assert outcome(getattr(pc, name), c) == outcome(builtin, c)


def test_unicode_version():
    assert pc.unidata_version() == unicodedata.unidata_version


def test_single_character_functions_reject_strings():
    for name in ["category", "combining", "char_name", "east_asian_width", "mirrored", "numeric_value"]:
        builtin = getattr(unicodedata, {"char_name": "name", "numeric_value": "numeric"}.get(name, name))
        for arg in ["", "ab"]:
            assert outcome(getattr(pc, name), arg) == outcome(builtin, arg), (name, arg)
