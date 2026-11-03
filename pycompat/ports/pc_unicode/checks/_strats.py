import unicodedata
from decimal import Decimal
from hypothesis import strategies as st
from portwright.compare import PythonExpression
from portwright.compare.strategies import text, characters

INTERESTING_RANGES = [
    (0, 0x7F), (0x80, 0x2FF), (0x300, 0x36F), (0x370, 0x3FF), (0x400, 0x52F), (0x590, 0x6FF),
    (0x900, 0xDFF), (0xE00, 0xFFF), (0x1100, 0x11FF), (0x1E00, 0x1FFF), (0x2000, 0x206F),
    (0x2070, 0x218F), (0x2190, 0x2BFF), (0x2C00, 0x2E7F), (0x2E80, 0x2FFF), (0x3000, 0x33FF),
    (0x3400, 0x4DBF), (0x4E00, 0x9FFF), (0xA000, 0xABFF), (0xAC00, 0xD7AF), (0xD7B0, 0xD7FF),
    (0xD800, 0xDFFF), (0xE000, 0xF8FF), (0xF900, 0xFAFF), (0xFB00, 0xFFFF),
    (0x10000, 0x1FFFF), (0x17000, 0x18D08), (0x1B000, 0x1B2FF), (0x1D400, 0x1D7FF),
    (0x1F000, 0x1FAFF), (0x20000, 0x323AF), (0xE0000, 0xE01EF), (0xF0000, 0x10FFFF),
]

SPECIAL = ("\x00\t\n\x0b\x0c\r\x1c\x1d\x1e\x1f \x85\xa0   ​    "
           "　﻿᠎_-+0123456789٠٩۰߀०０²³¹¼½"
           "⅓ⅠⅫⅰ①⑩〇㐅一万兆十萬零\U0001f100"
           "\U0001d7ce༳፩᧚⳽\U00010107\U0001ecb4\U00012432\U0001f10c"
           "İıßŉǄǅǆǇǈǉǊǋǌǰͅ"
           "ΐΣςσẞᾈᾀᾳᾼῳῼﬀﬁﬃﬅﬓ"
           "ᎠꭰΩKÅևẖაᲐႠⴀⒶⓐᵃʰ"
           "\U00010400\U00010428\U0001e900\U0001e922\U0001d400̧́̈ͅאا‎‏"
           "‪‮⁦⁩(){}[]<>«»∈√༺（각가"
           "각힣ཱཱཱིྀུ̈́⫝̸יִשׁפֿ\U0001d15e\U0001d1bbẛ̣"
           "ﷺ⿀ﹿＡａ￠ªºˆᴬⁱⁿ々〻ゝ"
           "\U0001f600\U0001f1e6\U000e0001\U000e0020\U000e0100\ud800􏰀\udfff\U000f0000"
           "\U0010ffff￾￿\U0001fffe͸ՠࢗ㇯ㅤᅟﾠⸯ­͏"
           "\U0001b132\U0001b155\U0001d2c0\U0001e030\U0001f6dc\U0001fa89\U0002ebf0\U00031350ㇼㇽ"
           "\U00018b00\U00018cd5\U00018d00\U00018d08\U00017000\U000187f7\U0002a6df\U0002b739\U0002b740"
           "\U0002b81d\U0002b820\U0002cea1\U0002ceb0\U0002ebe0\U0002f800\U0002fa1d\U00030000\U0003134a"
           "\U000323af\U0001f202㌀㋿①⑴⒈⒜㉑㊱௰൘ᛮ"
           "๐၀႐០᠐᥆꩐꣐\U000104a0\U00011066\U0001e950\U0001fbf0"
           "ÀàÅÅẠǕ΅ϓ΅῭ά̀́̓ʹ"
           ";·क़〈〉  `´¨´¸˘゙゚"
           "ゔがガｶﾞ기ᆧᇂᇃ"
           )


def code_point_chars():
    return st.one_of(
        st.integers(0, 0x10FFFF).map(chr),
        st.sampled_from(INTERESTING_RANGES).flatmap(lambda r: st.integers(r[0], r[1])).map(chr),
        st.sampled_from(SPECIAL),
        characters(),
    )


def char():
    return code_point_chars()


def rich_text(max_size=12):
    return st.lists(st.one_of(code_point_chars(), st.sampled_from(SPECIAL)), max_size=max_size).map("".join)


def any_text(max_size=12):
    return st.one_of(rich_text(max_size), text(max_size=max_size))


# Inputs that are not strings, but are builtin types (TypeError from str method),
# and values that are non-builtin (TypeError from the guard).
NON_STR_BUILTIN = st.sampled_from([None, 0, 1, -1, 1.5, float("nan"), True, b"a", bytearray(b"ab"),
                                   ["a"], ("a",), {"a"}, frozenset(), {"a": 1}, [], {}, b""])
NON_BUILTIN = st.sampled_from([
    Decimal("1"), Decimal("NaN"), ["a", Decimal("2")], ("x", [Decimal("-0")]), {"k": Decimal("1")},
    {Decimal("3"): "v"}, frozenset([Decimal("1")]), {Decimal("4")},
    PythonExpression("lambda s: s"), PythonExpression("object()"), PythonExpression("['a', object()]"),
    PythonExpression("{'a': ('b', type)}"), PythonExpression("str"),
    PythonExpression("type('S', (str,), {})('abc')"), PythonExpression("type('I', (int,), {})(3)"),
    PythonExpression("[[[[1, {2: {3: (4, frozenset({5.0, None, b'x'}))}}]]]]"),
    PythonExpression("[[[[1, {2: {3: (4, frozenset({5.0, None, complex(1, 2)}))}}]]]]"),
    PythonExpression("range(3)"), PythonExpression("memoryview(b'ab')"),
])
BAD_ARG = st.one_of(NON_STR_BUILTIN, NON_BUILTIN)
