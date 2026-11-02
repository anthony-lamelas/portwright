---
name: python-to-rust-traps
description: Differences between Python and Rust that silently break ports. Read before writing or reviewing Rust ported from Python.
---

# Python-to-Rust traps

Each of these has broken real ports. The goal is the same behavior at the Python boundary, with idiomatic Rust in the core.

## Types and values

| Python | Default in Rust | Trap |
|---|---|---|
| Inheritance | Enum if closed, trait if open, composition for shared code | PyO3 classes can't inherit from Python-defined classes |
| Object graphs | Owned tree, or an arena with IDs | `Rc` isn't `Send`, and `#[pyclass]` must be `Send + Sync` |
| `None` | `Option` | "Argument missing" and "explicitly None" can differ, so model both if the function distinguishes them |
| Exceptions | `Result` with an error enum | A panic surfaces as `PanicException`, a `BaseException` that `except Exception` won't catch |
| `int` | A chosen width, or `num-bigint` | Python never overflows. Huge arguments raise `OverflowError` on extraction unless you accept big integers |
| `bool` | `bool` | `True` is an `int` in Python, so check `isinstance` order |
| `str` | `String`, with care | Length and slicing count code points. Lone surrogates can't be a Rust `String`; read them with `encode("utf-32-le", "surrogatepass")` when inputs can contain them |
| `dict` | `IndexMap` | `HashMap` reorders output. Python dicts keep insertion order |
| `set` | `HashSet` | Python's set order is effectively arbitrary, so don't depend on it |
| Generators | Iterators | Errors must fire at the same moment. A generator raises on first `next()`, not at the call |
| `**kwargs` and defaults | Options struct in the core, `#[pyo3(signature)]` in the binding | Error messages for bad arguments must still match Python's |

## Operations that look the same

- **`//` and `%`:** Python floors, and Rust truncates. They differ for negatives: use `div_euclid` carefully, or `div_floor` and `mod_floor`.
- **`round`:** Python rounds halves to even. Rust's `f64::round` rounds away from zero.
- **Float to string:** `repr(1e16)` is `1e+16`, and Rust's `{}` prints `10000000000000000`.
- **`str.split()` with no argument:** splits on more whitespace characters than `split_whitespace`, including `\x1c` to `\x1f`.
- **`str.strip(chars)`:** treats `chars` as a set, not a substring.
- **`str.isdigit`, `isalnum`, `lower`, `upper`, `title`:** Unicode rules differ from `char` methods, and `lower()` handles final sigma.
- **`int(s)` and `float(s)`:** accept underscores, whitespace, and non-ASCII digits that Rust's `parse` rejects.
- **Negative indexes and out-of-range slices:** Python clamps, and Rust panics.
- **`repr(str)`:** chooses quotes and escapes by its own rules, which matters in error messages.
- **Recursion:** Python raises `RecursionError`. Rust overflows the stack and aborts the process, so add a depth limit where input controls depth.

## Regular expressions

Rust's `regex` crate has no backreferences or lookaround, so use `fancy-regex` for those. Even then, check:

- `\d`, `\w`, and `\s` are Unicode in Python's `str` patterns, with slightly different tables.
- `$` also matches before a trailing newline in Python, and doesn't in Rust by default. Python's `\Z` is Rust's `\z`.
- Named groups are `(?P<name>...)` in both, while backreferences are `(?P=name)` in Python and `\k<name>` in Rust.
- `re.IGNORECASE` Unicode case folding differs in edge cases.
- Group numbering, empty matches, and capture behavior on backtracking can differ.

## PyO3 boundary

- Use `py.detach` around long pure-Rust work, and call `py.check_signals()` in long loops so Ctrl-C works.
- Runtime borrow errors are new behavior: a callback into the same `#[pyclass]` raises `Already borrowed`. Don't hold a borrow while calling Python.
- Dunder methods aren't automatic: `__repr__`, `__eq__`, `__hash__`, `__iter__`, `__getitem__`, `__len__`, and pickling all need writing.
- Class `__qualname__` and the `__module__` must match the original. Set `#[pyclass(name = "...", module = "...")]`.
- Exceptions: match the base classes with `create_exception!`, and pass the same args tuple.
- `#[pyclass]` can't be generic or have lifetimes. Keep the core generic, and make bindings concrete.
- Subclassable classes need `#[pyclass(subclass)]`. Instance attributes need `dict`, and weak references need `weakref`.

## Crates

- Python's optional imports become Cargo features.
- Mark public error enums `#[non_exhaustive]`.
- Import-time setup becomes explicit initialization or `std::sync::OnceLock`.
- A "matching" crate isn't the same behavior: the same C library through a different API has given different bytes. Test it against the original.
- Release builds wrap on overflow unless `overflow-checks = true`, which the scaffold sets. Keep it.
