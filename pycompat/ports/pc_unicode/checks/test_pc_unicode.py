import sys
from hypothesis import given, settings, strategies as st, HealthCheck, example
from portwright.compare import same, PythonExpression
from portwright.compare.strategies import text
from _strats import char, any_text, rich_text, SPECIAL, BAD_ARG, NON_BUILTIN, NON_STR_BUILTIN

CASE = ["upper", "lower", "casefold", "title", "capitalize", "swapcase"]
PRED = ["isalpha", "isdigit", "isdecimal", "isnumeric", "isalnum", "isspace", "isupper", "islower",
        "istitle", "isidentifier", "isprintable", "isascii"]
CHARFN = ["category", "bidirectional", "combining", "east_asian_width", "mirrored", "decomposition"]
NUMFN = ["numeric_value", "decimal_value", "digit_value"]
FORMS = ["NFC", "NFD", "NFKC", "NFKD"]

words = st.one_of(
    any_text(16),
    st.lists(st.one_of(st.sampled_from(["ab", "Ab", "AB", "ǅx", "ß", "ŉ", "Σ", "ΣΑΣ", "ὈΔΥΣΣΕΎΣ", "İ", "ﬁ",
                                        "x'y", "1st", "_a1", "a b", " ", "\t", " ", "Ａｂ", "ᾳ", "ǰ",
                                        "\ud800", "aͅ", "ꭰ", "\U00010428", "ᵃ", "Ⓐ", "Σ.", ".Σ", "aΣ b", "AΣ"]),
                       char()), max_size=8).map("".join),
)


# ---------- case mapping / predicates ----------
@given(st.sampled_from(CASE), words)
def test_case(name, s):
    same(name, s)


@given(st.sampled_from(CASE + PRED), char())
def test_case_and_pred_single_char(name, c):
    same(name, c)


@given(st.sampled_from(PRED), words)
def test_predicates(name, s):
    same(name, s)


@given(st.sampled_from(PRED), st.sampled_from(["", " ", "_", "a", "A", "1", "_a1", "1a", "a b", "Ab Cd", "AB cd",
                                                 "ǅ", "ǅa", "ǅA", "Aǅ", "à", "̀", "Ab1", "A1b", "١٢",
                                                 "²", "½", "Ⅻ", "〇", "℘", "℮", "゛", "·", "x·", "a‌",
                                                 "\x7f", "\xa0", " ", "\U000e0100", "\ud800", "ascii\x7f", "\x80"]))
def test_predicates_fixed(name, s):
    same(name, s)


# ---------- split family ----------
seps = st.one_of(st.none(), st.sampled_from([",", " ", ",,", "ab", "　", "a", "\ud800", "\n", "ß", "aa"]),
                 any_text(3))
split_text = st.one_of(
    any_text(16),
    st.lists(st.sampled_from(["a", "b", ",", " ", "  ", "ab", "\t", "\n", "　", "\x1c", "\x85", "​",
                              "﻿", "\xa0", " ", "\ud800", "aa", "ß", "\x00"]), max_size=12).map("".join),
)
maxsplits = st.one_of(st.integers(-3, 6), st.sampled_from([100, -100, sys.maxsize, -sys.maxsize - 1]))


@given(st.sampled_from(["split", "rsplit"]), split_text, seps, maxsplits)
def test_split(name, s, sep, maxsplit):
    same(name, s, sep, maxsplit)


@given(st.sampled_from(["split", "rsplit"]), split_text, st.data())
def test_split_defaults(name, s, data):
    mode = data.draw(st.integers(0, 3))
    if mode == 0:
        same(name, s)
    elif mode == 1:
        same(name, s, data.draw(seps))
    elif mode == 2:
        same(name, s, maxsplit=data.draw(maxsplits))
    else:
        same(name, s, sep=data.draw(seps), maxsplit=data.draw(maxsplits))


lines_text = st.one_of(
    any_text(16),
    st.lists(st.sampled_from(["a", "b", "\n", "\r", "\r\n", "\x0b", "\x0c", "\x1c", "\x1d", "\x1e", "\x1f", "\x85",
                              " ", " ", "\n\r", "", "\ud800", " "]), max_size=10).map("".join),
)


@given(lines_text, st.one_of(st.none(), st.booleans(), st.sampled_from([0, 1, 2, -1])), st.integers(0, 2))
def test_splitlines(s, keepends, mode):
    if mode == 0:
        same("splitlines", s)
    elif mode == 1:
        same("splitlines", s, keepends)
    else:
        same("splitlines", s, keepends=keepends)


strip_text = st.one_of(
    any_text(12),
    st.lists(st.sampled_from(["a", "b", "x", " ", "\t", "\n", "\x1c", "\x1f", "\x85", "\xa0", "　", "​",
                              "﻿", " ", "\ud800", "ab", "ß"]), max_size=10).map("".join),
)
strip_chars = st.one_of(st.none(), st.sampled_from(["", "x", "ab", " x", "​", "\ud800", "a\ud800 "]), any_text(4))


@given(st.sampled_from(["strip", "lstrip", "rstrip"]), strip_text, strip_chars, st.integers(0, 2))
def test_strip(name, s, chars, mode):
    if mode == 0:
        same(name, s)
    elif mode == 1:
        same(name, s, chars)
    else:
        same(name, s, chars=chars)


# ---------- padding ----------
widths = st.one_of(st.integers(-5, 40), st.sampled_from([-sys.maxsize - 1, True, False]))
pad_text = st.one_of(any_text(8), st.sampled_from(["", "-5", "+5", "-", "+", "--1", "+-1", "5-", "-abc", "\ud800",
                                                    "日本", "x\ud800", "-\ud800", "+日", " -1", "٣", "0"]))
fills = st.one_of(char(), st.sampled_from([" ", "*", "日", "\ud800", "\x00", "-", "0"]))


@given(st.sampled_from(["center", "ljust", "rjust"]), pad_text, widths, fills, st.integers(0, 2))
def test_padding(name, s, width, fill, mode):
    if mode == 0:
        same(name, s, width)
    elif mode == 1:
        same(name, s, width, fill)
    else:
        same(name, s, width, fillchar=fill)


@given(pad_text, widths)
def test_zfill(s, width):
    same("zfill", s, width)


tab_text = st.one_of(any_text(10), st.lists(st.sampled_from(["a", "bc", "\t", "\n", "\r", "\r\n", "日", "\x0b",
                                                               "\x85", " ", "\ud800", " ", "\x00"]),
                                             max_size=12).map("".join))


@given(tab_text, st.one_of(st.integers(-3, 17), st.just(True)), st.integers(0, 2))
def test_expandtabs(s, tabsize, mode):
    if mode == 0:
        same("expandtabs", s)
    elif mode == 1:
        same("expandtabs", s, tabsize)
    else:
        same("expandtabs", s, tabsize=tabsize)


# ---------- normalization ----------
norm_text = st.one_of(
    any_text(10),
    st.lists(st.one_of(st.sampled_from(SPECIAL), st.integers(0x300, 0x36F).map(chr),
                       st.integers(0xAC00, 0xD7A3).map(chr), st.integers(0x1100, 0x11FF).map(chr),
                       st.integers(0xF900, 0xFAFF).map(chr), st.integers(0x1E00, 0x1FFF).map(chr),
                       st.integers(0x0591, 0x05C7).map(chr), st.integers(0xFB1D, 0xFB4F).map(chr),
                       st.integers(0x0F71, 0x0F81).map(chr), st.integers(0x2F800, 0x2FA1D).map(chr),
                       st.sampled_from("aeiouAEOUnNcCsSyY")), max_size=10).map("".join),
)


@given(st.sampled_from(FORMS), norm_text)
def test_normalize(form, s):
    same("normalize", form, s)
    same("is_normalized", form, s)


@given(st.sampled_from(FORMS), norm_text)
def test_normalize_idempotent_input(form, s):
    # feed normalized forms back in (exercises quick-check yes paths)
    import unicodedata
    for f in FORMS:
        try:
            t = unicodedata.normalize(f, s)
        except Exception:
            continue
        same("is_normalized", form, t)
        same("normalize", form, t)


@settings(max_examples=40)
@given(st.one_of(st.sampled_from(["nfc", "NFKC ", "", "NFE", "nfd", "NFC\x00", "NFκC"]), BAD_ARG),
       st.one_of(norm_text, BAD_ARG))
def test_normalize_bad(form, s):
    same("normalize", form, s)
    same("is_normalized", form, s)


@settings(max_examples=30)
@given(st.sampled_from(FORMS), BAD_ARG)
def test_normalize_bad_text(form, s):
    same("normalize", form, s)
    same("is_normalized", form, s)


# ---------- unicodedata per-char ----------
@settings(max_examples=300)
@given(st.sampled_from(CHARFN), char())
def test_char_props(name, c):
    same(name, c)


@settings(max_examples=300)
@given(char())
def test_char_name(c):
    same("char_name", c)
    same("char_name", c, "?")


@given(char(), st.sampled_from([None, "", "?", 0, -1, 1.5, b"x", ["d"], False, "\ud800"]), st.booleans())
def test_char_name_default(c, default, kw):
    if kw:
        same("char_name", c, default=default)
    else:
        same("char_name", c, default)


@settings(max_examples=300)
@given(st.sampled_from(NUMFN), char())
def test_numeric(name, c):
    same(name, c)


num_chars = st.one_of(char(), st.sampled_from("0123456789٠٣۹०௯๙０²³¹⁰₉①⑨⑩⒈⓪❶➓¼½¾⅓⅟ⅠⅫⅰↂ〇〡〸㆒㈠㉈㊀一二三四五六七八九十百千万億兆零壹弍"
                                             "\U00010107\U00010320\U0001d7ce\U0001d7ff\U0001f100\U0001f10a\U0001ecb4"
                                             "\U00012432\U00012399\U0001e8c7\U0001d360\U000109bc\U00011fc0\U000f0000"
                                             "௰௱௲൘൞ᛮᛰ፩፲፼ⅆ៰ↁ\ud800a\x00"))


@given(st.sampled_from(NUMFN), num_chars,
       st.sampled_from([None, 0, -1, 1, 1.5, float("nan"), -0.0, float("inf"), "x", "", b"", [], False, True,
                        10 ** 30]),
       st.integers(0, 2))
def test_numeric_default(name, c, default, mode):
    if mode == 0:
        same(name, c)
    elif mode == 1:
        same(name, c, default)
    else:
        same(name, c, default=default)


@settings(max_examples=60)
@given(st.sampled_from(CHARFN + NUMFN + ["char_name", "lookup"]),
       st.one_of(st.sampled_from(["", "ab", "à", "𐀀", "\U0001f600x", "  "]), BAD_ARG))
def test_char_bad(name, arg):
    same(name, arg)


# ---------- lookup ----------
def _names():
    import unicodedata
    out = []
    for c in SPECIAL:
        n = unicodedata.name(c, None)
        if n:
            out.append(n)
    return out


NAMES = _names() + [
    "LATIN SMALL LETTER A", "latin small letter a", "Latin Small Letter A", "HANGUL SYLLABLE GA",
    "HANGUL SYLLABLE GAG", "hangul syllable gag", "HANGUL SYLLABLE HIH", "HANGUL SYLLABLE KKWAELH",
    "HANGUL SYLLABLE", "HANGUL SYLLABLE X", "HANGUL SYLLABLE GAX", "HANGUL SYLLABLE A", "HANGUL SYLLABLE YEOSS",
    "CJK UNIFIED IDEOGRAPH-4E00", "CJK UNIFIED IDEOGRAPH-4e00", "CJK UNIFIED IDEOGRAPH-9FFF",
    "CJK UNIFIED IDEOGRAPH-A000", "CJK UNIFIED IDEOGRAPH-3400", "CJK UNIFIED IDEOGRAPH-4DBF",
    "CJK UNIFIED IDEOGRAPH-4DC0", "CJK UNIFIED IDEOGRAPH-20000", "CJK UNIFIED IDEOGRAPH-2A6DF",
    "CJK UNIFIED IDEOGRAPH-2A6E0", "CJK UNIFIED IDEOGRAPH-2EBF0", "CJK UNIFIED IDEOGRAPH-2EBE0",
    "CJK UNIFIED IDEOGRAPH-31350", "CJK UNIFIED IDEOGRAPH-323AF", "CJK UNIFIED IDEOGRAPH-4E0",
    "CJK UNIFIED IDEOGRAPH-04E00", "CJK UNIFIED IDEOGRAPH-", "CJK UNIFIED IDEOGRAPH-XYZW",
    "CJK UNIFIED IDEOGRAPH-4E00 ", "CJK COMPATIBILITY IDEOGRAPH-F900", "CJK COMPATIBILITY IDEOGRAPH-2F800",
    "TANGUT IDEOGRAPH-17000", "TANGUT IDEOGRAPH-187F7", "TANGUT IDEOGRAPH-18D08", "TANGUT IDEOGRAPH-18D09",
    "KHITAN SMALL SCRIPT CHARACTER-18B00", "KHITAN SMALL SCRIPT CHARACTER-18CD5",
    "NUSHU CHARACTER-1B170", "NUSHU CHARACTER-1B2FB",
    "EM DASH", "NOT A NAME", "", " ", "LINE FEED", "BYTE ORDER MARK", "SPACE ", " SPACE", "SPACE",
    "NULL", "LF", "NBSP", "ZWJ", "BOM", "ZWNBSP", "CHARACTER TABULATION", "TAB", "END OF LINE",
    "LATIN CAPITAL LETTER GHA", "LATIN SMALL LETTER GHA", "LATIN CAPITAL LETTER OI",
    "LATIN SMALL LETTER A WITH MACRON AND GRAVE", "LATIN CAPITAL LETTER A WITH MACRON AND GRAVE",
    "TAMIL SYLLABLE KSSA", "KEYCAP NUMBER SIGN", "BENGALI LETTER KHINYA",
    "HIRAGANA LETTER BIDAKUON NGA", "LATIN SMALL LETTER R WITH TILDE",
    "LATIN SMALL LETTER A WITH ACUTE", "LATIN SMALL LETTER A WITH ACUTE ", "LATIN  SMALL LETTER A",
    "LATIN SMALL LETTER A\x00", "latin_small_letter_a", "GRINNING FACE", "grinning face",
    "PRESENTATION FORM FOR VERTICAL RIGHT WHITE LENTICULAR BRAKCET",
    "PRESENTATION FORM FOR VERTICAL RIGHT WHITE LENTICULAR BRACKET", "BYZANTINE MUSICAL SYMBOL FHTORA SKLIRON CHROMA VASIS",
    "HANGUL JONGSEONG YESIEUNG-SIOS", "TIBETAN MARK BKA- SHOG GI MGO RGYAN", "TIBETAN MARK BKA-SHOG GI MGO RGYAN",
    "MARCHEN LETTER -A", "LATIN SMALL LETTER SHARP S", "LATIN CAPITAL LETTER SHARP S", "ǅ", "\ud800",
    "LATİN SMALL LETTER A", "x" * 300, "A" * 256, "VARIATION SELECTOR-1", "VARIATION SELECTOR-256",
    "VARIATION SELECTOR-257", "EGYPTIAN HIEROGLYPH A001", "MODIFIER LETTER CAPITAL A",
]


@settings(max_examples=250)
@given(st.one_of(st.sampled_from(NAMES), char().map(lambda c: __import__("unicodedata").name(c, "") or "NONAME"),
                 st.sampled_from(NAMES).map(str.lower), st.sampled_from(NAMES).map(lambda n: n[:-1]),
                 st.sampled_from(NAMES).flatmap(lambda n: any_text(2).map(lambda t: n + t)),
                 st.integers(0x3400, 0x323AF).map(lambda i: "CJK UNIFIED IDEOGRAPH-%X" % i),
                 st.integers(0x17000, 0x18D10).map(lambda i: "TANGUT IDEOGRAPH-%X" % i),
                 st.integers(0x18B00, 0x18CFF).map(lambda i: "KHITAN SMALL SCRIPT CHARACTER-%X" % i),
                 st.integers(0x1B170, 0x1B2FF).map(lambda i: "NUSHU CHARACTER-%X" % i),
                 st.integers(0xF900, 0xFAFF).map(lambda i: "CJK COMPATIBILITY IDEOGRAPH-%X" % i),
                 st.integers(0xAC00, 0xD7A3).map(lambda i: __import__("unicodedata").name(chr(i))),
                 any_text(6)))
def test_lookup(name):
    same("lookup", name)


# ---------- misc ----------
@settings(max_examples=30)
@given(st.integers(0, 3))
def test_unidata_version(_n):
    same("unidata_version")


@settings(max_examples=5)
@given(BAD_ARG)
def test_unidata_version_bad(arg):
    same("unidata_version", arg)


# ---------- type guard / error paths ----------
ALL_STR1 = CASE + PRED + ["splitlines", "strip", "lstrip", "rstrip", "split", "rsplit", "expandtabs"]


@settings(max_examples=80)
@given(st.sampled_from(ALL_STR1), BAD_ARG)
def test_bad_first_arg(name, arg):
    same(name, arg)


@settings(max_examples=60)
@given(st.sampled_from(["split", "rsplit"]), any_text(6), st.one_of(st.none(), st.just(","), BAD_ARG),
       st.one_of(st.integers(-2, 3), BAD_ARG, st.just(2 ** 64), st.just(-(2 ** 64))))
def test_split_bad(name, s, sep, maxsplit):
    same(name, s, sep, maxsplit)


@settings(max_examples=60)
@given(st.sampled_from(["split", "rsplit", "strip", "lstrip", "rstrip", "splitlines", "expandtabs", "center", "ljust",
                        "rjust", "zfill", "char_name", "numeric_value", "decimal_value", "digit_value", "lookup",
                        "normalize", "category"]),
       st.dictionaries(st.sampled_from(["sep", "maxsplit", "chars", "keepends", "tabsize", "width", "fillchar",
                                        "default", "s", "ch", "name", "form", "bogus"]),
                       st.one_of(st.sampled_from(["a", "NFC", 3, None, -1, True]), NON_BUILTIN), max_size=3))
def test_kwargs_mix(name, kwargs):
    same(name, **kwargs)


@settings(max_examples=60)
@given(st.sampled_from(["strip", "lstrip", "rstrip"]), any_text(6), BAD_ARG)
def test_strip_bad(name, s, chars):
    same(name, s, chars)


@settings(max_examples=60)
@given(st.sampled_from(["center", "ljust", "rjust"]), pad_text,
       st.one_of(st.integers(-2, 8), BAD_ARG, st.just(2 ** 64)),
       st.one_of(st.sampled_from(["", "ab", "*"]), BAD_ARG))
def test_padding_bad(name, s, width, fill):
    same(name, s, width, fill)


@settings(max_examples=40)
@given(st.one_of(pad_text, BAD_ARG), st.one_of(st.integers(-2, 8), BAD_ARG, st.just(2 ** 64)))
def test_zfill_bad(s, width):
    same("zfill", s, width)


@settings(max_examples=40)
@given(tab_text, st.one_of(BAD_ARG, st.just(2 ** 64), st.just(-(2 ** 64))))
def test_expandtabs_bad(s, tabsize):
    same("expandtabs", s, tabsize)


@settings(max_examples=40)
@given(lines_text, BAD_ARG)
def test_splitlines_bad(s, keepends):
    same("splitlines", s, keepends)


@settings(max_examples=40)
@given(st.sampled_from(NUMFN + ["char_name"]), num_chars, NON_BUILTIN)
def test_default_nonbuiltin(name, c, default):
    same(name, c, default)


@settings(max_examples=30)
@given(st.sampled_from(CASE + PRED + CHARFN + NUMFN + ["char_name", "lookup", "zfill", "center", "normalize"]),
       st.lists(st.one_of(st.just("a"), st.just(1)), min_size=0, max_size=4))
def test_arity(name, args):
    same(name, *args)


# Deterministic sweep: every 7th code point plus block edges, all per-char functions.
SWEEP = sorted(set(list(range(0, 0x110000, 97)) + [0x10FFFF, 0xD7A3, 0xAC00, 0x4DBF, 0x9FFF, 0x3400, 0x20000,
                                                    0x2A6DF, 0x323AF, 0x18D08, 0x18CD5, 0x1B2FB, 0xD800, 0xDFFF]))


def test_sweep():
    for i, cp in enumerate(SWEEP):
        c = chr(cp)
        name = CHARFN[i % len(CHARFN)]
        same(name, c)
        same(NUMFN[i % 3], c, -1)
        same("char_name", c, "")
        same((CASE + PRED)[i % len(CASE + PRED)], c)


# ---------- argument conversion depth boundary ----------
# The port's conversion caps nesting (decisions.md: RecursionError beyond depth 900). Inputs whose deepest
# element sits exactly at depth 900 must still convert and give Python's TypeError; this pins the off-by-one.
# Values deeper than ~300 crash the reference worker in transit, so each side builds them via PythonExpression.
def _nest_expr(depth, leaf, kinds):
    wrap = "[x]" if kinds == "list" else "(x,)" if kinds == "tuple" else "([x] if i % 2 else (x,))"
    return PythonExpression("__import__('functools').reduce(lambda x, i: %s, range(%d), %s)" % (wrap, depth, leaf))


@settings(max_examples=30, deadline=None)
@given(st.sampled_from(["upper", "lower", "casefold", "strip", "split", "expandtabs", "category", "normalize"]),
       st.one_of(st.just(900), st.integers(880, 900), st.integers(0, 900)),
       st.sampled_from(["1", "'a'", "None", "1.5", "b'x'", "[]", "()"]),
       st.sampled_from(["list", "tuple", "mixed"]),
       st.booleans())
@example("upper", 900, "1", "list", False)
@example("lower", 900, "[]", "tuple", False)
def test_deep_nesting_boundary(name, depth, leaf, kinds, last):
    arg = _nest_expr(depth, leaf, kinds)
    if name == "normalize":
        if last:
            same(name, "NFC", arg)
        else:
            same(name, arg, "a")
    else:
        same(name, arg)


# ---------- review finding #2: guard must run before (and independently of) deep conversion ----------
# Upstream checks args left to right, depth-first (dict keys before values) and raises TypeError at the first
# non-builtin, so a non-builtin that precedes a deeply nested value must win over any RecursionError.
# Deep values are built on each side via PythonExpression (see _nest_expr).
_DEEP = "__import__('functools').reduce(lambda x, i: %s, range(%d), %s)"
_BAD_LEAVES = ["__import__('decimal').Decimal(1)", "object()", "type('S', (str,), {})('abc')", "str"]


def _deep_src(depth, kinds="list", leaf="1"):
    wrap = "[x]" if kinds == "list" else "(x,)" if kinds == "tuple" else "([x] if i % 2 else (x,))"
    return _DEEP % (wrap, depth, leaf)


@settings(max_examples=40, deadline=None)
@given(st.sampled_from(["upper", "lower", "casefold", "strip", "split", "expandtabs", "category", "isalpha"]),
       st.sampled_from(_BAD_LEAVES),
       st.one_of(st.integers(901, 980), st.integers(1000, 3000)),
       st.sampled_from(["list", "tuple", "mixed"]),
       st.sampled_from(["list", "tuple", "dict_key", "dict_val", "nested"]))
@example("upper", "__import__('decimal').Decimal(1)", 950, "list", "list")
@example("upper", "object()", 2000, "list", "list")
@example("category", "__import__('decimal').Decimal(1)", 950, "tuple", "dict_key")
def test_nonbuiltin_before_deep_value(name, bad, depth, kinds, container):
    deep = _deep_src(depth, kinds)
    if container == "list":
        src = "[%s, %s]" % (bad, deep)
    elif container == "tuple":
        src = "(%s, %s)" % (bad, deep)
    elif container == "dict_key":
        src = "{%s: %s}" % (bad, deep) if bad != "object()" else "{object(): %s}" % deep
    elif container == "dict_val":
        src = "{'k': %s, 'z': %s}" % (bad, deep)
    else:
        src = "[['a', [%s]], (%s,)]" % (bad, deep)
    same(name, PythonExpression(src))


@settings(max_examples=40, deadline=None)
@given(st.sampled_from(["split", "rsplit", "center", "ljust", "rjust", "strip", "normalize", "expandtabs",
                        "numeric_value", "splitlines"]),
       st.sampled_from(_BAD_LEAVES),
       st.one_of(st.integers(901, 980), st.integers(1000, 3000)),
       st.sampled_from(["list", "tuple", "mixed"]),
       st.booleans())
@example("split", "__import__('decimal').Decimal(1)", 950, "list", False)
@example("center", "object()", 2000, "tuple", True)
def test_nonbuiltin_arg_before_deep_arg(name, bad, depth, kinds, as_kw):
    # first positional arg is a non-builtin; a later argument is deeply nested.
    bad_v = PythonExpression(bad)
    deep_v = PythonExpression(_deep_src(depth, kinds))
    if name in ("center", "ljust", "rjust"):
        same(name, bad_v, deep_v)
    elif name == "normalize":
        same(name, bad_v, deep_v)
    elif name in ("numeric_value",):
        same(name, bad_v, deep_v)
    elif name == "splitlines":
        if as_kw:
            same(name, bad_v, keepends=deep_v)
        else:
            same(name, bad_v, deep_v)
    elif as_kw and name in ("split", "rsplit"):
        same(name, bad_v, sep=deep_v)
    elif as_kw and name == "expandtabs":
        same(name, bad_v, tabsize=deep_v)
    else:
        same(name, bad_v, deep_v)


# Plain deep nesting just past the port's old cap (900): upstream still reaches the leaf and raises TypeError
# (reviewer measured upstream's first RecursionError at depth 998 when called directly).
@settings(max_examples=30, deadline=None)
@given(st.sampled_from(["upper", "lower", "strip", "split", "category", "isdigit"]),
       st.integers(901, 960),
       st.sampled_from(["1", "None", "b'x'", "[]", "__import__('decimal').Decimal(1)"]),
       st.sampled_from(["list", "tuple", "mixed"]))
@example("upper", 950, "1", "list")
@example("upper", 950, "__import__('decimal').Decimal(1)", "list")
@example("upper", 901, "1", "tuple")
def test_deep_nesting_past_old_cap(name, depth, leaf, kinds):
    same(name, PythonExpression(_deep_src(depth, kinds, leaf)))
