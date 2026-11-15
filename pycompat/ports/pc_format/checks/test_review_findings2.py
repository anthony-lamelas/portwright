"""Second-pass review findings turned into checks.

#1 str_format: upstream refuses attribute fields *before* formatting, so a refusal
   TypeError beats any format error (IndexError/KeyError/ValueError) earlier in the
   template. Guards against "format first, refuse later" reordering.
#2 silent unwrap_or fallbacks: percent output mixing 1/2/4-byte code points (Units
   widening), %c / {:c} at width boundaries, huge fixed precisions (decpt math).
#5 code points > 0x10FFFF must be rejected (%c, {:c}).
#6 KeyError arg is the key itself (not a Debug string).
"""
from hypothesis import example, given, strategies as st
from portwright.compare import same
from portwright.compare.strategies import text

from pcf_strategies import ints, floats

# ---------- #1 attribute refusal vs. format errors ----------

BEFORE = ["{1}", "{0:d}", "{a}", "{0[5]}", "{0:%}", "{0!x}", "{0:{1}}", "{:}", "{0:,s}",
          "{0:=s}", "{0:99999999999999999999}", "{0:.{9}}", "{}", "x", "{{", "{0:{a}}"]
ATTRS = ["{0.real}", "{.imag}", "{a.b}", "{0[0].x}", "{0:{1.real}}", "{0:>{0.real}}",
         "{0.}", "{0..x}", "{0:{0:{0.real}}}"]
AFTER = ["", "{", "}", "{0", "{0:d}", "{9}", "{0!}"]


@example("{1}{0.real}", ["s"])
@example("{0:d}{0.real}", ["s"])
@example("{a}{0.real}", [1])
@example("{0.real}", [])
@example("{0:%}{1.real}", ["x", 1])
@example("{0:{1.imag}}", ["s", 1])
@example("{0:xyz}{0.real}{", [1])
@example("{0[5]}{0.real}", [[1]])
@example("{1}{0:{0:{0.real}}}", [1])
@given(st.tuples(st.sampled_from(BEFORE), st.sampled_from(ATTRS), st.sampled_from(AFTER)).map("".join),
       st.lists(st.one_of(st.integers(-5, 5), text(max_size=3), floats(), st.lists(st.integers(0, 3), max_size=2)),
                max_size=3))
def test_refusal_beats_format_errors(template, args):
    same("str_format", template, args)
    same("str_format", template, args, {"a": 1})


# ---------- #2/#5 mixed-width output and %c / {:c} boundaries ----------

NARROW = st.characters(min_codepoint=0, max_codepoint=0xFF)
UCS2 = st.characters(min_codepoint=0x100, max_codepoint=0xFFFF)
WIDE = st.characters(min_codepoint=0x10000, max_codepoint=0x10FFFF)
CHUNK = st.one_of(st.text(NARROW, max_size=3), st.text(UCS2, max_size=3), st.text(WIDE, max_size=3), text(max_size=3))
CP = st.one_of(st.sampled_from([0, 1, 0x7F, 0x80, 0xFF, 0x100, 0xD7FF, 0xD800, 0xDFFF, 0xE000, 0xFFFF,
                                0x10000, 0x10FFFF, 0x110000, -1, 2**31, 2**63, -2**63]),
               st.integers(0, 0x10FFFF))
DIRECTIVES = ["%s", "%r", "%a", "%c", "%5c", "%-4s", "%.2s", "%3r", "%d", "%x"]


@example("\U0001F600%s", ("é",))
@example("é%s%s", ("\U0001F600", "ā"))
@example("%c%c%c", (0xFF, 0x100, 0x10000))
@example("%c", (0x10FFFF,))
@example("%c", (0x110000,))
@example("%c%s", (0xD800, "\udc00"))
@example("Ā%c", (0x10FFFF,))
@example("%5c|%-5c", (0x10000, 0xFF))
@given(st.lists(st.tuples(CHUNK, st.sampled_from(DIRECTIVES)), max_size=4).map(
           lambda ps: "".join(c + d for c, d in ps)),
       st.lists(st.one_of(CHUNK, CP), max_size=5).map(tuple))
def test_percent_mixed_widths(template, vals):
    same("percent_format", template, vals)


@example(0x110000, "c")
@example(0x10FFFF, "c")
@example(-1, "c")
@example(0x100, ">5c")
@given(CP, st.sampled_from(["c", ">3c", "<2c", "^4c", "*^5c", "\U0001F600>3c", "é<3c", "", "x"]))
def test_c_boundaries(cp, spec):
    same("format_value", cp, spec)
    same("str_format", "{:" + spec + "}", [cp])
    same("percent_format", "%c", cp)
    same("percent_format", "%c", chr(cp) if 0 <= cp <= 0x10FFFF else cp)


# ---------- #2 fixed-mode digits at huge precisions ----------

PRECISIONS = st.one_of(st.sampled_from([0, 1, 17, 20, 36, 37, 330, 767, 768, 1074, 1075, 1099, 1100, 1101, 1200]),
                       st.integers(0, 1300))


@example(5e-324, 1100)
@example(5e-324, 1075)
@example(1.7976931348623157e308, 1100)
@example(0.5, 1101)
@example(-0.0, 1200)
@example(1e-320, 1074)
@example(123456789.125, 1100)
@given(floats(), PRECISIONS)
def test_huge_fixed_precision(x, p):
    for t in ("f", "e", "g", "%"):
        same("format_value", x, f".{p}{t}")
    same("percent_format", f"%.{p}f", x)
    same("percent_format", f"%.{p}g", x)
    same("round_number", x, p - 650)


# ---------- #6 KeyError carries the key itself ----------

@example("%(a)s", {"b": 1})
@example("%(\udc80)s", {})
@example("%(é)s", {1: 2})
@example("%()s", {"x": 1})
@given(text(max_size=4).filter(lambda s: ")" not in s), st.dictionaries(st.one_of(text(max_size=2), st.integers()), st.integers(), max_size=2))
def test_keyerror_key(key, mapping):
    same("percent_format", "%(" + key + ")s", mapping)
    if not any(c in key for c in "{}[]:!.") and key and not key.isdigit():
        same("str_format", "{" + key + "}", [], mapping if all(isinstance(k, str) for k in mapping) else {})
        same("str_format", "{0[" + key + "]}", [mapping])
