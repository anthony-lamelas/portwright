# Conformance report: pc_format

**Experimental, unofficial Rust port. Not affiliated with or endorsed by the original authors, and not actively maintained.**

- **Original:** https://github.com/anthony-lamelas/portwright/tree/main/pycompat/shims/pc_format at commit `b41309b8ee1ea69fcdb3acd63510d696382e47bf`

- **Result:** matched on what was tested

- **What the original is:** CPython 3.12.14's built-in formatting and number behavior, called through a shim of one-line functions


This report shows the port matched the original on the inputs tested. It doesn't prove the two are equivalent.

## Changes since this report (by hand, not a new run)

- **Sets holding a NaN** (harness issue #78). A NaN's hash is its memory address, so a set or frozenset of more than one item where an item is, or holds, a NaN was ordered differently from run to run. The shim and the core (`value.rs`, `hash_by_address`) now refuse these with a fixed `TypeError`, and the checks' `REFUSED` values include them.
- **Re-checked** against the current shim (CPython 3.12.3 on the host, seed 20261116, 100 examples): no differences across 22,308 calls to 13 APIs; 12 calls not compared because the original timed out or crashed. All 262 shim tests pass in the toolchain image (CPython 3.12.14). The gate table and figures below predate this change.

## Gates
| Gate | Result | Detail |
| --- | --- | --- |
| canaries | pass | caught 41/42 hand-written planted bugs (98%, minimum 85%) |
| cleanup | pass | Rust lines went from 7364 to 7376 (5% growth allowed) |
| coverage | pass | checks reach 100.0% of the original (minimum 80%) |
| fuzz | pass | no crashes in 13 targets, 120s each |
| holdout_tests | pass | 252/252 upstream tests pass |
| main_build | pass | the Python package builds |
| main_differential | pass | no differences for all APIs across 28852 calls; 14 not compared because the original timed out or crashed |
| main_integrity | pass | no integrity problems |
| main_license | pass | credit is present and all dependency licenses are allowed |
| main_rust_quality | pass | rustfmt, Clippy, and Rust tests pass |
| main_upstream_tests | pass | 226/226 upstream tests pass |
| performance-1 | pass | 10 faster, 0 slower (noise threshold 5%) |
| performance-2 | pass | 3 faster, 0 slower (noise threshold 5%) |
| performance-3 | FAIL | 1 faster, 1 slower (noise threshold 5%) |
| rust_coverage | pass | checks reach 93.0% of Rust lines and 89.8% of functions |
| rust_mutation | pass | checks caught 26/28 Rust mutants (93%, minimum 60%); 12 not viable |
| self_comparison | pass | no differences across 25256 calls to 13 APIs |
| self_comparison_after_final_review | pass | no differences across 28852 calls to 13 APIs; 14 calls not compared because the original timed out or crashed |
| self_comparison_after_review | pass | no differences across 26257 calls to 13 APIs; 14 calls not compared because the original timed out or crashed |
| upstream_baseline | pass | 252/252 upstream tests pass |

## Bugs the checks didn't catch
Planted bugs that went unnoticed are blind spots in this report.

**Hand-written planted bugs** (the original's logic is C, so there was no Python to plant bugs in):
- 37-truediv-naive: big int / int done in floats

**In the Rust:**
- `crates/pc-format-port-core/src/format_spec/number.rs:17` in `None`: >>
- `crates/pc-format-port-core/src/numbers.rs:926` in `float_from_string_inner`: *

## Upstream tests
6 test functions were held back from agents and run only by the harness.

**Failing tests that use private internals or mocks** (reported, not blocking):
None.

## Approved differences
None.

## Differences agents proposed (not approved unless listed above)
None.

## Likely bugs in the original
The port reproduces these on purpose.

None reported.

## Speed through Python
Median nanoseconds per call on one machine. Below 1.00x means the port is slower.

| Benchmark | Original | Port | Speedup |
| --- | --- | --- | --- |
| divmod_floats | 736 | 227 | 3.25x |
| float_repr_shortest | 1,063 | 210 | 5.06x |
| format_float_fixed | 1,048 | 318 | 3.29x |
| format_int_hex_padded | 937 | 290 | 3.23x |
| int_str_big | 634 | 372 | 1.71x |
| parse_float_text | 792 | 234 | 3.39x |
| parse_int_hex | 766 | 332 | 2.31x |
| percent_mixed | 2,926 | 1,030 | 2.84x |
| round_half_even | 936 | 171 | 5.46x |
| str_format_template | 5,720 | 941 | 6.08x |

## Native Rust speed
| Benchmark | Median ns |
| --- | --- |
| divmod_floats | 42 |
| float_repr_shortest | 56 |
| format float .3e | 156 |
| format float 08.3f | 118 |
| format int +d | 61 |
| format int _10 | 64 |
| format int empty | 26 |
| format str _12 | 42 |
| format_float_fixed | 138 |
| format_int_hex_padded | 88 |
| int_str_big | 99 |
| parse_float_text | 61 |
| parse_int 0xdead_beef_cafe_babe_0123 | 242 |
| parse_int 0xdeadbeefcafebabe | 63 |
| parse_int 12345 | 37 |
| parse_int deadbeefcafebabe | 64 |
| parse_int_hex | 68 |
| percent %(h)x | 124 |
| percent %(label)-10s | 129 |
| percent %(n)+d | 132 |
| percent %(q)r | 122 |
| percent %(v)08.3f | 173 |
| percent abc | 68 |
| percent_mixed | 467 |
| round_half_even | 21 |
| str_format abc | 51 |
| str_format {0[0]} | 106 |
| str_format {name!r__12} | 153 |
| str_format_template | 495 |

## Final design review (advisory, by a reviewer agent)
*This review's behavior findings were turned into checks, and the port already passed them. The checks reproduced none of the findings below, so any that are real still stand.*

# Rust review: /work/port (pc-format-port-core + pc-format-port-py), second pass

This is a library port of `pc_format`, a hand port of CPython 3.12's formatting and number code (about 9.2k lines). The core has no `unsafe`, and the PyO3 layer is thin. I reviewed it as a library. **Verdict: good. I found no must-fix issues.** Every finding from the previous review is fixed: guard-before-bind order, the depth cap in the core, private modules, `rust-version`, and the `round` dead path. The two `unsafe` blocks have accurate SAFETY comments.

Tool results:
- `cargo fmt --check`: clean.
- Default clippy: clean.
- `cargo test`: 38 pass.
- `clippy::pedantic`: about 120 warnings, mostly `doc_markdown` (53), `single_match_else` (9), and `needless_pass_by_value` (6). None matter.

Differential run: I built the wheel and ran 3 random seeds × 56k calls across all 13 APIs, covering edge numbers, specs, `%` templates, `str.format` templates, and parse inputs, under a 1 GiB `RLIMIT_AS`. Port and upstream output were **identical**. The only divergence I hit is the documented one: `round_number(int, -10**20)` hangs in CPython but returns 0 in the port (`arith.rs:383`).

Gaming check: I found no special-casing of inputs. The magic constants (`78913>>18`, `NDIGITS_MAX/MIN`, `CPYTHON_BUDGET`, `MAX_DEPTH=900`) each cite the CPython or C behavior they reproduce.

## Must fix
None.

## Should fix
### 1. `decisions.md` describes code that isn't there (`/work/state/decisions.md`, "performance pass 3")
**Problem:** Two claims don't match the code:
- `str_format_api` "formats first and runs `refuse_attribute_fields` only if formatting fails". The code still refuses first (`templates.rs:35`).
- `bind_n` "runs the builtin guard only when `check_missing` fails". The code always guards (`pc-format-py/src/lib.rs:401-403`).

The code is the safe version, and both runs match upstream. But the next session will trust the notes and may "restore" an optimization that was apparently dropped.
**Fix:** correct the notes to match the code, or record in `tried.md` why the change was dropped.

### 2. Silent fallbacks on "impossible" conversions (≈130 sites, worst in `format_spec/number.rs`, `numbers/decimal.rs`, `percent.rs:97,104`)
**Rule:** M-PANIC-ON-BUG (Microsoft Pragmatic Rust); don't mask invariant violations.
**Problem:** Patterns like `i64::try_from(len).unwrap_or(0)` (`number.rs:541,545`), `D::try_from(u).unwrap_or_default()` (`percent.rs:97,104`) and `u8::try_from(v % 10).unwrap_or(0)` keep clippy quiet. But if an invariant ever breaks, the result is a wrong digit or a NUL in the output, not a test failure. The `.ok()?` sites in the fast paths are fine, because they fall back to the slow path.
**Fix:** for values that are provably in range, add a `debug_assert!` or use a small helper so fuzzing catches a regression:
```rust
fn digit(v: u64) -> u8 { debug_assert!(v < 10); b'0' + (v as u8) } // v % 10 < 10
```
For the `Units` widen invariant in `percent.rs`, add `debug_assert!(D::try_from(u).is_ok())`.

### 3. Test-only twins of production parsers (`numbers.rs:128,815,996`)
**Problem:** The `#[cfg(test)]` functions `str_repr`, `parse_int_str` and `parse_float_str` build their error message differently from the production `*_code_points` paths. For example, `parse_int_str` calls `str_repr(text)` with no repr length limit. Tests that use them don't exercise the code Python calls.
**Fix:** in tests, call `parse_int_code_points` and `parse_float_code_points` on `PyStr::from(text).code_points()`, then delete the twins.

## Consider
### 4. Guard runs twice per call (`pc-format-py/src/lib.rs:314,402` and every `*_api`)
The binding runs `check_builtin` and then each core API runs `check_builtins` again. For big containers that walks every element twice. This is cheap next to conversion and safe, so it's fine to keep. If you keep it, update `bind_n`'s doc comment to say so.

### 5. `PyStr::from_code_points` doesn't validate (`value.rs:25`)
The doc says each code point must be `<= 0x10FFFF`, and the binding's SAFETY comment (`py/lib.rs:171`) relies on that, but nothing enforces it. This is not UB: CPython's `PyUnicode_New` rejects such input with `SystemError`. Still, a Rust caller can build an invalid `PyStr`.
**Fix:** add `debug_assert!(cps.iter().all(|&c| c <= 0x10FFFF))`, or weaken the SAFETY wording to "CPython validates".

### 6. Public API polish (`value.rs`, `lib.rs:27`)
- `Value` is a public enum without `#[non_exhaustive]`, unlike `ExcKind` (C-STRUCT-PRIVATE / API evolution).
- `decimal_digit_value` is re-exported only for the fuzz crate. Prefer `#[doc(hidden)]`.
- `Display for PyError` prints a `KeyError` as `KeyError: Str("a")` (the Debug format). Python would show `KeyError: 'a'`, which you could get with `repr::value_repr`.
- The previous review's #8 still applies: typed wrappers such as `float_repr(f64) -> String` would serve Rust users better than an all-`&Value` API.

### 7. No `[lints]` table
The workspace and crates have no `[lints]` table. Adding the BeautifulRust `lints.toml` (https://github.com/morganlinton/BeautifulRust) would flag `cast_possible_truncation` and `doc_markdown` mechanically, and would push the #2-style fallbacks toward explicit, commented casts.

## What's already good
- Behavior matched CPython exactly on 168k randomized calls, including error class and message, surrogates, MemoryError budgets, and the `%c` and `{:c}` range limits.
- The fast paths (`fixed_digits_fast`, `significant_digits_fast`) use exact integer arithmetic with correct ties-to-even, fall back to the slow path whenever they don't apply, and are cross-checked against the slow path in unit tests.
- The binding stays strictly a converter. Error order (guard first, then arity, then missing) follows CPython's `initialize_locals`, and singular/plural wording matches.
- Allocations are fallible (`try_vec`, `try_reserve`) and recursion is capped in the core (`value.rs:197`), so untrusted input can't abort the process.


## Optimization changes rejected by the harness
- performance-3: 1 faster, 1 slower (noise threshold 5%)

## Usage
Cost is Claude Code's client-side estimate of the API-equivalent price, not a bill.

| Role | Sessions | Output tokens | Estimated cost |
| --- | --- | --- | --- |
| cleanup | 1 | 11,623 | $0.81 |
| debugger | 2 | 28,416 | $1.43 |
| performance | 3 | 145,439 | $11.73 |
| planner | 1 | 10,369 | $0.84 |
| porter | 5 | 33,121 | $2.11 |
| reviewer | 2 | 28,361 | $2.40 |
| test_writer | 7 | 81,057 | $5.42 |

**Total estimate:** $24.75
