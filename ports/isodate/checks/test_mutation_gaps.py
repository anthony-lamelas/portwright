"""Decimal inputs for Rust mutants the checks used to miss (harness issue #90).

isodate's port carries its own Decimal. A re-run of the mutation gate found mutants in
its addition with a zero operand, its division's exponent shift and quantize's digit
limit going unnoticed: the checks' decimals all had small exponents. These reach those
paths through fquotmod and Duration arithmetic.
"""

from decimal import Decimal

from hypothesis import example, given
from hypothesis import strategies as st

from portwright.compare import PythonExpression, same

COEFFICIENT = st.lists(st.integers(0, 9), min_size=1, max_size=34).map(tuple)
WIDE = st.one_of(
    st.builds(lambda sign, digits, exp: Decimal((sign, digits, exp)), st.integers(0, 1), COEFFICIENT, st.integers(-45, 45)),
    st.builds(lambda sign, exp: Decimal((sign, (0,), exp)), st.integers(0, 1), st.integers(-60, 60)),
)


@given(WIDE, WIDE, WIDE)
@example(Decimal("0E-40"), Decimal("1E+30"), Decimal("7"))
@example(Decimal("1E+30"), Decimal("0E-40"), Decimal("3"))
@example(Decimal("12345678901234567890123456789"), Decimal("1"), Decimal("3E-20"))
def test_fquotmod_wide_decimals(val, low, high):
    same("duration.fquotmod", val, low, high)


@given(WIDE, WIDE, st.integers(-3, 3))
@example(Decimal("0E-40"), Decimal("1E+20"), 2)
def test_duration_arithmetic_wide_decimals(years, months, factor):
    left = same("Duration", years=years, months=months)
    right = same("Duration", years=months, months=years)
    if left is not None and right is not None:
        left.call("__add__", right)
        left.call("__sub__", right)
        left.call("__mul__", factor)
        left.call("__eq__", right)


ORDINAL_DATE = '''
import datetime
class Day(datetime.date):
    def toordinal(self):
        return {kind}(super().toordinal()) + {shift}
r = Day({y}, {m}, {d})
'''


@given(st.integers(1, 9999), st.integers(1, 12), st.integers(1, 28),
       st.sampled_from(["float", "__import__('decimal').Decimal"]), st.sampled_from(["0", "0.5", "-0.25"]))
@example(2004, 3, 1, "float", "0")
def test_year_day_of_a_date_whose_ordinal_is_not_an_int(y, m, d, kind, shift):
    # %j is toordinal() - date(year, 1, 1).toordinal() + 1, kept as the type toordinal returns.
    code = ORDINAL_DATE.format(kind=kind, shift=shift, y=y, m=m, d=d)
    day = PythonExpression(f"(lambda ns: (exec({code!r}, ns), ns['r'])[1])({{}})")
    same("strftime", day, "%j")
    same("strftime", day, "%Y-%j")
