"""More review-finding rules (see test_review.py for the main set).

#1 round_int: positive/huge/bool ndigits on ints, and k right at bits(n)+1 for mid-size ints.
#2 memory budget: huge *precisions* in printf and str.format nested specs. CPython under the
   1 GiB worker limit refuses at once (MemoryError), says 'precision too big' past INT_MAX, or
   returns short text (inf/nan, 0.0 with 'g', %r/%s/%a ignore-ish precision on short reprs).
"""
import sys

from hypothesis import example, given, settings, strategies as st
from portwright.compare import same

from pcstrats import floats

# ----- #1 ---------------------------------------------------------------------------------


@given(st.one_of(st.integers(-10 ** 40, 10 ** 40), st.booleans()),
       st.one_of(st.integers(0, 50), st.booleans(),
                 st.sampled_from([2 ** 31, 2 ** 63 - 1, 2 ** 63, 2 ** 64, 10 ** 30, sys.maxsize + 1])))
@example(5, True)
@example(True, False)
@example(-15, 0)
@example(10 ** 40, 2 ** 64)
def test_round_int_nonnegative_ndigits(n, nd):
    same("round_number", n, nd)


@settings(deadline=None)
@given(st.integers(1, 4000), st.integers(-1, 1), st.booleans(), st.integers(-2, 2))
@example(64, 0, False, 0)
@example(63, 0, True, 1)
@example(3321, 0, False, 0)
def test_round_int_k_equals_bits_plus_one(digits, delta, neg, shift):
    # n with `digits` decimal digits (all 9s / 5 lead), k swept around bits(n)+1 and digits(n).
    for n in (10 ** digits - 1 + delta, 5 * 10 ** (digits - 1) + delta, 10 ** (digits - 1) + delta):
        if n <= 0:
            continue
        if neg:
            n = -n
        b = abs(n).bit_length()
        same("round_number", n, -(b + 1 + shift))
        k = digits + shift
        if k > 0:
            same("round_number", n, -k)


# ----- #2 ---------------------------------------------------------------------------------

HUGE_PREC = [1_100_000_000, 1_500_000_000, 2_000_000_000, 2_147_483_646, 2_147_483_647,
             2_147_483_648, 2 ** 40, sys.maxsize]
PVALUES = st.one_of(st.sampled_from([0, 7, -255, 10 ** 30, True, 0.0, -0.0, 1.5, -2.5, 1e300,
                                     float("inf"), float("-inf"), float("nan"), "ab", ""]),
                    floats(), st.integers(-10 ** 6, 10 ** 6))


@given(PVALUES, st.sampled_from(HUGE_PREC), st.sampled_from(list("fFeEgGdixXorsa")),
       st.sampled_from(["", "#", "+", "-", "0", " "]))
@example(1.5, 1_100_000_000, "f", "")
@example(7, 1_100_000_000, "d", "")
@example(1.5, 2_147_483_648, "f", "")
@example(1.5, 2_147_483_647, "f", "")
@example(1.5, 1_100_000_000, "g", "")
@example(0.0, 1_100_000_000, "g", "")
@example(255, 1_100_000_000, "x", "#")
@example(1.5, 2_000_000_000, "r", "")
@example(float("inf"), 1_100_000_000, "f", "")
def test_percent_huge_precision(value, prec, kind, flag):
    if kind in "dixXo" and not isinstance(value, (int, float)) or kind in "dixXo" and value != value:
        value = 7
    same("percent_format", f"%{flag}.{prec}{kind}", (value,))
    if prec <= 2 ** 40:
        same("percent_format", f"%{flag}.*{kind}", (prec, value))


@given(PVALUES, st.sampled_from(HUGE_PREC), st.sampled_from(["", "f", "e", "g", "%", "s", "d"]))
@example(1.5, 1_100_000_000, "f")
@example(7, 1_100_000_000, "f")
@example(0.0, 1_100_000_000, "g")
@example("ab", 2_147_483_648, "s")
def test_str_format_nested_huge_precision(value, prec, kind):
    same("str_format", "{0:.{1}" + kind + "}", (value, prec))
    same("str_format", "{v:>{w}.{p}" + kind + "}", (), {"v": value, "w": 3, "p": prec})
