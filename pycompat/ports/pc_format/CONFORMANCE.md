# Conformance report: pc_format

**Experimental, unofficial Rust port. Not affiliated with or endorsed by the original authors, and not actively maintained.**

- **Original:** https://github.com/anthony-lamelas/portwright/tree/main/pycompat/shims/pc_format at commit `bd9ea59b374c5c192d3d322acb093025c50dd827`

- **Result:** matched on what was tested

- **What the original is:** CPython 3.12.14's built-in formatting and number behavior for core built-in types, called through a shim of one-line functions


This report shows the port matched the original on the inputs tested. It doesn't prove the two are equivalent.

## Gates
| Gate | Result | Detail |
| --- | --- | --- |
| canaries | pass | caught 38/42 hand-written planted bugs (90%, minimum 85%) |
| cleanup | pass | Rust lines went from 8309 to 8306 (5% growth allowed) |
| coverage | pass | checks reach 93.3% of the original (minimum 80%) |
| fuzz | pass | no crashes in 13 targets, 120s each; format_value reached the 2 GB memory limit, which needs a person to check |
| holdout_tests | pass | 222/222 upstream tests pass |
| main_build | pass | the Python package builds |
| main_differential | pass | no differences for all APIs across 22574 calls; 10 not compared because the original timed out or crashed |
| main_integrity | pass | no integrity problems |
| main_license | pass | credit is present and all dependency licenses are allowed |
| main_rust_quality | pass | rustfmt, Clippy, and Rust tests pass |
| main_upstream_tests | pass | 212/212 upstream tests pass |
| performance-1 | pass | 9 faster, 0 slower (noise threshold 5%); 1 benchmarks don't run and were ignored: percent_format_row |
| performance-2 | pass | 6 faster, 0 slower (noise threshold 5%); 1 benchmarks don't run and were ignored: percent_format_row |
| performance-3 | pass | 7 faster, 0 slower (noise threshold 5%); 1 benchmarks don't run and were ignored: percent_format_row |
| rust_coverage | pass | checks reach 90.5% of Rust lines and 88.8% of functions |
| rust_mutation | pass | checks caught 14/20 Rust mutants (70%, minimum 60%); 20 not viable |
| self_comparison | pass | no differences across 16002 calls to 13 APIs; 5 calls not compared because the original timed out |
| self_comparison_after_review | pass | no differences across 22573 calls to 13 APIs; 10 calls not compared because the original timed out or crashed |
| upstream_baseline | pass | 222/222 upstream tests pass |

## Bugs the checks didn't catch
Planted bugs that went unnoticed are blind spots in this report.

**Hand-written planted bugs** (the original's logic is C, so there was no Python to plant bugs in):
- 32-fmt-nested: nested '{:{}}' width ignored
- 33-fmt-message: auto/manual numbering message changed
- 37-truediv-naive: big int / int done in floats
- 41-repr-nan-sign: repr(-nan) gives '-nan'

**In the Rust:**
- `crates/pc-format-port-core/src/templates/object.rs:109` in `int_equals`: !=
- `crates/pc-format-port-core/src/numbers.rs:174` in `bytes_repr_flavored`: 
- `crates/pc-format-port-core/src/numbers.rs:232` in `layout_repr_digits`: <=
- `crates/pc-format-port-core/src/percent.rs:706` in `pad`: >=
- `crates/pc-format-port-core/src/arith.rs:78` in `float_div_mod`: -=
- `crates/pc-format-port-core/src/numbers.rs:175` in `bytes_repr_flavored`: &&

## Upstream tests
5 test functions were held back from agents and run only by the harness.

**Failing tests that use private internals or mocks** (reported, not blocking):
None.

## Approved differences
None.

## Differences agents proposed (not approved unless listed above)
- {"signature": "round_number(int, ndigits) with -ndigits >~ 1.5e7", "reason": "CPython computes 10**-ndigits and passes 10s (differential outcome 'timeout'); the fuzz gate's -timeout=10 flags the same behavior in the port as a crash. No implementation satisfies both. Currently the port matches CPython (slow); the fast alternative (0 / MemoryError) is in /work/out/round_int_fast_alternative.rs.txt."}
- {"signature": "round_number(int n, ndigits) with -ndigits > n.bit_length()+1 (huge)", "reason": "Per person decision: port returns 0 immediately; CPython builds 10**-ndigits and hangs/MemoryErrors. Not compared by the differential (CPython times out)."}

## Likely bugs in the original
The port reproduces these on purpose.

- round(n, -k) for an int computes 10**k before rounding, so round(0, -2**63) hangs/exhausts memory instead of
  returning 0 (CPython long_round). Reproduced in arith::round_int.
# percent task
- `'%.2147483647e' % 1.5 == '2e+00'` (and `'%#.2147483647E' % -1234.5 == '-1.E+03'`): format_float_short does `precision++` in a C int,
  INT_MAX wraps to INT_MIN and dtoa treats it as 1 digit (same output as precision 0). Reproduced in percent.rs.
  format(1.5, '.2147483647e') has the same quirk in CPython ('2e+00'), but the port's format_value raises MemoryError: the
  format_value task should map precision INT_MAX to 0 for 'e'/'E' the same way.
- `'%.N d'` with N > INT_MAX-3 raises OverflowError('precision too large') before the int is converted (wins over the 4300-digit limit).
- round(n, -k) for an int computes 10**k before rounding (CPython long_round), so round(0, -2**63) or -2**934 hangs
  and eventually raises MemoryError instead of returning 0. The port reproduces this (arith::round_int).
# Harness/check note (not an upstream bug)
- checks/benchmarks.json `percent_format_row` calls percent_format(template, [list]). The original raises TypeError
  ("not enough arguments for format string": a list is one argument, not an argument tuple), and so does the port.
  gates/bench.py `performance_gate` counts any non-"benchmark" result as a regression ("benchmark didn't run"), so the
  performance gate can't pass for ANY change until this benchmark passes a tuple (or the gate skips benchmarks
  that raise on both sides). I didn't work around it: making the call succeed would break parity.


## Speed through Python
Median nanoseconds per call on one machine. Below 1.00x means the port is slower.

| Benchmark | Original | Port | Speedup |
| --- | --- | --- | --- |
| div_mod_floats | 589 | 183 | 3.22x |
| float_repr_shortest | 670 | 174 | 3.86x |
| format_float_fixed | 785 | 289 | 2.72x |
| format_int_hex_padded | 733 | 261 | 2.80x |
| int_str_big | 511 | 394 | 1.30x |
| parse_float_text | 594 | 195 | 3.05x |
| parse_int_big | 554 | 357 | 1.55x |
| percent_format_row | n/a | n/a | n/a |
| round_float_digits | 726 | 126 | 5.77x |
| str_format_template | 3,987 | 928 | 4.29x |

## Native Rust speed
| Benchmark | Median ns |
| --- | --- |
| div_mod_floats | 39 |
| float_repr_shortest | 56 |
| format_float_fixed | 136 |
| format_int_hex_padded | 87 |
| int_str_big | 143 |
| parse_float_text | 69 |
| parse_int_big | 109 |
| percent_format_row | 566 |
| round_float_digits | 19 |
| str_format_template | 466 |

## Final design review (advisory, by a reviewer agent)
# Rust review: /work/port (pc-format-port-core + pc-format-port-py), session 2

This is a library: the core crate ports CPython 3.12's format, %, str.format, round, parse and divide built-ins, and the PyO3 crate is a thin binding. It's in good shape. `cargo fmt --check` passes, default clippy is clean, `cargo test` passes 30 tests, and pedantic clippy gives about 120 warnings, mostly doc backticks and literal separators. I found no gaming: no input-specific branches and no hard-coded answers outside the generated attribute table (#5). I ran my own differential against CPython 3.12.14 through the built `.so`. It made 340k random calls over all 13 APIs and found **0 mismatches**. The person's round_number decision is implemented correctly (`arith.rs:381-397`). `round(12345, -2**63)` and `round(-10**50, -2**70)` return 0 in microseconds, and values on the `5·10^(k-1)±2` boundary match CPython. The most important fix is the guard order with keyword arguments (#1). It's the only behavior difference I could reproduce.

## Must fix
### 1. Builtin guard runs in parameter order, not call order, for keyword calls (`py/src/lib.rs:305-317, 374-421`)
**Rule:** Parity with the spec. Upstream `_builtins_only` checks `(*args, *kwargs.values())`, so keyword arguments are checked in the order the caller wrote them.
**Problem:** Confirmed by running both versions. `parse_int(base=Decimal(1), text=Fraction(1))` gives `...not Decimal` upstream and `...not Fraction` in the port. The same happens with `round_number(ndigits=.., x=..)` and `str_format(kwargs=.., template=..)`. A second difference: `parse_int(**{1: 2})` gives "keywords must be strings" upstream and "'int' object is not an instance of 'str'" in the port (`lib.rs:341`, the `cast::<PyString>()?`).
**Fix:** `py/src/lib.rs` is frozen, so this needs a `shared_change`. In `bind_args`, record the call order (positional, then `kw.iter()` order) and convert values in that order before binding them. For round_number, take `*pos, **kw` and go through `bind_args` too. Map a non-str key to `PyTypeError("keywords must be strings")`.

## Should fix
### 2. Public parse helpers panic or loop forever on an out-of-range `base` (`numbers.rs:797, 805, 832`; loop at `718-742`)
**Rule:** M-PANIC-ON-BUG / C-VALIDATE. A `pub fn` must validate its inputs, or encode them in its type.
**Problem:** Reproduced from Rust: `parse_int_str("10", 37)` panics inside `BigInt::parse_bytes`. `parse_int_str("10", 1000)` hangs, because `ascii_digit_value` returns its sentinel 37 for the NUL terminator, `37 < base`, and the digit loop never stops. `parse_int` (the `_api` path) validates the base first, so Python can't reach this, but any Rust caller can.
**Fix:** Make these `pub(crate)`, or validate at the entry point: `if base == 1 || base > 36 { return Err(PyError::value_error("int() base must be >= 2 and <= 36, or 0")); }`. A `Radix` newtype would be cleaner still.
### 3. `int_magnitude_digits` panics for a base outside 2..=36 (`format_spec/number.rs:349`)
**Problem:** `to_str_radix` asserts on the radix. I confirmed panics for bases 1 and 37, even though the doc promises only 2/8/10/16. **Fix:** Make it `pub(crate)`, or return a `ValueError` (or take an enum `Base { Bin, Oct, Dec, Hex }`).
### 4. `double_to_string` returns `""` on allocation failure (`format_spec/number.rs:608`)
**Rule:** Errors must not be silently swallowed (Effective Rust item 4, C-GOOD-ERR).
**Problem:** A public function that returns wrong output silently when it fails. No caller in the crate uses it any more (percent.rs uses `try_double_to_string`). **Fix:** Delete it and rename `try_double_to_string` to `double_to_string -> PcResult<String>`.
### 5. The generator for `templates/attrs.rs` is gone (`templates/attrs.rs:1`)
**Problem:** Line 1 says it was generated by `str_format_gen_attrs.py` "kept in /work/out", but `/work/out` is empty this run. Nobody can regenerate these 2,736 lines of CPython-3.12-specific data (`__flags__: 528642`, `__basicsize__`...) for 3.13. **Fix:** Ask, through a `requests/` entry, for a non-shipped `tools/` directory, or regenerate the script and keep it with the run notes. Also, `Node.key` is a string like `"inst:tuple"` that every lookup binary-searches, and `placeholder` and `opaque_type` (`templates/object.rs:207-232`) parse it back apart. A generated `enum NodeId` would catch typos at compile time.
### 6. Deep `Value` nesting aborts the process (`value.rs:200`, plus the recursive `Drop`)
**Problem:** Confirmed: a `Value::List` nested 200k deep built in Rust overflows the stack in `check_builtin` (SIGABRT, which can't be caught). Dropping such a value also recurses. The binding caps depth at 900, but the core doesn't. **Fix:** Add a depth parameter to `check_builtin` and return `RecursionError` past 900. That covers every API because they all call it first. value.rs is frozen, so this needs a `shared_change`. Session 1 also reported it and it's still open.

## Consider
### 7. `index(other).map(|_| Value::None)` (`arith.rs:441`)
This works only because `index` rejects everything the match hasn't already handled. If `index` ever accepted more types, `round(1.5, x)` would silently return `None`. Instead, convert first with `let nd = index(nd)?;` and then clamp it with `nd.to_i64().unwrap_or(if nd.is_negative() { i64::MIN } else { i64::MAX })`. That removes the fake `Value::None` arm.
### 8. Repr helpers live in `numbers.rs` (`numbers.rs:130-240`)
`str_repr`, `bytes_repr` and `bytearray_repr` belong in `repr.rs`, which already exists. Moving them would make `numbers.rs` (1,362 lines) about parsing and number text only.
### 9. Memory budget policy is hidden (`format_spec/number.rs:17`)
`CPYTHON_BUDGET` is the harness's RLIMIT_AS, built into a library, so a Rust caller gets `MemoryError` for `format(1, "2000000000")` on a machine that could allocate it. The differential needs it, so keep it, but make it a documented `pub const` so Rust users can see the policy.
### 10. Linear attribute scan of a sorted table (`templates/object.rs:190`)
`n.attrs` is documented as sorted, but `iter().find` scans it linearly. Use `binary_search_by` with a code-point comparison. This is a cold path, so it's low priority.
### 11. Unchecked `PyStr` invariant (`value.rs:25`)
`from_code_points(vec![0xFFFF_FFFF])` is accepted, and `repr` then prints `'\Uffffffff'`. It doesn't panic, which is good. A `debug_assert!` or a `try_` constructor would make the invariant real (C-VALIDATE). value.rs is frozen.
### 12. Pedantic lint noise
`doc_markdown` (48) and `unreadable_literal` (25) are mechanical. The 6 `needless_pass_by_value` hits are test helpers or the frozen binding. The `as` casts flagged at `arith.rs:258`, `numbers.rs:617/627` and `percent.rs:96` are all bounded and commented.

## What's already good
- The core is `#![forbid(unsafe_code)]`. The binding's one `unsafe` block has a correct `// SAFETY:` comment. `PyError` implements `Display` and `Error`, and `ExcKind` is `#[non_exhaustive]`.
- Numerics are exact. `int_true_divide` follows `long_true_divide`, `int_to_float` uses a sticky bit, `double_round` is correctly rounded, and there's no signed `%` or `/`. Release builds keep `overflow-checks`.
- The Ryu and `decimal.rs` fast paths fall back to the exact path when they hit a tie, and tests check them against the reference path.
- Big buffers go through `try_reserve`, so no width or precision can cause an infallible huge allocation.

## Lints
There's no `[lints]` table. The BeautifulRust config (https://github.com/morganlinton/BeautifulRust, `lints/lints.toml` and `lints/clippy.toml`) would flag #12 and the `too_many_lines` functions (`format_spec/number.rs:760`, `numbers/ryu.rs:103`, `templates.rs:504`) automatically.


## Optimization changes rejected by the harness
- performance-1: 9 faster, 1 slower (noise threshold 5%)

## Usage
Cost is Claude Code's client-side estimate of the API-equivalent price, not a bill.

| Role | Sessions | Output tokens | Estimated cost |
| --- | --- | --- | --- |
| cleanup | 4 | 23,539 | $2.43 |
| debugger | 5 | 57,480 | $3.27 |
| performance | 4 | 356,446 | $24.40 |
| planner | 1 | 29,766 | $2.18 |
| porter | 5 | 137,156 | $6.60 |
| reviewer | 2 | 12,617 | $2.03 |
| test_writer | 5 | 16,539 | $5.24 |

**Total estimate:** $46.15
