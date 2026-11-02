"""A tiny library for testing the harness. Each function holds a known porting trap."""

__all__ = [
    "EmptyError", "ParseError", "Stack", "first_chunks", "floor_bucket",
    "normalize_in_place", "parse_kv", "slug", "sum_digits", "truncate", "word_count",
]


class ParseError(ValueError):
    pass


class EmptyError(Exception):
    pass


def slug(text, max_len=None, sep="-"):
    """Exception text and optional arguments must match."""
    if not isinstance(text, str):
        raise TypeError(f"text must be str, got {type(text).__name__}")
    out = []
    pending = False
    for char in text:
        if char.isascii() and char.isalnum():
            if pending and out:
                out.append(sep)
            out.append(char.lower())
            pending = False
        else:
            pending = True
    result = "".join(out)
    if max_len is not None:
        if max_len < 0:
            raise ValueError("max_len must be non-negative")
        result = result[:max_len].rstrip(sep)
    return result


def truncate(text, n):
    """Slices by code point, and negative numbers count from the end."""
    return text[:n]


def floor_bucket(n, size):
    """Rust division truncates, while Python floors. Integers can also be huge."""
    return n // size, n % size


def sum_digits(n):
    """Python integers never overflow."""
    n = abs(n)
    total = 0
    while n:
        total += n % 10
        n //= 10
    return total


def word_count(text):
    """Dicts keep insertion order."""
    counts = {}
    for word in text.split(" "):
        if word:
            counts[word] = counts.get(word, 0) + 1
    return counts


def parse_kv(line):
    """A custom exception with more than one argument."""
    if "=" not in line:
        raise ParseError("missing '='", line)
    key, _, value = line.partition("=")
    key = key.strip(" ")
    if not key:
        raise ParseError("empty key", line)
    return key, value.strip(" ")


def normalize_in_place(values):
    """Changes its argument, and the caller can see the change."""
    before = len(values)
    values[:] = sorted(set(values))
    return before - len(values)


def first_chunks(values, n):
    """A generator: the error appears on the first read, not at the call."""
    if n <= 0:
        raise ValueError("n must be positive")
    for start in range(0, len(values), n):
        yield values[start:start + n]


class Stack:
    """A class with state, a custom error, and Python protocol methods."""

    def __init__(self, capacity=None):
        self.capacity = capacity
        self._items = []

    def push(self, item):
        if self.capacity is not None and len(self._items) >= self.capacity:
            raise OverflowError("stack full")
        self._items.append(item)

    def pop(self):
        if not self._items:
            raise EmptyError("pop from empty stack")
        return self._items.pop()

    def peek(self):
        return self._items[-1] if self._items else None

    def __len__(self):
        return len(self._items)

    def __repr__(self):
        return f"Stack(size={len(self._items)})"
