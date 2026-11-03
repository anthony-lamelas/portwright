# Conformance report: pc_unicode

**Experimental, unofficial Rust port. Not affiliated with or endorsed by the original authors, and not actively maintained.**

- **Original:** https://github.com/anthony-lamelas/portwright/tree/main/pycompat/shims/pc_unicode at commit `11a1ee5dc419a6f248a28c0f4c31a4fa42c04270`

- **Result:** matched on what was tested

- **What the original is:** CPython 3.12.14's str methods and unicodedata (Unicode 15.0) for core built-in types, called through a shim of one-line functions


This report shows the port matched the original on the inputs tested. It doesn't prove the two are equivalent.

## Gates
| Gate | Result | Detail |
| --- | --- | --- |
| canaries | pass | caught 36/38 hand-written planted bugs (95%, minimum 85%) |
| cleanup | pass | Rust lines went from 18265 to 18301 (5% growth allowed) |
| coverage | pass | checks reach 100.0% of the original (minimum 80%) |
| fuzz | pass | no crashes in 43 targets, 120s each |
| holdout_tests | pass | 54/54 upstream tests pass |
| main_build | pass | the Python package builds |
| main_differential | pass | no differences for all APIs across 62093 calls |
| main_integrity | pass | no integrity problems |
| main_license | pass | credit is present and all dependency licenses are allowed |
| main_rust_quality | pass | rustfmt, Clippy, and Rust tests pass |
| main_upstream_tests | pass | 25/25 upstream tests pass |
| performance-1 | pass | 9 faster, 0 slower (noise threshold 5%); 1 benchmarks don't run and were ignored: lookup_hangul |
| performance-2 | pass | 9 faster, 0 slower (noise threshold 5%); 1 benchmarks don't run and were ignored: lookup_hangul |
| performance-3 | pass | 5 faster, 0 slower (noise threshold 5%); 1 benchmarks don't run and were ignored: lookup_hangul |
| rust_coverage | pass | checks reach 95.4% of Rust lines and 92.8% of functions |
| rust_mutation | pass | checks caught 32/36 Rust mutants (89%, minimum 60%); 4 not viable |
| self_comparison | pass | no differences across 61971 calls to 43 APIs |
| self_comparison_after_review | pass | no differences across 62089 calls to 43 APIs |
| upstream_baseline | pass | 54/54 upstream tests pass |

## Bugs the checks didn't catch
Planted bugs that went unnoticed are blind spots in this report.

**Hand-written planted bugs** (the original's logic is C, so there was no Python to plant bugs in):
- 06-swapcase-sharp: swapcase('ß') stays 'ß'
- 16-center-bias: center() puts the odd pad on the right

**In the Rust:**
- `crates/pc-unicode-port-core/src/value.rs:73` in `Value::truthy`: 
- `crates/pc-unicode-port-core/src/normalization.rs:148` in `decompose`: <=
- `crates/pc-unicode-port-core/src/layout.rs:449` in `expandtabs`: *=
- `crates/pc-unicode-port-core/src/error.rs:69` in `<impl fmt::Display for Error>::fmt`: Ok(Default::default())

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

# names (CPython 3.12 unicodedata.lookup quirks, reproduced)
- `lookup` prefixes `HANGUL SYLLABLE ` and `CJK UNIFIED IDEOGRAPH-` are case-sensitive (jamo and hex too),
  though other names are matched case-insensitively: `lookup('hangul syllable ga')` -> KeyError.
- `CJK UNIFIED IDEOGRAPH-04E00` (5 digits, leading zero) is accepted.
- KeyError message is cut at the first NUL: `lookup('x\0y')` -> "undefined character name 'x'".
- `lookup` accepts bytes; bytearray -> TypeError 'lookup() argument must be read-only bytes-like object, not bytearray'.
# names (CPython 3.12 unicodedata.lookup quirks, reproduced)
- `HANGUL SYLLABLE ` / `CJK UNIFIED IDEOGRAPH-` prefixes are matched case-sensitively; other names case-insensitively.
- `CJK UNIFIED IDEOGRAPH-04E00` (leading zero, 5 digits) is accepted.
- KeyError message is cut at the first NUL: `lookup('x\0y')` -> "undefined character name 'x'".
# names (CPython 3.12 unicodedata.lookup quirks, reproduced)
- `lookup` prefixes `HANGUL SYLLABLE ` and `CJK UNIFIED IDEOGRAPH-` are case-sensitive (jamo and hex too),
  though other names are matched case-insensitively: `lookup('hangul syllable ga')` -> KeyError.
- `CJK UNIFIED IDEOGRAPH-04E00` (5 digits, leading zero) is accepted.
- KeyError message is cut at the first NUL: `lookup('x\0y')` -> "undefined character name 'x'".
- `lookup` accepts bytes; bytearray -> TypeError 'lookup() argument must be read-only bytes-like object, not bytearray'.
# names (CPython 3.12 unicodedata.lookup quirks, reproduced)
- `HANGUL SYLLABLE ` / `CJK UNIFIED IDEOGRAPH-` prefixes are matched case-sensitively; other names case-insensitively.
- `CJK UNIFIED IDEOGRAPH-04E00` (leading zero, 5 digits) is accepted.
- KeyError message is cut at the first NUL: `lookup('x\0y')` -> "undefined character name 'x'".
# normalization (CPython 3.12 unicodedata quirks, reproduced)
- Empty `s` short-circuits before the form is validated: `normalize('bad', '')` -> `''`,
  `is_normalized('bad', '')` -> `True` (no ValueError). Type errors are still raised first.
# normalization
- CPython checks `s == ''` before validating `form`: normalize('bogus', '') -> '' and is_normalized('bogus', '') -> True. Reproduced.


## Speed through Python
Median nanoseconds per call on one machine. Below 1.00x means the port is slower.

| Benchmark | Original | Port | Speedup |
| --- | --- | --- | --- |
| casefold_paragraph | 768 | 615 | 1.25x |
| char_name_cjk | 467 | 230 | 2.03x |
| expandtabs_table | 728 | 493 | 1.48x |
| is_normalized_nfc_ascii | 558 | 201 | 2.78x |
| lookup_hangul | n/a | n/a | n/a |
| normalize_nfd_hangul | 943 | 671 | 1.41x |
| normalize_nfkc_mixed | 3,715 | 1,208 | 3.08x |
| split_whitespace | 875 | 1,079 | 0.81x |
| splitlines_keepends | 838 | 891 | 0.94x |
| title_sentence | 801 | 665 | 1.20x |

## Native Rust speed
| Benchmark | Median ns |
| --- | --- |
| casefold_paragraph | 236 |
| char_name_cjk | 64 |
| expandtabs_table | 138 |
| is_normalized_nfc_ascii | 29 |
| lookup_hangul | 249 |
| normalize_nfd_hangul | 273 |
| normalize_nfkc_mixed | 708 |
| normalize_nfkd_mixed | 323 |
| split_whitespace | 508 |
| splitlines_keepends | 208 |
| title_sentence | 290 |

## Final design review (advisory, by a reviewer agent)
# Rust review: /work/port (pc-unicode-port-core + pc-unicode-port-py)

This is a library port of `pc_unicode` (str methods and unicodedata, CPython 3.12 / Unicode 15.0) that stores text as `Vec<u32>` code points and reads generated tables. I reviewed it as a library.
**The code is in good shape.** I found no gaming: the tables are generated, there are no special cases for particular inputs, and the tests check real CPython behavior. Panics are guarded: indexing is bounds-checked or uses `.get()`, integers go through checked converters, and `overflow-checks` is on in release. There is one `unsafe` block, and it is audited. The core and PyO3 layers are cleanly separated.
Since the last review, these are fixed: the quadratic split search (now KMP), conversion that ran before the guard, the duplicated helpers, and the Hangul constants.
- `cargo clippy --workspace --all-targets`: clean.
- `-W clippy::pedantic` on core: 123 warnings, almost all literal separators and doc backticks in generated tables, plus 3 missing `# Errors` sections.
- `cargo test`: 19 tests pass.
- I built the wheel and ran about 20 edge cases against upstream. All matched except #1.

## Must fix
### 1. Keyword names with lone surrogates are mangled in error messages (`py/src/convert.rs:202`, `core/src/call.rs:52`)
**Confirmed by running both:**
- `upper(**{'\udc80x': 1})`: upstream gives `...unexpected keyword argument '\udc80x'`.
- The port gives `'���x'`.

`bind_call` converts each key with `to_string_lossy()`, and `bind` takes `Vec<(String, T)>`. The core `Error.message: String` can't hold a surrogate either.
**Fix:** pass keys as `Text` (`text_of(k.cast::<PyString>()?)`) and compare them with `p.name` code point by code point. For the error, the message must carry the surrogate. One option is to let `bind` return the offending key separately, e.g. `Err(BindError { kind, prefix, key: Text, suffix })`. The PyO3 layer then builds the message with `text_to_py`. If the frozen `Error` type can't change, record this in `differences/`.

## Should fix
### 2. Core API is Python-typed (`casing.rs`, `layout.rs`, `normalization.rs`, `unidata.rs`, `names.rs`: every `pub fn`)
This was raised last review and is still open. Every public function takes `&Value` and reports CPython message strings. A Rust user who wants `upper(&[u32])` must allocate a `Value::Str` and then match on `Error.message`.
**Fix:** add typed entry points over the private functions that already exist:
- `pub fn normalize_text(s: &[u32], form: Form)` (make `Form` public and implement `FromStr`)
- `upper_text(&[u32]) -> Text`
- `split_text(&[u32], Option<&[u32]>, usize) -> Vec<&[u32]>`

Keep the `&Value` versions as thin wrappers that check arguments.

### 3. Table generators are not with the port (`tables/*.rs`, `names/table.rs` headers)
The headers point to `out/tools/gen_case_tables.py` and `gen_names.py`, but neither is in `/work/state`. That leaves about 15k generated lines that nobody can regenerate or audit.
**Fix:** keep the generators as harness artifacts. If the integrity gate permits it, commit a non-Python checksum or a README that names their location.

### 4. `ErrorKind` is not `#[non_exhaustive]` and `Display` is Rust-flavored (`core/src/error.rs:7`, `:65`)
- Adding an exception kind later (for example `UnicodeEncodeError`, which is already special-cased through `LookupError`) breaks downstream matches. Rule: C-STRUCT-PRIVATE / API evolution.
- `Display` prints `Type: msg`.

**Fix:** add `#[non_exhaustive]`. Add `ErrorKind::python_name() -> &'static str` and use it in `Display`, giving `TypeError: msg`.

## Consider
### 5. Name-table decode failure is silently swallowed (`core/src/names.rs:108`)
`decode_table().unwrap_or_default()` is now covered by the `table_decodes_every_name` test, which is acceptable. An `expect("generated name table is valid")` would still fail loudly in production instead of turning every name into "no such name" (M-PANIC-ON-BUG).

### 6. Repeated table lookups (`casing.rs:43-50`, `normalization.rs` `ccc`/`qc_field`)
- `has()` → `flags()` → `record()` decodes the full 5-field record for each flag test.
- `compose` calls `ccc(next)` and `qc_field(next)` separately for the same character.

Measured performance is already at CPython parity (split 1M whitespace: 2 ms; strip with a 1000-char set: 10 ms vs 2 ms upstream), so this is polish only.

### 7. 4 bytes per code point (`value.rs:9`, `layout.rs:66`)
`Text = Vec<u32>` uses 4× CPython's memory for Latin-1 text. Huge paddings hit `MemoryError` or the OOM killer sooner than upstream would. This follows from a frozen decision, so I'm noting it, not asking for a change.

### 8. `strip` with `chars` is O(n·m) (`layout.rs:338`)
`set.contains(&c)` scans the set for each character. CPython does the same (with a bloom prefilter), so this isn't a regression. A `[bool; 256]` prefilter for small code points would be cheap.

### 9. Lints
There is no `[lints]` table. A `[workspace.lints.clippy] pedantic = "warn"`, with `unreadable_literal` and `doc_markdown` allowed in `tables/`, would catch the missing `# Errors` docs and the `must_use` gaps. See BeautifulRust `lints/`.

## What's already good
- `call.rs` reproduces CPython's call-error order and wording generically, with tests. `bind_call` has a cheap path for positional calls that still runs the guard in upstream order.
- Converting text to Python picks CPython's cheapest decoder (ASCII, Latin-1, or UTF-32 with BOM plus `surrogatepass`). Lone surrogates survive the round trip without extra unsafe code.
- `LookupError::Surrogates` reproduces the span of CPython's `UnicodeEncodeError`. KeyError text from invalid bytes matches CPython's `%s` replacement.
- Allocation sizes are checked before allocating (`new_text`, `MAX_TEXT_LEN`, `try_reserve_exact`) and map to `MemoryError`/`OverflowError` like `PyUnicode_New`.


## Usage
Cost is Claude Code's client-side estimate of the API-equivalent price, not a bill.

| Role | Sessions | Output tokens | Estimated cost |
| --- | --- | --- | --- |
| cleanup | 1 | 11,949 | $0.89 |
| debugger | 3 | 16,960 | $1.10 |
| performance | 3 | 90,519 | $5.31 |
| planner | 2 | 41,613 | $1.65 |
| porter | 8 | 128,553 | $7.06 |
| reviewer | 2 | 23,492 | $2.05 |
| test_writer | 5 | 57,023 | $2.79 |

**Total estimate:** $20.86
