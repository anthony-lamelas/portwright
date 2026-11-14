# Conformance report: pc_datetime

**Experimental, unofficial Rust port. Not affiliated with or endorsed by the original authors, and not actively maintained.**

- **Original:** https://github.com/anthony-lamelas/portwright/tree/main/pycompat/shims/pc_datetime at commit `721dca3d1fee2b96bd67e7c139d78f2d86387190`

- **Result:** matched on what was tested

- **What the original is:** CPython 3.12.14's datetime module (dates, times, timedeltas, ISO 8601, strftime with glibc, strptime) on plain tuples, called through a shim of small functions; the process time zone is pinned to UTC


This report shows the port matched the original on the inputs tested. It doesn't prove the two are equivalent.

## Gates
| Gate | Result | Detail |
| --- | --- | --- |
| canaries | pass | caught 37/41 hand-written planted bugs (90%, minimum 85%) |
| cleanup | pass | Rust lines went from 5622 to 5696 (5% growth allowed) |
| coverage | pass | checks reach 100.0% of the original (minimum 80%) |
| fuzz | pass | no crashes in 34 targets, 120s each |
| holdout_tests | pass | 30/30 upstream tests pass |
| main_build | pass | the Python package builds |
| main_differential | pass | no differences for all APIs across 17485 calls |
| main_integrity | pass | no integrity problems |
| main_license | pass | credit is present and all dependency licenses are allowed |
| main_rust_quality | pass | rustfmt, Clippy, and Rust tests pass |
| main_upstream_tests | pass | 27/27 upstream tests pass |
| performance-1 | pass | 8 faster, 0 slower (noise threshold 5%) |
| performance-2 | pass | 8 faster, 0 slower (noise threshold 5%) |
| performance-3 | pass | 8 faster, 0 slower (noise threshold 5%) |
| rust_coverage | pass | checks reach 89.5% of Rust lines and 89.6% of functions |
| rust_mutation | pass | checks caught 25/27 Rust mutants (93%, minimum 60%); 13 not viable |
| self_comparison | pass | no differences across 14214 calls to 34 APIs |
| self_comparison_after_final_review | pass | no differences across 17481 calls to 34 APIs |
| self_comparison_after_review | pass | no differences across 15029 calls to 34 APIs |
| upstream_baseline | pass | 30/30 upstream tests pass |

## Bugs the checks didn't catch
Planted bugs that went unnoticed are blind spots in this report.

**Hand-written planted bugs** (the original's logic is C, so there was no Python to plant bugs in):
- 02-year-zero: year 0 allowed
- 15-fromisocalendar: week 53 accepted for every year
- 16-add-overflow: date overflow wraps instead of raising
- 21-iso-millis-round: milliseconds timespec rounds

**In the Rust:**
- `crates/pc-datetime-port-core/src/strftime.rs:579` in `convert`: 
- `crates/pc-datetime-port-core/src/strptime/text.rs:136` in `push_wtf8`: &

## Upstream tests
3 test functions were held back from agents and run only by the harness.

**Failing tests that use private internals or mocks** (reported, not blocking):
None.

## Approved differences
None.

## Differences agents proposed (not approved unless listed above)
- {"signature": "timezone_name(None)", "reason": "CPython raises AttributeError (None.tzname); frozen PyError has no AttributeError variant, so the port raises NotImplementedError with the identical message until requests/attribute_error.json is applied."}
- {"signature": "datetime_fromtimestamp(ts, off) where glibc gmtime_r overflows (e.g. ts=-2**63, 2**63-1, 10**18)", "reason": "Raises OverflowError('[Errno 75] Value too large for defined data type') instead of OSError, because PyError has no OSError variant (requests/os_error.json). Fixed by a one-line change in timestamp.rs gmtime_overflow() once the variant exists."}

## Likely bugs in the original
The port reproduces these on purpose.

# date
- `date(*d)` with one 4-byte bytes/str arg whose 3rd byte is 1..12 takes CPython's unpickle path and builds an unvalidated date (year 0..65535, day 0..255), e.g. `date_isoformat(["\x00\x00\x01\xff"])` -> '0000-01-255', `date_toordinal(["\x00\x00\x01\x01"])` -> -364, `weekday` -> -1 (C `%`). Reproduced in the port (date.rs `pickle_state`/`Date::from_pickle`).
# isoparse (CPython 3.12 C parsers, reproduced)
- date_fromisoformat doesn't check trailing text for len 10: '20240101ab' -> (2024,1,1); '2024W01123' -> (2024,1,1).
- time_fromisoformat ignores garbage between fraction and offset: '12:30:00.1234567x+01:00' parses; '12:30:00.+01:00' parses too.
- A UTC offset whose seconds part is 0 is UTC even with microseconds: '12:30-00:00:00.5' -> offset 0.
# timestamp
- fromtimestamp: CPython computes `year = tm.tm_year + 1900` in a C int, so years just past INT_MAX wrap: datetime_fromtimestamp(67767976233532800, 0) -> ValueError 'year -2147483648 is out of range'. Reproduced in timestamp.rs.
# strptime (reproduced)
- '%z' with a colon only after the minutes ('+0530:15') passes the regex, then `int(':1')` raises "invalid literal for int() with base 10: ':1'".
- IGNORECASE lets 'ſun'/'frİ' match %a/%b names, then `.lower()` lookup fails: ValueError "'ſun' is not in list".
- A trailing lone '%' in a format stays a literal '%' (no 'stray %' error in 3.12).


## Speed through Python
Median nanoseconds per call on one machine. Below 1.00x means the port is slower.

| Benchmark | Original | Port | Speedup |
| --- | --- | --- | --- |
| datetime_add | 3,601 | 732 | 4.92x |
| fromtimestamp | 1,500 | 414 | 3.62x |
| iso_format_datetime | 3,228 | 902 | 3.58x |
| iso_parse_datetime | 872 | 496 | 1.76x |
| isocalendar | 963 | 325 | 2.96x |
| strftime_mixed | 4,600 | 1,249 | 3.68x |
| strptime_full | 7,505 | 1,475 | 5.09x |
| timedelta_float | 2,066 | 450 | 4.59x |

## Native Rust speed
| Benchmark | Median ns |
| --- | --- |
| datetime_add | 291 |
| fromtimestamp | 155 |
| iso_format_datetime | 385 |
| iso_parse_datetime | 207 |
| isocalendar | 89 |
| strftime_mixed | 848 |
| strptime_full | 983 |
| timedelta_float | 173 |

## Final design review (advisory, by a reviewer agent)
# Rust review: /work/port (pc-datetime-port-core + pc-datetime-port-py)

**Scope:** a library. It has a core crate of `fn(&[Value], &Kwargs) -> PyResult<Value>` shims and a thin PyO3 layer. Edition 2021, no `[lints]` table.
**Tools:** `cargo fmt --check` is clean, default `cargo clippy --all-targets` is clean, and all 28 core tests pass. `clippy::pedantic` gives about 160 warnings, mostly doc backticks and `must_use`.
**Verdict:** the code is good. I found no gaming: the only hard-coded values are in `#[cfg(test)]` blocks. There's no `unsafe` (`forbid(unsafe_code)` in both crates), no `unwrap`/`expect` outside tests, and the layering is clean.
**Person-granted change (AttributeError):** already in place and correct. It's in `error.rs:29,66,86,103` (variant, constructor, `type_name`, `Display`), `convert.rs:208` (maps to `PyAttributeError`) and `timedelta/mod.rs:193`. `timezone_name(None)` matches the original exactly; I checked with both builds.
**Differences:** I confirmed two by running both builds (upstream vs a release cdylib), comparing with `ascii()`. Both are below as Must fix. Neither has changed since the previous review.

## Must fix
### 1. Long strptime formats fail with a regex size-limit error (`strptime/pattern.rs:250`)
**Rule:** match the original on all inputs; don't let internal limits show up as Python errors (M-PANIC-ON-BUG spirit).
**Problem:** `strptime("2024"+"k"*400_000, "%Y"+"k"*400_000)` returns `(2024,1,1,0,0,0,0,None,0)` in Python. Rust raises `ValueError: internal regex error: Compiled regex exceeds size limit of 67108864 bytes.` The cause is `push_literal` (pattern.rs:153): under `(?i)` each literal becomes a Unicode case-fold class (`k` → `[kK\u{212A}]`), so the compiled program grows much faster than the text.
**Fix:** raise the limits (`.size_limit(usize::MAX).dfa_size_limit(1 << 28)`). CPython accepts these inputs at a similar memory cost. Optionally, emit runs of literals as one `(?i:...)` group. Add a regression test with a format holding a 1M-char literal.

### 2. Keyword names with lone surrogates become U+FFFD (`args.rs:7`, `pc-datetime-port-py/src/convert.rs:236-240`)
**Rule:** decisions.md says "never use the lossy form for results", and error messages are results. Stringly-typed keys also break C-NEWTYPE.
**Problem:** `Kwargs = [(String, Value)]`, so `call_args` falls back to `to_string_lossy()`. For `make_date(2024,1,1, **{'\udc80': 1})`, Python reports `unexpected keyword argument '\udc80'` and Rust reports `'�'`. Two different surrogate keys also collapse to the same name in the duplicate and name checks.
**Fix:** this needs a shared change to args.rs and convert.rs. Key on `PyStr`, and use `PyStr::repr()` in binding messages:
```rust
pub type Kwargs = [(PyStr, Value)];
// convert.rs
keywords.push((str_to_core(key)?, to_value(&item)?));
```

## Should fix
### 3. `parse` is 169 lines long (`strptime/mod.rs:199`, clippy `too_many_lines`)
It does three jobs: matching, mapping groups to fields, and ISO/julian resolution. Split it into `fn fields(&groups) -> PyResult<Fields>` and `fn resolve(Fields) -> PyResult<Parsed>`, mirroring `_strptime`. Then the field logic can be tested without regexes.

### 4. `pub enum Default` shadows the prelude's `Default` (`args.rs:11`)
Any `use crate::args::Default` hides `Default::default()`/`#[derive(Default)]` by name, which breaks the naming guidance in the Rust API Guidelines. Rename it to `ParamDefault`.

### 5. Missing `# Errors` docs (clippy `missing_errors_doc`)
`date::as_c_long` (date.rs:44), `as_c_int` (date.rs:62), `check_date_args` (date.rs:198) and `normalize_date` (date.rs:220) have none. Most shims already document their errors; add one line to each of these.

## Consider
### 6. Allocations in hot paths (`strptime/pattern.rs:164`, `strptime/mod.rs:221`)
`out.push_str(&format!("\\x{b:02X}"))` allocates once per byte; use `write!(out, ...)` instead (clippy `format_push_string`). `String::from_utf8_lossy` on each capture can become `std::str::from_utf8`, because the groups are built from ASCII patterns. This is minor, because the compiled regex is cached.

### 7. Thread-local `RefCell<HashMap<PyStr, Rc<Compiled>>>` cache (`strptime/pattern.rs:265`)
It's correct and private, and clearing it at 64 entries mirrors `_strptime._regex_cache`. If the core ever needs to be `Send`/`Sync`, switch to `Arc` plus `Mutex`.

### 8. Exact float equality in half-even rounding (`timedelta/delta.rs:151,236`, `value.rs:145`)
`(whole - leftover).abs() == 0.5` is intentional and exact; it mirrors CPython's `delta_new`. Add a comment or `#[expect(clippy::float_cmp, reason = ...)]` so a later cleanup doesn't "fix" it.

### 9. Pedantic sweep
There are 99 `doc_markdown` warnings, 51 `must_use` candidates on pure getters, two pairs of identical match arms (strftime.rs:543, 559), and one redundant `continue`. Fix them in one pass.

## Layering (OK)
- The recursion limit (`MAX_DEPTH`) is in convert.rs, which is right because it models the Python call stack.
- The UTF-32 surrogate round-trip is documented in decisions.md.
- `to_py_err` is exhaustive with no catch-all arm, so a new `PyError` variant won't compile until it's mapped. The AttributeError addition shows this works.

## What's already good
- A typed core sits under Python-shaped shims. `Date`, `DateTime`, `Time`, `TimeDelta` and `FixedOffset` are `Copy` value types with checked constructors, and conversion happens only at the edge.
- Arithmetic is checked or done in i128/BigInt, and errors are a plain-data `PyError` that implements `Display` and `Error`.
- C quirks are ported structurally and logged in upstream_bugs.md, not patched input by input.
- Parser internals have unit tests (pattern translation, ReError messages).

## Lints
There's no `[lints]` table. BeautifulRust's `lints/lints.toml` and `clippy.toml` (https://github.com/morganlinton/BeautifulRust) would catch items 3, 5, 6 and 9 automatically.


## Usage
Cost is Claude Code's client-side estimate of the API-equivalent price, not a bill.

| Role | Sessions | Output tokens | Estimated cost |
| --- | --- | --- | --- |
| cleanup | 1 | 10,189 | $0.84 |
| debugger | 3 | 38,032 | $2.54 |
| performance | 3 | 125,136 | $8.58 |
| planner | 1 | 49,213 | $2.07 |
| porter | 9 | 230,093 | $11.63 |
| reviewer | 3 | 27,058 | $1.85 |
| test_writer | 7 | 46,336 | $3.99 |

**Total estimate:** $31.49
