"""Inputs for Rust mutants the checks used to miss (harness issue #90).

A re-run of the mutation gate left two real gaps: ISO times that mix the compact and
extended styles ("1230:45"), which a parser that loosened its separator rule would
accept; and strptime input holding a lone surrogate before more text, where a
miscounted length shows in the "unconverted data remains" message. Four other missed
mutants can't change any output and need no checks.
"""

from hypothesis import example, given
from hypothesis import strategies as st

from portwright.compare import same

DIGITS = st.text(alphabet="0123456789", min_size=2, max_size=2)


@given(DIGITS, DIGITS, DIGITS, st.sampled_from(["{h}{m}:{s}", "{h}:{m}{s}", "{h}{m}{s}:{s}", "{h}:{m}:{s}", "{h}{m}{s}", "{h}{m}:{s}.5"]))
@example("12", "30", "45", "{h}{m}:{s}")
@example("12", "30", "45", "{h}{m}{s}:{s}")
def test_iso_clock_separator_styles(h, m, s, shape):
    clock = shape.format(h=h, m=m, s=s)
    same("time_fromisoformat", clock)
    same("datetime_fromisoformat", "2024-01-01T" + clock)


SURROGATES = st.sampled_from(["\udc80", "\ud800", "\udfff", "𐂀", "\udc80\udc80"])


@given(SURROGATES, st.sampled_from(["", " ", "x"]), st.sampled_from(["", " junk", "9", "\udc80"]))
@example("\udc80", " ", " junk")
def test_strptime_text_after_lone_surrogates(surrogate, gap, tail):
    same("strptime", f"{surrogate}{gap}2024{tail}", f"{surrogate}{gap}%Y")
    same("strptime", f"2024{gap}{surrogate}{tail}", "%Y")
