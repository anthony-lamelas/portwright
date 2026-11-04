"""Review finding #1: nesting limit for container arguments.

CPython's _check_builtin recurses once per container level, so the RecursionError cutoff
sits near the interpreter limit (~996 levels), and it is checked depth-first in argument
order: a Decimal in an earlier argument (or earlier inside the same container) wins over
a too-deep container later. The port must not apply its own, shallower limit during
conversion.

The harness worker rebuilds arguments recursively (~3 frames per level), so it crashes on
anything nested deeper than ~330 levels before either library is called: the 900 vs ~996
cutoff itself is unreachable (see /work/out/tried.md). What is reachable and checked here:
deep containers of every kind in every argument slot, and depth-first argument order
(Decimal earlier vs. later than a deep container).
"""

from decimal import Decimal

from hypothesis import given, example, settings, strategies as st
from portwright.compare import same

DEPTHS = [0, 1, 2, 3, 10, 50, 100, 200, 250, 290, 300]


def nest(n, kind="list", leaf=1):
    v = leaf
    for i in range(n):
        if kind == "list":
            v = [v]
        elif kind == "tuple":
            v = (v,)
        elif kind == "dict":
            v = {"k": v}
        elif kind == "frozenset":
            v = frozenset([v])
        elif kind == "mixed":
            v = [v] if i % 3 == 0 else ((v,) if i % 3 == 1 else {"k": v})
    return v


def build(n, kind, leaf, before):
    v = nest(n, kind, leaf)
    if before is not None:
        # an earlier sibling in the outermost container is checked first
        v = [before, v]
    return v


depth = st.one_of(st.sampled_from(DEPTHS), st.integers(0, 300))
kinds = st.sampled_from(["list", "tuple", "dict", "frozenset", "mixed"])
leaves = st.sampled_from([1, None, "a", 2.5, Decimal("1"), b"x"])
befores = st.sampled_from([None, None, 0, Decimal("2"), [Decimal("3")]])


@given(depth, kinds, leaves, befores, st.sampled_from(["flags", "pos", "kwflags", "count", "pattern_pos"]))
@example(0, "list", 1, None, "flags")
@example(1, "list", Decimal("1"), None, "flags")
@example(200, "mixed", 1, None, "flags")
@example(250, "list", 1, None, "flags")
@example(290, "dict", Decimal("1"), None, "count")
@example(300, "frozenset", 1, [Decimal("3")], "pos")
@example(300, "list", 1, None, "flags")
@example(250, "list", Decimal("1"), None, "flags")
@example(300, "list", Decimal("1"), None, "flags")
@example(250, "list", 1, Decimal("2"), "flags")
@example(300, "list", 1, Decimal("2"), "flags")
@example(250, "dict", 1, None, "kwflags")
@example(250, "tuple", 1, None, "count")
@example(250, "frozenset", 1, None, "pattern_pos")
@settings(max_examples=150, deadline=None)
def test_deep_container_argument(n, kind, leaf, before, where):
    v = build(n, kind, leaf, before)
    if where == "flags":
        same("search", "a", "a", v)
    elif where == "kwflags":
        same("match", "a", "ab", flags=v)
    elif where == "count":
        same("sub", "a", "b", "aaa", v)
    elif where == "pos":
        same("split", "a", "bab", v)
    else:
        c = same("compile", "a")
        same("pattern_search", c, "a", v)


@given(depth, kinds, st.sampled_from(["first", "second", "kw"]))
@example(300, "list", "first")
@example(250, "list", "first")
@example(300, "list", "second")
@example(250, "dict", "kw")
@settings(max_examples=60, deadline=None)
def test_decimal_before_deep_container(n, kind, where):
    """search(Decimal(1), <deep>) must report the Decimal, not RecursionError."""
    v = nest(n, kind)
    if where == "first":
        same("search", Decimal(1), v)
    elif where == "second":
        same("search", "a", Decimal(1), v)
    else:
        same("search", Decimal(1), "a", flags=v)


@given(depth, kinds)
@example(300, "list")
@example(250, "list")
@settings(max_examples=40, deadline=None)
def test_deep_container_before_decimal(n, kind):
    """The deep container comes first: CPython raises RecursionError only past its real limit."""
    same("search", "a", "a", nest(n, kind), Decimal(1))
    same("sub", "a", "b", "a", count=nest(n, kind), flags=Decimal(1))
