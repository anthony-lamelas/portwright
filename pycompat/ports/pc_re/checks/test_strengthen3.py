"""Targeted checks for: non-BMP case-insensitive set ranges (RANGE_UNI_IGNORE followed by more
items), case-insensitive sets with non-ASCII members (IN_UNI_IGNORE vs other IN opcodes), and
the error position of (?P=name) references to open/unknown groups."""

from hypothesis import given, strategies as st
from portwright.compare import same, Pair
from portwright.compare.strategies import text


def _probe(m):
    if isinstance(m, Pair):
        same("match_span", m)


def _r(c):
    return "\\U%08x" % c


# Non-BMP blocks: uncased (emoji, CJK ext B) and cased (Deseret, Osage, Adlam).
NONBMP_BASES = [0x1F600, 0x20000, 0x10400, 0x10428, 0x104B0, 0x1E900, 0x1E922, 0x10FFF0]


@st.composite
def nonbmp_set(draw):
    items = []
    for _ in range(draw(st.integers(2, 5))):
        base = draw(st.sampled_from(NONBMP_BASES))
        a = min(0x10FFFF, base + draw(st.integers(0, 30)))
        if draw(st.booleans()):
            b = min(0x10FFFF, a + draw(st.integers(0, 8)))
            items.append(_r(a) + "-" + _r(b))
        else:
            items.append(_r(a))
    if draw(st.booleans()):
        items.insert(draw(st.integers(0, len(items))),
                     draw(st.sampled_from(["a", "b-d", "\\d", "\\w", "é", "K", "σ", "\\s", "x-z"])))
    neg = draw(st.sampled_from(["", "", "^"]))
    return "[" + neg + "".join(items) + "]"


@given(nonbmp_set(), st.sampled_from(["(?i)", "(?i)", "", "(?a)(?i)"]), st.data())
def test_nonbmp_ignorecase_sets(s, flag, data):
    p = flag + s
    subj = "".join(chr(min(0x10FFFF, b + k)) for b in NONBMP_BASES for k in range(0, 40, 3))
    subj += "aBc5 éÉKkσΣ" + "".join(chr(min(0x10FFFF, b + data.draw(st.integers(0, 45)))) for b in NONBMP_BASES)
    same("findall", p, subj)
    same("findall", p + "+", subj[::-1])
    _probe(same("search", "x" + p + "y", "x" + chr(min(0x10FFFF, data.draw(st.sampled_from(NONBMP_BASES))
                                                         + data.draw(st.integers(0, 40)))) + "y"))


NONASCII_ITEMS = ["é", "à-ÿ", "σ", "ß", "k", "s", "K", "Ω", "İ", "ı", "ǅ", "ſ", "ﬀ", "Ā-ſ", "α-ω", "А-Я", "a", "0-9",
                  "\\d", "\\w", "ǈ", "ϐ", "θ", "µ", "Å"]
CASE_SUBJECT = "éÉàÀÿŸσΣςßẞkKKsSſΩωΩİiIıǄǅǆﬀāĀαΑωаАяЯ5µΜμÅåÅǇǈǉϐβΒθϑϴΘxyz"


@given(st.lists(st.sampled_from(NONASCII_ITEMS), min_size=2, max_size=5),
       st.sampled_from(["", "^"]), st.sampled_from(["(?i)", "(?i)", "(?ai)", "(?ui)"]), text(max_size=8))
def test_ignorecase_nonascii_sets(items, neg, flag, extra):
    p = flag + "[" + neg + "".join(items) + "]"
    same("findall", p, CASE_SUBJECT + extra)
    same("findall", p + "+", extra + CASE_SUBJECT[::-1])
    for ch in CASE_SUBJECT[::5]:
        _probe(same("fullmatch", p, ch))


GROUP_NAMES = ["a", "bc", "name", "éé", "g1", "x_y_z", "Σσ"]


@given(st.sampled_from(GROUP_NAMES), st.sampled_from(GROUP_NAMES),
       st.sampled_from(["(?P<{d}>x(?P={r}))", "(?P<{d}>(?P={r}))", "(?P<{d}>a|(?P={r})b)", "x(?P<{d}>(?:(?P={r})))",
                        "(?P<{d}>a)(?P={r})", "(?P={r})(?P<{d}>a)", "ab\n(?P<{d}>c(?P={r}))", "(?P<{d}>a)\n(?P={r})",
                        "(?P<o>(?P<{d}>a)(?P={r}))", "(?x)  (?P<{d}> a (?P={r}) )"]))
def test_named_reference_error_positions(d, r, tmpl):
    p = tmpl.format(d=d, r=r)
    same("compile", p)
    same("search", p, "aaxx")
