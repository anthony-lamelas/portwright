"""Hypothesis strategies for text that Python allows but Rust strings usually don't.

``st.text()`` never produces lone surrogates, so ports that can't handle them pass
checks built on it. These strategies mix them in, along with characters whose
Unicode properties change between Unicode versions.
"""

from hypothesis import strategies as st

# Characters that commonly expose differences: NUL, dotted capital I, sharp s,
# zero-width joiners, a non-ASCII digit, and code points newer than the Unicode
# data in older Pythons.
TRAPS = ["\x00", "İ", "ß", "‌", "‍", "٣", "\U0001cc00", "\U0001fbf0", " "]


def characters() -> st.SearchStrategy[str]:
    """Any code point Python can hold, with lone surrogates and traps much more common."""
    return st.one_of(
        st.characters(exclude_categories=()),
        st.characters(categories=["Cs"]),
        st.sampled_from(TRAPS),
    )


def text(**kwargs) -> st.SearchStrategy[str]:
    """Like ``st.text()``, but drawn from ``characters()``. ``min_size``/``max_size`` pass through.

    Built from a list because ``st.text`` merges a ``one_of`` of character strategies
    and silently drops the surrogates again.
    """
    return st.lists(characters(), **kwargs).map("".join)
