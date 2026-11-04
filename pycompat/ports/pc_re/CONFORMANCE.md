# Conformance report: pc_re

**Experimental, unofficial Rust port. Not affiliated with or endorsed by the original authors, and not actively maintained.**

- **Original:** https://github.com/anthony-lamelas/portwright/tree/main/pycompat/shims/pc_re at commit `c08703b9fd40279ec394d510387f30b47e832d3f`

- **Result:** matched on what was tested

- **What the original is:** CPython 3.12.14's re module (the sre engine) for str patterns and core built-in types, called through a shim of one-line functions


This report shows the port matched the original on the inputs tested. It doesn't prove the two are equivalent.

## Gates
| Gate | Result | Detail |
| --- | --- | --- |
| canaries | pass | caught 39/39 hand-written planted bugs (100%, minimum 85%) |
| cleanup | pass | Rust lines went from 49620 to 49175 (5% growth allowed) |
| coverage | pass | checks reach 100.0% of the original (minimum 80%) |
| fuzz | pass | no crashes in 7 targets, 120s each |
| holdout_tests | pass | 195/195 upstream tests pass |
| main_build | pass | the Python package builds |
| main_differential | pass | no differences for all APIs across 68641 calls; 1 not compared because the original timed out or crashed |
| main_integrity | pass | no integrity problems |
| main_license | pass | credit is present and all dependency licenses are allowed |
| main_rust_quality | pass | rustfmt, Clippy, and Rust tests pass |
| main_upstream_tests | pass | 193/193 upstream tests pass |
| performance-1 | pass | 8 faster, 0 slower (noise threshold 5%) |
| performance-2 | pass | 3 faster, 0 slower (noise threshold 5%) |
| performance-3 | pass | 8 faster, 0 slower (noise threshold 5%) |
| rust_coverage | pass | checks reach 79.2% of Rust lines and 78.1% of functions |
| rust_mutation | pass | checks caught 29/35 Rust mutants (83%, minimum 60%); 5 not viable |
| self_comparison | pass | no differences across 67169 calls to 43 APIs |
| self_comparison_after_final_review | pass | no differences across 72477 calls to 43 APIs |
| self_comparison_after_review | pass | no differences across 72134 calls to 43 APIs |
| upstream_baseline | pass | 195/195 upstream tests pass |

## Bugs the checks didn't catch
Planted bugs that went unnoticed are blind spots in this report.

**Hand-written planted bugs** (the original's logic is C, so there was no Python to plant bugs in):
None.

**In the Rust:**
- `crates/pc-re-port-core/src/engine.rs:436` in `State<'s>::at`: &&
- `crates/pc-re-port-core/src/pyrepr.rs:22` in `int_text`: +
- `crates/pc-re-port-core/src/pyrepr.rs:140` in `bytes_repr`: !=
- `crates/pc-re-port-core/src/parser.rs:20` in `None`: ^
- `crates/pc-re-port-core/src/template.rs:208` in `Template::parse`: >=
- `crates/pc-re-port-core/src/api.rs:29` in `escape_text`: <=

## Upstream tests
2 test functions were held back from agents and run only by the harness.

**Failing tests that use private internals or mocks** (reported, not blocking):
None.

## Approved differences
- compile outcome.stdout
- search outcome.stdout
- match outcome.stdout
- fullmatch outcome.stdout
- findall outcome.stdout
- finditer outcome.stdout
- sub outcome.stdout
- subn outcome.stdout
- split outcome.stdout

## Differences agents proposed (not approved unless listed above)
- {"signature": "compile(pattern, flags) with flags & re.DEBUG (128): stdout", "reason": "CPython prints the parse dump and dis() to sys.stdout. The strict bindings can't import sys, call print or write to sys.stdout, and Rust writes to fd 1 aren't captured by redirect_stdout. The core produces the exact text (Pattern::compile_traced with a debug buffer, verified equal to CPython on ~7000 patterns), so enabling it is one call if a write-to-stdout path is ever allowed. Everything else (return value, errors, no caching of DEBUG patterns) matches."}
- {"signature": "compile/search/... with flags & re.DEBUG (128): stdout output", "reason": "CPython prints the parse tree and code dump to sys.stdout, and the harness compares captured sys.stdout. Rust can't reach sys.stdout without calling into Python objects (sys import, getattr, or call), which the strict-bindings integrity gate forbids, and writing to fd 1 isn't captured. The core still builds the exact dump text (pattern::debug_dump), so allowing a single sys.stdout.write of core-produced text in the bindings would close this difference."}

## Likely bugs in the original
The port reproduces these on purpose.

# compile task
- None found: the getwidth `hi = MAXWIDTH` assignment and `if av[2]:` (empty no-branch of `(?(1)a|)` dropped) are reproduced but have no observable effect.
# substitute
- `parse_template` (3.12): the `warnings.warn("bad character in group name ...")` branch is dead code (guarded by the
  negation of its own enclosing condition). No observable effect; not reproduced.
- `re.compile('\\N{\ud800}')` gives "bad escape \N" instead of "undefined character name": unicodedata.lookup's
  UnicodeEncodeError escapes `except (KeyError, TypeError)`. The port reproduces this.


## Speed through Python
Median nanoseconds per call on one machine. Below 1.00x means the port is slower.

| Benchmark | Original | Port | Speedup |
| --- | --- | --- | --- |
| compile_verbose | 709 | 332 | 2.13x |
| escape_text | 2,692 | 349 | 7.71x |
| findall_numbers | 2,570 | 1,592 | 1.61x |
| fullmatch_iso_date | 1,087 | 628 | 1.73x |
| ignorecase_unicode | 2,007 | 1,550 | 1.29x |
| search_word | 1,397 | 495 | 2.82x |
| split_csvish | 2,365 | 1,196 | 1.98x |
| sub_template | 2,068 | 1,002 | 2.06x |

## Native Rust speed
| Benchmark | Median ns |
| --- | --- |
| compile_verbose | 85 |
| engine_fixed_cost | 61 |
| engine_fixed_repeats_in_branch | 461 |
| engine_fullmatch_iso_date | 252 |
| engine_search_word | 182 |
| escape_text | 102 |
| findall_numbers | 1,049 |
| fullmatch_iso_date | 393 |
| ignorecase_unicode | 808 |
| scan_findall_numbers | 956 |
| scan_ignorecase_unicode | 731 |
| scan_split_csvish | 556 |
| search_word | 294 |
| split_csvish | 618 |
| sub_template | 715 |

## Final design review (advisory, by a reviewer agent)
# Rust review: /work/port (pc-re-port-core + pc-re-port-py)

This is a port of CPython 3.12 `re` for str patterns: parser, compiler, an iterative sre VM and templates in a core library, plus PyO3 bindings that only convert. I held the core to the library standard. **Verdict: good.** `cargo fmt --check` and default clippy are clean. Pedantic clippy reports about 80 doc-backtick and `must_use` nits. `cargo test` passes (27 tests). I built the module and diffed it against upstream on 40k random patterns × search/findall/split/subn/fullmatch (flags, lookarounds, conditionals, possessive and atomic groups, IGNORECASE with K/ß/İ) and ~25 edge cases (nesting 300–495, `x{4294967295}`, 2**70 pos and group, huge flags, DEBUG|I). There were **0 mismatches and no panics**. I found no gaming: no input-specific branches and no hard-coded answers. The items from the earlier review are fixed: the stdout FFI is gone, `State` buffers are reused, fields are `pub(crate)`, and `call.required(i)` exists. The one confirmed behavior difference is #1.

## Must fix
### 1. Nesting limit for arguments: wrong depth, wrong layer, wrong order (`pc-re-port-py/src/lib.rs:22,60-64`)
**Rule:** "the core reproduces the shim's order exactly" (decisions.md); M-NO-LOGIC-IN-FFI-LAYER.
**Problem (confirmed by running both):** the bindings raise `RecursionError` for any container nested deeper than 900.
- (a) **Wrong depth.** CPython's `_check_builtin` succeeds up to ~996 levels: run from a script, it first fails at 997. So `search('a', 'a', <list nested 950>)` gives `TypeError: unhashable type: 'list'` in CPython but `RecursionError` in the port.
- (b) **Wrong order.** The limit is checked during conversion, before the core's `check_builtins`. So `search(Decimal(1), <list nested 1200>)` gives `TypeError ... not Decimal` in CPython but `RecursionError` in the port.

**Fix:** In the bindings, convert with a generous stack guard only, e.g. 10_000 levels, or iteratively. Add `Value::TooDeep`, or record depth in `Value`. Move the CPython limit into core `args::check_builtin(value, depth)` and charge `frames::frame(depth + K)?` for each level, with K calibrated like `FRAME_BASE` (the shim's `checked` wrapper is Python frame 4, and each `_check_builtin` level adds one). Add a `frames::tests` case for it. Then the depth-first order (Decimal before depth) comes for free.

## Should fix
### 2. The subject is copied twice per call, so every call is O(n) (`core/src/api.rs:529`, `module_scan` ~l.600, `value.rs` `Value::Str(PyStr)`)
**Rule:** don't allocate per call in hot paths (Rust Performance Book: "Heap allocations").
**Problem:** `str` becomes `Vec<u32>` in the bindings, then `.into()` copies it again into `Arc<[u32]>`. `search('a', 'ab'*500000)` takes 1.8 ms vs CPython's 0.004 ms, and the per-call cost is about 2 ms on 1 MB. `search('.c', same)` takes 20 ms vs 4 ms, and `findall('a')` 37 ms vs 13 ms.
**Fix:** make `Value::Str` hold `Arc<[u32]>`, built once in `str_to_pystr` with `Arc::from(vec)`, or move the `Vec` out (`Arc::<[u32]>::from(vec)`, still one copy) rather than cloning it from `&[u32]`. Longer term, an ASCII/latin-1 fast path that keeps `&[u8]` would remove the 4× widening.
### 3. The matcher costs ~5× CPython per start position (`core/src/engine.rs`, `search` loop)
After #2, `.c` over 1 MB is still ~18 ms vs 4 ms. This is not a bug. Profile `State::search` → `run` with `cargo bench`: check frame push/pop per position and the `w()` bounds checks in the inner `LITERAL`/`IN` paths. Consider a specialized loop for the `INFO`-literal-prefix case, as `sre` has.
### 4. `alloc_repeat` pushes after the reserve failed (`core/src/engine.rs:755-758`)
**Problem:** when `try_grow` fails, it sets `out_of_memory` and then still calls `push`, which aborts the process on real OOM. The other `try_grow` sites return early.
**Fix:** `if !try_grow(..) { self.out_of_memory = true; return usize::MAX; }`, and let callers check the flag, as at l.975.
### 5. `group_span(0)` underflows (`core/src/engine.rs:733-734`)
**Problem:** `2 * (g - 1)` panics with `overflow-checks = true` if `g == 0`. All three callers guard against this today (`template.rs:347`, `matching.rs:59`, `scan.rs:60`), but nothing enforces it.
**Fix:** `let j = 2 * g.checked_sub(1)?;`.

## Consider
### 6. The Rust-facing API is Python-shaped (`core/src/api.rs`, `pub fn NAME(&[Value], &[(String, Value)])`)
This is deliberate, because argument binding is observable behavior. A Rust user still has no typed entry point. A small facade would fix that: `Pattern::compile(&[u32], u32)`, `Pattern::search(&self, &[u32], Range<usize>) -> Result<Option<Match>>`, and `Pattern::compile` taking `&BigInt` (`pattern.rs:56`) is awkward.
### 7. Too many `pub` modules (`core/src/lib.rs`)
`compiler`, `parser`, `sre`, `casefix`, `debug`, `frames` and `scan` are `pub`, so their internals become the semver surface. Make them `pub(crate)` unless the fuzz crate needs them (the `stdout` stub is already `#[doc(hidden)]`, which is fine given the frozen fuzz helper).
### 8. `ReError` has `pub` fields and no `Display` (`core/src/error.rs:13-20`)
This is a data-carrying error, so public fields are acceptable. Add `impl Display` (using `message()`) and `#[non_exhaustive]` so new fields don't break callers.
### 9. A `Mutex` guards the global pattern cache (`core/src/api.rs:302`)
The cache is correct: the lock isn't held while compiling, poisoning is recovered on purpose, and float/bool key equality matches `re`. Every module-level call takes the lock, though. That is fine under the GIL, but keep it in mind if the core ever runs with the GIL released.
### 10. Pedantic clippy
36 doc-backtick warnings, 44 `must_use` warnings and 5 missing `# Errors` sections. They are mechanical, so a `[lints]` table would keep them fixed.

## What's already good
- The engine is a faithful iterative sre port with a heap frame stack. Every code-word read is bounds-checked (`w`/`wu`), there is no `unwrap`/`expect`/`panic!` outside tests, and the core is `#![forbid(unsafe_code)]`. Now that the FFI stdout path is gone, the bindings are `forbid(unsafe_code)` too.
- The RecursionError emulation is centralized in `frames::frame` and pinned by tests against depths measured on CPython. It isn't a scatter of magic numbers.
- `Error` is a `#[non_exhaustive]` enum mapped 1:1 to Python exceptions, with `Display` and `std::error::Error`. Messages are built only in the core, and the bindings just translate.
- Performance work is safe: `FirstSet`/`AsciiSets` fall back to the full charset check, and per-thread buffer reuse is capped and returned on `Drop`. Small-call overhead beats CPython (compile ×5, small search ×2.7).

## Lints
There is no `[lints]` table. The BeautifulRust config (`lints/lints.toml`, `lints/clippy.toml`, https://github.com/morganlinton/BeautifulRust) would flag #10 and `arithmetic_side_effects` (#5).


## Usage
Cost is Claude Code's client-side estimate of the API-equivalent price, not a bill.

| Role | Sessions | Output tokens | Estimated cost |
| --- | --- | --- | --- |
| cleanup | 1 | 35,559 | $2.33 |
| debugger | 4 | 50,983 | $7.25 |
| performance | 3 | 154,594 | $11.06 |
| planner | 1 | 67,428 | $3.38 |
| porter | 8 | 302,340 | $15.99 |
| reviewer | 2 | 32,825 | $2.72 |
| test_writer | 7 | 99,465 | $6.31 |

**Total estimate:** $49.05
