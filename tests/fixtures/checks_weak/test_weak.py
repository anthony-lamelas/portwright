"""Deliberately broken checks, based on a real failure in a textdistance port.

Every call passes a bad argument, so both versions raise the same TypeError. A naive
harness would report zero differences here even though no real logic ever ran.
"""

from hypothesis import given
from hypothesis import strategies as st

from portwright.compare import same


@given(st.text())
def test_slug(text):
    same("slug", text, shared_option=True)


@given(st.text())
def test_truncate(text):
    same("truncate", text, shared_option=True)
