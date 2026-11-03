"""Exhaustive table coverage: every code point through the case mappings (batched into long
strings), one representative per behaviour class through every predicate, and structured
Hangul jamo sequences through normalization. Random sampling misses single table rows."""
import sys
import unicodedata

from hypothesis import example, given, settings, strategies as st
from portwright.compare import same

CHUNK = 4096
ALL_CPS = list(range(0x110000))
CHUNKS = ["".join(map(chr, ALL_CPS[i:i + CHUNK])) for i in range(0, len(ALL_CPS), CHUNK)]

PRED = ["isalpha", "isdigit", "isdecimal", "isnumeric", "isalnum", "isspace", "isupper", "islower",
        "istitle", "isidentifier", "isprintable", "isascii"]


def test_case_mappings_every_code_point():
    # Mappings are per character (sigma aside), so a whole chunk checks each row's deltas/EXT entries.
    for chunk in CHUNKS:
        for name in ("upper", "lower", "casefold", "swapcase"):
            same(name, chunk)


def test_title_mapping_every_code_point():
    # Space-separated: each char is at a word start -> titlecase mapping of every code point.
    for chunk in CHUNKS:
        same("title", " ".join(chunk))
        # After a cased letter -> lower path for every code point (and cased-flag of each char).
        same("title", " ".join("A" + c + "b" for c in chunk[:1024]))
        same("title", " ".join("A" + c + "b" for c in chunk[1024:2048]))
        same("title", " ".join("A" + c + "b" for c in chunk[2048:3072]))
        same("title", " ".join("A" + c + "b" for c in chunk[3072:]))


def _signature(cp):
    c = chr(cp)
    return (c.isalpha(), c.isdecimal(), c.isdigit(), c.isnumeric(), c.isspace(), c.isupper(),
            c.islower(), c.istitle(), c.isprintable(), c.isidentifier(), ("a" + c).isidentifier(),
            ("A" + c + "Σ").lower()[-1], ("AΣ" + c).lower()[1],
            tuple(ord(x) - cp for x in ("a" + c).title()[1:]), ("ǅ" + c).istitle(),
            ("A" + c).istitle(), (c + "a").istitle(),
            tuple(ord(x) - cp for x in c.upper()), tuple(ord(x) - cp for x in c.lower()),
            tuple(ord(x) - cp for x in c.title()), tuple(ord(x) - cp for x in c.casefold()))


def _representatives():
    seen = {}
    for cp in ALL_CPS:
        seen.setdefault(_signature(cp), cp)
    return sorted(seen.values())


REPS = _representatives()


def test_predicates_per_class():
    # One code point per observable behaviour class; each predicate alone and in contexts that
    # expose the cased / case-ignorable / ID_CONTINUE / titlecase flags.
    for cp in REPS:
        c = chr(cp)
        for name in PRED:
            same(name, c)
        same("istitle", "A" + c)
        same("istitle", c + "a")
        same("istitle", "ǅ" + c)
        same("isidentifier", "a" + c)
        same("lower", "A" + c + "Σ")
        same("lower", "AΣ" + c)
        same("lower", "AΣ" + c + "a")
        same("swapcase", "AΣ" + c + "Σ")
        same("capitalize", c + "AΣ")
        same("capitalize", "a" + c)


# ---------- Hangul composition: L V [T] including the invalid T_BASE (U+11A7) and edges ----------
L = list(range(0x1100, 0x1113)) + [0x10FF, 0x1113]
V = list(range(0x1161, 0x1176)) + [0x1160, 0x1176]
T = list(range(0x11A7, 0x11C3)) + [0x11A6, 0x11C3]
jamo = st.one_of(st.sampled_from(L), st.sampled_from(V), st.sampled_from(T),
                 st.sampled_from([0xAC00, 0xAC01, 0xD7A3, 0xD7A4, 0x300, 0x61])).map(chr)
hangul_text = st.one_of(
    st.lists(jamo, max_size=10).map("".join),
    st.tuples(st.sampled_from(L), st.sampled_from(V), st.sampled_from(T + [0x61, 0x300]))
    .map(lambda t: "".join(map(chr, t))),
    st.tuples(st.sampled_from([0xAC00, 0xAC1C, 0xD788]), st.sampled_from(T)).map(lambda t: "".join(map(chr, t))),
)


@given(st.sampled_from(["NFC", "NFD", "NFKC", "NFKD"]), hangul_text)
@example("NFC", "가ᆧ")
@example("NFKC", "가ᆧ")
@example("NFC", "힣")
@example("NFC", "히ᇃ")
def test_hangul_normalize(form, s):
    same("normalize", form, s)
    same("is_normalized", form, s)


def test_hangul_lvt_exhaustive_edges():
    for l in (0x1100, 0x1112):
        for v in (0x1161, 0x1175):
            for t in T:
                s = chr(l) + chr(v) + chr(t)
                same("normalize", "NFC", s)
                same("normalize", "NFKC", s)
                same("is_normalized", "NFC", s)


# ---------- rsplit with whitespace left over after maxsplit is exhausted ----------
@settings(max_examples=200)
@given(st.lists(st.sampled_from(["a", "bc", " ", "  ", "\t", "　", "\x85", "\ud800"]), max_size=10).map("".join),
       st.integers(0, 5))
@example("a  b", 1)
@example("x \t y  z ", 2)
@example("  a b", 1)
def test_rsplit_whitespace_leftover(s, maxsplit):
    same("rsplit", s, None, maxsplit)
    same("rsplit", s, maxsplit=maxsplit)
