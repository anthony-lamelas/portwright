# Conformance report: parse

**Experimental, unofficial Rust port. Not affiliated with or endorsed by the original authors, and not actively maintained.**

- **Original:** https://github.com/r1chardj0n3s/parse at commit `7f07b64089338ee7b7db85a752eb45c4816c2657`

- **Result:** matched on what was tested


This report shows the port matched the original on the inputs tested. It doesn't prove the two are equivalent.

## Gates
| Gate | Result | Detail |
| --- | --- | --- |
| canaries | pass | caught 137/149 planted bugs (92%, minimum 70%) |
| cleanup | pass | Rust lines went from 11330 to 11331 (5% growth allowed) |
| coverage | pass | checks reach 98.1% of the original (minimum 80%) |
| fuzz | pass | no crashes in 11 targets, 120s each |
| holdout_tests | pass | 113/115 upstream tests pass (2 failures use private internals or mocks) |
| main_build | pass | the Python package builds |
| main_differential | pass | no differences for all APIs across 71073 calls |
| main_integrity | pass | no integrity problems |
| main_license | pass | credit is present and all dependency licenses are allowed |
| main_rust_quality | pass | rustfmt, Clippy, and Rust tests pass |
| main_upstream_tests | pass | 78/80 upstream tests pass (2 failures use private internals or mocks) |
| performance-1 | pass | 8 faster, 0 slower (noise threshold 5%) |
| performance-2 | pass | 8 faster, 0 slower (noise threshold 5%) |
| performance-3 | pass | 5 faster, 0 slower (noise threshold 5%) |
| rust_coverage | pass | checks reach 69.3% of Rust lines and 73.8% of functions |
| rust_mutation | pass | checks caught 17/27 Rust mutants (63%, minimum 60%); 13 not viable |
| self_comparison | pass | no differences across 40832 calls to 46 APIs |
| self_comparison_after_review | pass | no differences across 71069 calls to 52 APIs |
| upstream_baseline | pass | 115/115 upstream tests pass |

## Bugs the checks didn't catch
Planted bugs that went unnoticed are blind spots in this report.

**In copies of the Python:**
- `parse/__init__.py:93`: change string
- `parse/__init__.py:150`: change 3
- `parse/__init__.py:150`: change string
- `parse/__init__.py:152`: change string
- `parse/__init__.py:154`: change string
- `parse/__init__.py:162`: change 10
- `parse/__init__.py:163`: change string
- `parse/__init__.py:165`: change string
- `parse/__init__.py:358`: change string
- `parse/__init__.py:425`: flip boolean
- `parse/__init__.py:484`: negate condition
- `parse/__init__.py:514`: return None

**In the Rust:**
- `crates/parse-port-core/src/pyre/parser.rs:1230` in `parse_group`: |
- `crates/parse-port-core/src/format/percent.rs:105` in `percent_format`: <
- `crates/parse-port-core/src/format/percent.rs:91` in `percent_format`: -=
- `crates/parse-port-core/src/convert/mod.rs:51` in `int_convert`: >=
- `crates/parse-port-core/src/pyre/parser.rs:1195` in `parse_group`: 
- `crates/parse-port-core/src/format/percent.rs:90` in `percent_format`: +
- `crates/parse-port-core/src/pyre/charset.rs:54` in `ascii_iscased`: ||
- `crates/parse-port-core/src/pyre/parser.rs:583` in `escape`: ==
- `crates/parse-port-core/src/pyre/vm.rs:249` in `Vm<'p>::at`: !=
- `crates/parse-port-core/src/pyre/parser.rs:1089` in `parse_group`: *

## Upstream tests
33 test functions were held back from agents and run only by the harness.

**Failing tests that use private internals or mocks** (reported, not blocking):
- tests.test_parse::test_flexible_dates_inferred_year_day_of_year
- tests.test_parse::test_flexible_dates_inferred_year_leap_day

## Approved differences
None.

## Differences agents proposed (not approved unless listed above)
- {"signature": "parse.compile(fmt) where a %-style field width (e.g. bracketed name '{a[%-300000000s]}') expands output beyond 2**28 chars", "reason": "CPython allocates gigabytes or raises MemoryError depending on host memory; the frozen core::Error has no MemoryError variant, so the port raises ValueError('width too big') instead of exhausting memory (fuzz OOM)."}
- {"signature": "re.compile(r'\\N{EM DASH}') via user with_pattern patterns", "reason": "No Unicode character-name database is available (no approved crate), so every \\N{...} escape raises re.error 'undefined character name ...' even for valid names."}

## Likely bugs in the original
The port reproduces these on purpose.

# parse: upstream quirks the port must reproduce (exercised by /work/checks)
- Literal text is not escaped for `{`/`}`: format `a{2,3}` becomes regex quantifier and matches "aaa".
- Fill char `|` is not escaped: `{:|<4}` injects regex alternation.
- `{a}{a[b]}` raises TypeError ('str' object does not support item assignment) in _expand_named_fields; `{a[b]}{a}` overwrites the dict.
- `{a_b_}{a[b]}` raises KeyError("duplicated group name ...") from _to_group_name.
- Repeated names compile to backrefs `(?P=x)`, which match case-insensitively by default.
- Invalid regex: parse() raises NotImplementedError (wrapped), search()/findall() raise raw re.error.
- `\d`/`\w`/`\s` are Unicode-aware (Python re); `{:n}` on "١٢" matches then int_convert strips all chars -> ValueError.
- strptime repeated directive (`%B%B`) raises re.error from inside strf_date_convert.
- TooManyFields is unreachable on Python 3.12 (no 100-group limit); only constructed directly.
- date_convert: an all-uppercase tz (e.g. " EST") or an empty tz group is left as a str, so datetime()/time() raise TypeError "tzinfo argument must be None or of a tzinfo subclass, not type 'str'". Reproduced.
- int_convert(0) and int_convert(b <= -36) build the regex "[^]", which raises re.error "unterminated character set at position 0". Reproduced.
- strf_date_convert: year detection is a substring test, so `type='%%y %H'` (literal "%y") skips appending the current year
  and strptime then defaults to 1900; a non-str list `type` gets " %Y" extended into it in place before strptime's TypeError. Reproduced.
- strf_date_convert with `%c` (or `%x`) and no `%Y`/`%y`: `%c` already has group Y, so the appended ` %Y` raises re.error
  "redefinition of group name 'Y' ..."; `%x` alone returns ValueError 'Datetime not a date nor a time?'. Reproduced.
- (parser) `_generate_expression` treats ANY split part that starts with `{` and ends with `}` as a field, including
  *text* that PARSE_RE didn't match: `{:}` becomes `(.+?)`, and `{a b}` becomes group `a b` (a lazy re.error). Reproduced.
- (parser) `wrap % s` interprets `%` inside bracketed names: `{a[%d]}` raises TypeError "%d format: a real number is required, not str",
  and `{a[%%]}` gives group `a_%_`. Reproduced by `format::percent_format`.
- CPython quirk reproduced: whether a deeply nested custom pattern raises `RecursionError` depends on the caller's stack depth and on `re._cache` (a pattern once compiled shallowly keeps working when called deeper). The port models both.


## Speed through Python
Median nanoseconds per call on one machine. Below 1.00x means the port is slower.

| Benchmark | Original | Port | Speedup |
| --- | --- | --- | --- |
| apache_log | 37,849 | 2,412 | 15.69x |
| compile_complex | 23,382 | 159 | 147.27x |
| findall_numbers | 4,650 | 224 | 20.79x |
| iso_datetime | 21,899 | 1,316 | 16.64x |
| numeric_mix | 22,640 | 1,421 | 15.93x |
| search_log | 9,919 | 665 | 14.92x |
| simple_parse | 11,746 | 688 | 17.08x |
| strftime_field | 21,095 | 1,130 | 18.67x |

## Native Rust speed
| Benchmark | Median ns |
| --- | --- |
| apache_log_match | 605 |
| apache_log_new | 4,625 |
| apache_log_new+compile | 4,735 |
| apache_log_new_cached+compile | 50 |
| compile_complex_match | 65 |
| compile_complex_new | 4,897 |
| compile_complex_new+compile | 4,513 |
| compile_complex_new_cached+compile | 42 |
| convert_date_iso | 197 |
| convert_date_th | 213 |
| convert_int | 49 |
| convert_percentage | 56 |
| convert_strf_date | 431 |
| convert_strptime | 424 |
| findall_numbers_search | 60 |
| iso_datetime_match | 283 |
| iso_datetime_new | 1,531 |
| iso_datetime_new+compile | 1,699 |
| iso_datetime_new_cached+compile | 39 |
| numeric_mix_match | 336 |
| numeric_mix_new | 2,090 |
| numeric_mix_new+compile | 2,299 |
| numeric_mix_new_cached+compile | 39 |
| search_log_search | 189 |
| simple_parse_match | 171 |
| simple_parse_new | 1,408 |
| simple_parse_new+compile | 1,578 |
| simple_parse_new_cached+compile | 39 |
| strftime_field_match | 94 |
| strftime_field_new | 1,211 |
| strftime_field_new+compile | 1,447 |
| strftime_field_new_cached+compile | 38 |

## Final design review (advisory, by a reviewer agent)
# Rust review: /work/port (parse-port-core + parse-port-py), round 2

Library, edition 2021. The core has its own sre-compatible backtracking regex engine (`pyre`), plus the format→regex builder, converters and `_strptime`. The PyO3 crate handles Python objects. With `CARGO_TARGET_DIR=/tmp/tgt`, `cargo fmt --check` and `cargo clippy --all-targets` are clean and the 30 core tests pass. `-W clippy::pedantic` gives about 190 warnings, mostly `must_use`, doc backticks and missing `# Errors`, none of which matter here. I built the release `.so` and ran every behaviour claim below against upstream on local CPython 3.12. **No gaming found:** there are no input-specific branches, and the regressions.json case is left alone (see tried.md). Most of the previous review is fixed: the recursion segfault, surrogates, `Parser` `__dict__`, and the shared `BoundedCache`. **Most important:** two pieces of interpreter state are hard-coded, which produces confirmed differences.

## Must fix
### 1. `sys.setrecursionlimit` is ignored (`parse-port-py/src/parser.rs:133` `RECURSION_LIMIT = 1000`)
**Problem (run):** with `setrecursionlimit(5000)`, a `with_pattern` regex of 1500 nested groups raises `RecursionError` in the port, but upstream returns `<Result ('a',) {}>`. With `setrecursionlimit(300)` and 200 groups, the port matches and upstream raises. The comment says `sys` "may not be imported", but `py.import("sys")` always works because `sys` is built in.
**Fix:** `let limit: usize = py.import("sys")?.getattr("getrecursionlimit")?.call0()?.extract()?;` in `nesting_limit`.
### 2. `int()` digit limit: wrong message and fixed value (`parse-port-core/src/pystr.rs:298,373-379`)
**Problem (run):** `parse('{:d}','1'*5000)` gives `Exceeds the limit (4300) for integer string conversion…` in the port. CPython 3.12 says `(4300 digits)`. Also, after `sys.set_int_max_str_digits(0)`, upstream returns the 5000-digit int, but the port still raises. With the limit set to 1000, upstream reports 1000 and the port still reports 4300.
**Fix:** change the format string to `"Exceeds the limit ({} digits) for …"`, and pass the limit in. For example, add `py_int_limited(s, base, max_digits: Option<usize>)`, and have the py layer read `sys.get_int_max_str_digits()`, where 0 means no limit. `pystr.rs` is frozen, so this needs a `shared_change` request, or a wrapper in `pyre::unicode15::py_int`, which all callers already use.

## Should fix
### 3. Python stack depth is found by provoking an exception (`parse-port-py/src/parser.rs:148-163`)
`python_depth` calls `fractions.Fraction("")` on purpose and walks the frames of the traceback. That is fragile (it depends on `fractions` staying pure Python) and costs an import plus an exception. It only runs on a cache miss.
**Fix:** `sys._getframe(0)` and walk `f_back`, or `PyEval_GetFrame` through `pyo3::ffi`.
### 4. RecursionError is a sentinel string inside `Error::Value` (`parse-port-core/src/pyre/mod.rs:29-37`, `pyre/parser.rs:309`)
`is_recursion` compares the message text. A Rust caller sees a `ValueError` that is really a recursion error, and any future ValueError with that text would be misreported (C-GOOD-ERR). `error.rs` is frozen, which explains the workaround.
**Fix:** request an `Error::Recursion(String)` variant (it is `#[non_exhaustive]`, so adding it isn't breaking), map it in `to_pyerr`, and delete `is_recursion`.
### 5. Lazy `.+?` scanning is 16× slower than sre (`parse-port-core/src/pyre/vm.rs:421-440,621-640`)
Measured: `parse('{} {:d} y','x'*200000+' 42 y')` takes 25 ms in the port and 1.6 ms upstream. `search('{:d}z','1'*3000)` takes 56 ms against 33 ms. Lazy `RepOne` grows by one char per backtrack: a pop, a push, and after `MEMO_AFTER` a `memo_admit` key build and hash for every char. sre's `MIN_REPEAT_ONE` instead skips ahead to the next occurrence of the following literal.
**Fix:** at compile time, record the literal (if any) that follows a lazy `RepOne`. In the lazy branch, scan forward with `m.matches(c) && c != lit` (or `memchr`-style) before resuming. `RepOne` backtrack points are fully determined by `(pc, pos)`, so they could also skip `memo_admit`.
### 6. No Rust-level `parse` (`parse-port-core/src/format/parser.rs:150-170`)
The core yields a regex and `Conversion` descriptors, but running them (`evaluate_result`, `_expand_named_fields`) exists only in the py layer (`parser.rs:579-860`). A Rust user can't get typed values without rewriting about 300 lines. That's acceptable for the port's goal. For a usable crate, add `enum Value { Str, Int(BigInt), Float(f64), Temporal(..), Custom(String) }` and a `Parser::evaluate(&Captures, &[char]) -> Result<ParsedValues>` for the built-in conversions. The py layer would keep only custom converters and dict expansion.

## Consider
### 7. `plain_class_names` rewraps the getters in `builtins.property` at import (`parse-port-py/src/parser.rs:1422-1429`)
This is deliberate, to match upstream's descriptor types. Add a one-line comment saying which harness check needs it, so nobody "cleans it up".
### 8. Stray doc comment (`parse-port-py/src/parser.rs:1042-1043`)
`/// \`int_convert(base)(text)\` as a Python int.` is attached to `plain_float`. It belongs on `int_to_py`.
### 9. Large py file (`parse-port-py/src/parser.rs`, 1451 lines; the `impl Parser` at `:395-860` is about 465 lines)
Move evaluation (`evaluate`, `convert_group`, `expand_named_fields`, `EvalPlan`) into `parser/evaluate.rs`, next to `parser/re_match.rs`.
### 10. `pystr` vs `unicode15` (`parse-port-core/src/lib.rs:15`)
`crate::pystr`, which has the wrong Unicode version for 3.12, is still `pub`. decisions.md repeats "never use it" four times. Make it `#[doc(hidden)]`/`#[deprecated]`, or re-export `unicode15` at the root. This is blocked by the freeze, so request it.
### 11. Lints
There is no `[lints]` table. BeautifulRust's `lints.toml` would flag the `as u32` casts in frozen `types.rs:212-264` (`cast_possible_truncation`) and the 28 `Result` functions missing `# Errors`.

## What's already good
- **Layering:** the core contains no Python and forbids `unsafe`. `ExtraTypes` abstracts the user mapping cleanly. Python-only duck-typing (foreign `re.Match`, `setdefault`) stays in the py crate.
- **Robustness:** the VM is iterative with undo records, and parser depth is now capped. Indexing goes through `get`, and there's no `unwrap`/`expect` outside tests. Fuzzing ran 11 targets with no crashes.
- **Fast paths:** each fast path has a test that checks it against the general path (`date_fast_matches_general`, `unicode15` ASCII tests), and every error comes from the general path.
- **Caches:** one `BoundedCache` stores only successes, sharing is documented as behaviour-neutral, and `new_cached` is used only when no user code can run.


## Usage
Cost is Claude Code's client-side estimate of the API-equivalent price, not a bill.

| Role | Sessions | Output tokens | Estimated cost |
| --- | --- | --- | --- |
| cleanup | 2 | 22,425 | $1.83 |
| debugger | 12 | 253,243 | $14.82 |
| performance | 5 | 258,216 | $18.23 |
| planner | 1 | 69,249 | $3.00 |
| porter | 5 | 261,360 | $12.14 |
| reviewer | 3 | 43,022 | $3.33 |
| test_writer | 3 | 52,062 | $2.71 |

**Total estimate:** $56.06
