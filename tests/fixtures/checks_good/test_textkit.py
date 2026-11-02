"""Input rules for textkit. These choose inputs only; the Python supplies the answers."""

from hypothesis import given
from hypothesis import strategies as st
from hypothesis.stateful import RuleBasedStateMachine, initialize, rule

from portwright.compare import same
from portwright.compare.session import current

TEXT = st.one_of(
    st.text(),
    st.text(alphabet="aB1 -=_é\ud800X"),
    st.text(alphabet="ab ", max_size=12),
)
INTS = st.one_of(st.sampled_from([0, 1, -1]), st.integers(-20, 20), st.integers())


@given(TEXT, st.one_of(st.none(), INTS), st.sampled_from(["-", "_", "--", "", "a"]))
def test_slug(text, max_len, sep):
    same("slug", text, max_len=max_len, sep=sep)


@given(TEXT)
def test_slug_defaults(text):
    same("slug", text)


@given(st.one_of(st.integers(), st.none(), st.binary()))
def test_slug_rejects_other_types(text):
    same("slug", text)


@given(TEXT, INTS)
def test_truncate(text, n):
    same("truncate", text, n)


@given(INTS, INTS)
def test_floor_bucket(n, size):
    same("floor_bucket", n, size)


@given(st.one_of(INTS, st.integers(10**30, 10**60)))
def test_sum_digits(n):
    same("sum_digits", n)


@given(TEXT)
def test_word_count(text):
    same("word_count", text)


@given(st.one_of(TEXT, st.text(alphabet="abX =", max_size=8)))
def test_parse_kv(line):
    same("parse_kv", line)


@given(st.lists(INTS, max_size=12))
def test_normalize_in_place(values):
    same("normalize_in_place", values)


@given(st.lists(INTS, max_size=12), st.integers(-3, 6))
def test_first_chunks(values, n):
    same("first_chunks", values, n)


class StackMachine(RuleBasedStateMachine):
    @initialize(capacity=st.one_of(st.none(), st.integers(0, 3)))
    def create(self, capacity):
        self.stack = same("Stack", capacity)

    @rule(item=st.one_of(st.integers(), st.text(max_size=3), st.none()))
    def push(self, item):
        self.stack.call("push", item)

    @rule()
    def pop(self):
        self.stack.call("pop")

    @rule()
    def peek(self):
        self.stack.call("peek")

    @rule()
    def length(self):
        self.stack.call("__len__")

    @rule()
    def show(self):
        self.stack.call("__repr__")


TestStack = StackMachine.TestCase


def test_exceptions_exist():
    session = current()
    session.get("ParseError")
    session.get("EmptyError")
