# Conformance report: isodate

**Experimental, unofficial Rust port. Not affiliated with or endorsed by the original authors, and not actively maintained.**

- **Original:** https://github.com/gweis/isodate at commit `4408b60fecfc682a021047b3e5d9095a8face012`

- **Result:** matched on what was tested


This report shows the port matched the original on the inputs tested. It doesn't prove the two are equivalent.

## Changes since this report (by hand, not a new run)

`vendor/pc_re` and `vendor/pc_datetime` were replaced with the current crates (harness issues #84 and #86): pc_re now raises `re`'s `FutureWarning`s and refuses `re.DEBUG`, and pc_datetime follows its shim's tightened domain. isodate uses neither crate's shim-level functions, only their regex engine, dates and timedeltas, so only the error mapping in `crates/isodate-port-py/src/errors.rs` changed. Re-checked in the toolchain image (CPython 3.12.14, seed 20261101, 150 examples), with warnings compared: no differences across 37,856 calls outside the approved list below, and 278 of 280 upstream tests pass (the same two `monkeypatch` tests fail). Fuzzing for 30 s per target found no crashes in its 10 targets. Re-checked again after the port's comparison moved to a container of its own with a guard on the code it runs (harness issue #88): no differences across 37,856 calls, 278 of 280 upstream tests. Its first mutation run judged only 10 of 40 sampled mutants, because cargo-mutants writes `PyResult::new(..)` mutants that never build; with those dropped, a re-run caught 26 of 33 (#90). Five of the seven misses can't change any result isodate returns; `checks/test_mutation_gaps.py` catches the other two (`%j` for a date whose `toordinal()` isn't an int, and Decimal addition of a zero with a far smaller exponent) and shows no differences on the port across 1,811 calls. The gate table and figures below are from the original run.


## Gates
| Gate | Result | Detail |
| --- | --- | --- |
| canaries | pass | caught 138/144 planted bugs (96%, minimum 85%) |
| cleanup | pass | Rust lines went from 6302 to 6159 (5% growth allowed) |
| coverage | pass | checks reach 97.5% of the original (minimum 80%) |
| fuzz | pass | no crashes in 10 targets, 120s each |
| holdout_tests | pass | 278/280 upstream tests pass (2 failures use private internals or mocks) |
| main_build | pass | the Python package builds |
| main_differential | pass | no differences for all APIs across 37856 calls |
| main_integrity | pass | no integrity problems |
| main_license | pass | credit is present and all dependency licenses are allowed |
| main_rust_quality | pass | rustfmt, Clippy, and Rust tests pass |
| main_upstream_tests | pass | 275/275 upstream tests pass |
| performance-1 | pass | 8 faster, 0 slower (noise threshold 5%) |
| performance-2 | pass | 7 faster, 0 slower (noise threshold 5%) |
| performance-3 | pass | 2 faster, 0 slower (noise threshold 5%) |
| rust_coverage | pass | checks reach 84.0% of Rust lines and 83.0% of functions |
| rust_mutation | pass | checks caught 10/10 Rust mutants (100%, minimum 60%); 30 not viable |
| self_comparison | pass | no differences across 28811 calls to 95 APIs |
| self_comparison_after_final_review | pass | no differences across 37854 calls to 125 APIs |
| self_comparison_after_review | pass | no differences across 32883 calls to 95 APIs |
| upstream_baseline | pass | 280/280 upstream tests pass |

## Bugs the checks didn't catch
Planted bugs that went unnoticed are blind spots in this report.

**In copies of the Python:**
- `isodate/duration.py:299`: swap And
- `isodate/isostrf.py:140`: return None
- `isodate/tzinfo.py:124`: negate condition
- `isodate/tzinfo.py:125`: return None
- `isodate/tzinfo.py:134`: return None
- `isodate/version.py:3`: change 7

**In the Rust:**
None.

## Upstream tests
2 test functions were held back from agents and run only by the harness.

**Failing tests that use private internals or mocks** (reported, not blocking):
- tests.test_strf::test_format[dt0-%Y-%m-%dT%H:%M:%S%Z-2012-12-25T13:30:00+10:00]
- tests.test_strf::test_format[dt1-%Y-%m-%dT%H:%M:%S%Z-1999-12-25T13:30:00+11:00]

## Approved differences
- isodates.build_date_regexps outcome.value.v[0].module
- isotzinfo.TZ_RE.match outcome.value.module
- isotime.build_time_regexps outcome.value.v[0].module
- isotzinfo.TZ_RE.search outcome.value.module
- isotzinfo.TZ_RE.fullmatch outcome.value.module
- isotzinfo.TZ_RE outcome.value.module

## Rulings by a person
Agents had to follow these. Some accept differences that the list above doesn't show.

- Granted: you may now change crates/isodate-port-py/src/lib.rs. Put the ten submodule names (isoerror, version, tzinfo, isotzinfo, duration, isostrf, isodates, isotime, isodatetime, isoduration) in __all__ in module_init, so maturin's 'from .isodate import *' exposes isodate.tzinfo, isodate.duration and the rest, as upstream does. Change only that.
- The sys.modules request is answered by the harness, not by allowing sys: crates/isodate-port-py/src/portwright_submodules.rs (written by the harness, don't edit it; the integrity gate accepts its sys import only while it is unchanged) registers the root module's submodules as sys.modules['isodate.<name>']. In lib.rs (unfrozen for this), add 'mod portwright_submodules;' and call 'portwright_submodules::register_submodules(root)?;' at the end of module_init, after __all__ is set. That makes 'from isodate.duration import Duration' and pickling a Duration work. Don't import sys anywhere else.
- Accepted difference (Anthony, via Claude, who recommended it when asked; Anthony replied 'go with what you think is best'): Duration's operator methods (__add__, __radd__, __sub__, __rsub__, __mul__, __rmul__) are objects defined in Rust, not Python functions. Their type identity, inspect.isfunction/ismethod, isinstance(..., types.FunctionType/MethodType), and function-only attributes (__code__, __globals__, __defaults__, __closure__ and the like) may differ from upstream. Don't compile or exec Python to fake them, and never give a Rust type a builtins module or another type's name. Checks must not compare those properties, so remove those probes from checks/test_review_methods.py. Keep comparing names, __qualname__, __module__, __doc__, signatures, binding, calls and errors. Don't route expression probes through ISO8601Error or any API they aren't about: a probe's differences would share that API's signature, so approving one would hide real differences in it. Probe Duration's methods through a Duration API.
- Accepted difference (Anthony, chosen when asked): build_date_regexps, build_time_regexps and TZ_RE return the port's Rust-backed pattern objects, not re.Pattern. Their type identity (type, __module__, __class__, isinstance(..., re.Pattern)) may differ from upstream. The request to allow importing re in the bindings is refused: keep matching on the Rust pc_re matchers and keep py/tz/pattern.rs. Checks must not compare those type properties, so remove such probes. Keep comparing pattern text, flags, groups, groupindex and every match/search/fullmatch/findall/finditer/scanner result, span, group and error. Probe these through the pattern APIs themselves, not through unrelated APIs. Also keep the fixes for the other 7 kinds of difference the final review found.

## Differences agents proposed (not approved unless listed above)
The final comparison finds no differences outside the approved list. So each proposal below was fixed later, is covered by an approved difference or a ruling, or concerns inputs the comparison doesn't judge, such as calls where the original times out.

- {"signature": "isodates.build_date_regexps / isotime.build_time_regexps / isotzinfo.TZ_RE (+ .match/.search/.fullmatch results): type module 're' vs 'isodate.isotzinfo'", "reason": "Already accepted by the person (Rust-backed pattern objects, re import refused). Remaining check probes test_tz.py same('isotime.build_time_regexps') and same('isotzinfo.TZ_RE.search', ...) compare type identity and should be removed; /work/checks is read-only to agents."}
- {"signature": "isodates.build_date_regexps outcome.value.v[0].module", "reason": "Person-accepted ruling: build_date_regexps/build_time_regexps/TZ_RE return Rust-backed pc_re Pattern/Match objects; their type __module__ is isodate.isotzinfo, not re. Spoofing module=\"re\" is forbidden and importing re is refused. Class qualname, repr, spans, groups, groupdict all match upstream. Probes in checks/test_tz.py:108-111 and checks/test_review_findings3.py:112-116 compare type module via same() and should be removed per the ruling (checks are read-only to agents)."}
- {"signature": "isodates.build_date_regexps outcome.value.v[0].module", "reason": "Accepted by a person: build_date_regexps/build_time_regexps return Rust-backed Pattern objects (module isodate.isotzinfo) instead of re.Pattern; only type identity differs, every pattern/match API result matches."}
- {"signature": "isotime.build_time_regexps outcome.value.v[0].module", "reason": "Person-accepted ruling: build_date_regexps/build_time_regexps/TZ_RE return Rust-backed pc_re Pattern/Match objects; their type __module__ is isodate.isotzinfo, not re. Spoofing module=\"re\" is forbidden and importing re is refused. Class qualname, repr, spans, groups, groupdict all match upstream. Probes in checks/test_tz.py:108-111 and checks/test_review_findings3.py:112-116 compare type module via same() and should be removed per the ruling (checks are read-only to agents)."}
- {"signature": "isodate.Duration.__add__/__radd__/__sub__/__rsub__/__mul__/__rmul__ (type / inspect.isfunction / ismethod / __code__)", "reason": "Upstream's operators are Python functions. An extension can create a FunctionType only from a code object, which needs compile/exec (rejected by the integrity gate) or hand-assembled bytecode. The port exposes honest descriptor objects (isodate.duration.DurationOperator) with upstream's name, qualname, module, __doc__, signature, binding, call and error behavior; only type identity and function-only attributes (__code__, __globals__, ...) differ."}
- {"signature": "isodates.build_date_regexps / isotime.build_time_regexps: type(p).__module__ ('re' upstream, 'isodate.isotzinfo' in port)", "reason": "Upstream returns real re.Pattern objects. The port may not import re (integrity ALLOWED_PYTHON_IMPORTS), and the person's decision forbids naming a Rust type after another type (re.Pattern), as review finding #2 flagged. The pc_re-backed isodate.isotzinfo.Pattern matches behaviour (match/fullmatch/search/findall/finditer/split/sub/subn/groupindex, Match objects). Only type identity differs. See requests/allow-re-import.json for the real fix."}
- {"signature": "isotzinfo.TZ_RE.fullmatch outcome.value.module", "reason": "Person-accepted ruling: build_date_regexps/build_time_regexps/TZ_RE return Rust-backed pc_re Pattern/Match objects; their type __module__ is isodate.isotzinfo, not re. Spoofing module=\"re\" is forbidden and importing re is refused. Class qualname, repr, spans, groups, groupdict all match upstream. Probes in checks/test_tz.py:108-111 and checks/test_review_findings3.py:112-116 compare type module via same() and should be removed per the ruling (checks are read-only to agents)."}
- {"signature": "isotzinfo.TZ_RE.match outcome.value.module", "reason": "Person-accepted ruling: build_date_regexps/build_time_regexps/TZ_RE return Rust-backed pc_re Pattern/Match objects; their type __module__ is isodate.isotzinfo, not re. Spoofing module=\"re\" is forbidden and importing re is refused. Class qualname, repr, spans, groups, groupdict all match upstream. Probes in checks/test_tz.py:108-111 and checks/test_review_findings3.py:112-116 compare type module via same() and should be removed per the ruling (checks are read-only to agents)."}
- {"signature": "isotzinfo.TZ_RE.match outcome.value.module", "reason": "Accepted by a person: TZ_RE and build_*_regexps return the port's Rust-backed pattern objects (and their Match objects), not re.Pattern/re.Match. The bindings may not import re, and naming a Rust type module 're' is forbidden spoofing. Spans, groups, groupdict, text, flags and errors all match."}
- {"signature": "isotzinfo.TZ_RE.search outcome.value.module", "reason": "Person-accepted ruling: build_date_regexps/build_time_regexps/TZ_RE return Rust-backed pc_re Pattern/Match objects; their type __module__ is isodate.isotzinfo, not re. Spoofing module=\"re\" is forbidden and importing re is refused. Class qualname, repr, spans, groups, groupdict all match upstream. Probes in checks/test_tz.py:108-111 and checks/test_review_findings3.py:112-116 compare type module via same() and should be removed per the ruling (checks are read-only to agents)."}
- {"signature": "isodate.Duration.__add__/__radd__/__sub__/__rsub__/__mul__/__rmul__ (type, inspect.isfunction/ismethod, __code__)", "reason": "Upstream's operators are Python functions. The port can't create a PyFunction without a code object, which needs exec/compile (forbidden by the integrity gate) or a spoofed code object. The port uses honest isodate.duration.DurationOperator objects. Behavior, signature, names, docs and binding identity match; only the function-type introspection differs."}
- {"signature": "isotzinfo.TZ_RE.search outcome.value.module; isotzinfo.TZ_RE.match outcome.value.module; isotzinfo.TZ_RE.fullmatch outcome.value.module", "reason": "Same accepted pattern-type difference, applied to the Match object a Rust-backed Pattern returns (isodate.isotzinfo.Match vs re.Match). test_review_findings3.py::test_tz_re_direct returns the Match through `same`, so its type module is compared. Can't be 're' without module='re' or importing re (both forbidden). span/group/groups/groupdict/lastgroup/lastindex/pos/endpos all match. Asking the person to confirm the acceptance covers Match as well as Pattern."}
- {"signature": "isodate.isodates.build_date_regexps / isodate.isotime.build_time_regexps / isodate.isotzinfo.TZ_RE (type of the returned compiled patterns and their matches)", "reason": "Upstream returns real re.Pattern / re.Match objects. The PyO3 bindings may not import `re` (integrity gate), and naming a Rust type re.Pattern is forbidden type-name spoofing, so these are isodate.isotzinfo.Pattern / Match / Scanner backed by the vendored pc_re. Behaviour (match/search/fullmatch/scanner/findall/finditer/split/sub/subn/expand, attributes, argument errors, signatures) matches CPython 3.12; only type identity (__module__, isinstance(x, re.Pattern), reprs that embed the type name) differs."}
- {"signature": "isotime.build_time_regexps outcome.value.v[0].module; isodates.build_date_regexps outcome.value.v[*].module", "reason": "Person-accepted difference: build_*_regexps return the port's Rust-backed isodate.isotzinfo.Pattern, not re.Pattern. The read-only check test_tz.py::test_build_regexps returns the list through `same`, so the harness compares each element's type module ('re' vs 'isodate.isotzinfo'). Matching it needs module='re' or importing re; both are forbidden by the decisions. Pattern text, flags, groups, groupindex and all match results are equal."}
- {"signature": "isodates.build_date_regexps / isotime.build_time_regexps / isotzinfo.TZ_RE.{match,search,fullmatch}: outcome.value[.v[i]].module", "reason": "Person-accepted difference: patterns and their Match objects are Rust-backed (module isodate.isotzinfo), not re.Pattern/re.Match; importing re is refused and spoofing module='re' is forbidden. Pattern text, flags, groups, groupindex and every match view/span/group already match (test_match_view_direct, test_review_pattern pass). The diffs come only from read-only checks that return raw objects: test_tz.py::test_build_regexps and test_review_findings3.py::test_tz_re_direct. See tried.md for the probe rewrite."}
- {"signature": "isotzinfo.TZ_RE.{match,search,fullmatch} / isodates.build_date_regexps / isotime.build_time_regexps outcome.value[.v[i]].module", "reason": "Already accepted by a person: Pattern/Match objects are the port's Rust-backed isodate.isotzinfo.{Pattern,Match}, not re.Pattern/re.Match. Only type __module__ differs; qualname, repr and all match data are identical. Spoofing module='re' is forbidden and importing re was refused. The probes should be removed from checks (read-only in this sandbox)."}
- {"signature": "isotzinfo.TZ_RE.match / isodates.build_date_regexps outcome .module ('re' vs 'isodate.isotzinfo')", "reason": "Person-accepted: pattern objects are Rust-backed, not re.Pattern; type identity (__module__) of Pattern/Match may differ. All match results, spans, groups, reprs, flags and groupindex are identical. The remaining failing probes compare only the result type's module and should be removed from read-only checks (test_tz.py::test_build_regexps, test_review_findings3.py TZ_RE.match direct calls) per the person's decision."}

## Likely bugs in the original
The port reproduces these on purpose.

# tz
- parse_tzinfo: TZ_RE.match never fails (all optional, unanchored), so it parses only a prefix ('Zulu' -> UTC, 'x' -> None) and its ISO8601Error is unreachable. Reproduced.
- FixedOffset.__repr__ uses `'%r' % name`: a tuple name is treated as the format args (() / 2+ items raise TypeError). Reproduced.
- build_tzinfo multiplies tzhour/tzmin by an int, so str hours become repeated/empty strings before timedelta rejects them. Reproduced.
# duration.py (reproduced)
- `Duration.__str__` uses "%d month" whenever months <= 1, so negative months print as "-2 month".
- `Duration.__add__`'s second `except AttributeError` is unreachable for timedelta (+ raises TypeError), so
  `Duration(1) + 5` raises TypeError instead of returning NotImplemented, while `Duration(1).__sub__(5)` returns NotImplemented.
- A Duration without `tdelta` (`Duration.__new__(Duration)`) recurses in `__getattr__` until RecursionError.
- `fquotmod` with plain ints fails: `int / int` is a float, which has no `to_integral` (AttributeError).
# duration (reproduced; also in state/upstream_bugs.md)
- fquotmod with plain ints: int/int is a float, which has no to_integral (AttributeError).
- Duration(1)+5 raises TypeError; Duration(1).__sub__(5) returns NotImplemented. The except AttributeError branch can't be reached.
- __str__ prints "%d month" whenever months <= 1, so negative months print as "-2 month".
# strf (reproduced)
- The date-path regex has no %P/%p, so duration_isoformat(None) returns 'P%P' (and strftime(date, 'P%P') leaves it literal).
- '%%0%dd' % yeardigits: yds=0 -> '%00d' (no padding), negative yds -> '%0-Nd' left-justified ('+1985 ', '+19 '); %C uses yds-2 and true division.
- duration_isoformat with Decimal('NaN') years raises InvalidOperation from `years < 0`.
# parse (reproduced)
- parse_date with yeardigits 0/1/2: `[0-9]{0}` / `{-1}` (a literal in `re`) leave the year/century group empty or
  holding '{-1}', so `int()` raises ValueError("invalid literal for int() with base 10: ...").
- parse_time rounds the microseconds half-even after quantizing, so e.g. '23.9999999999999' gives
  ValueError('microsecond must be in 0..999999') instead of rolling over.
- parse_duration's alternative format can never return a timedelta (`durdt.year == 0` is impossible).
- `repr(Duration(months='1e999999999999999999'))` raises MemoryError (CPython can't allocate int(Decimal)); smaller huge exponents give
  ValueError (int str-digit limit). The port reproduces this with a cutoff at 10**10 digits, which is a machine-dependent boundary.


## Speed through Python
Median nanoseconds per call on one machine. Below 1.00x means the port is slower.

| Benchmark | Original | Port | Speedup |
| --- | --- | --- | --- |
| max_days | 210 | 100 | 2.09x |
| parse_date_ext | 1,413 | 261 | 5.41x |
| parse_date_week | 2,824 | 316 | 8.94x |
| parse_datetime_utc | 5,735 | 359 | 15.99x |
| parse_duration_full | 5,757 | 715 | 8.06x |
| parse_duration_td | 5,663 | 319 | 17.74x |
| parse_time_tz | 5,915 | 518 | 11.42x |
| parse_tzinfo_ext | 2,133 | 395 | 5.41x |

## Final design review (advisory, by a reviewer agent)
*This review was written before its fix round. New checks reproduced these differences, and the fix round fixed all of them: `Duration.__add__.__name__.__class__ outcome.value.v`, `Duration.__mul__.__name__.__class__ outcome.value.v`, `Duration.__name__.__class__ outcome.value.v`, `Duration.__radd__.__name__.__class__ outcome.value.v`, `Duration.__rmul__.__name__.__class__ outcome.value.v`, `Duration.__rsub__.__name__.__class__ outcome.value.v`, `Duration.__sub__.__name__.__class__ outcome.value.v`, `FixedOffset.__name__.__class__ outcome.value.v`, `ISO8601Error outcome.value.v.args.v[0].v`, `isodates.build_date_regexps.__name__.__class__ outcome.value.v`, `isotime.build_time_regexps.__name__.__class__ outcome.value.v`, `isotzinfo.TZ_RE.match outcome.kind` and 2 more. A finding below that none of these covers wasn't reproduced and may still stand.*

# Rust review: /work/port (isodate-port-core + isodate-port-py; vendor/ not reviewed)

Reviewed as a library. `core/` = `crates/isodate-port-core/src`, `py/` = `crates/isodate-port-py/src`. Clippy (workspace, all targets): 0 warnings. Core tests: 30 pass. I built the wheel (`CARGO_TARGET_DIR=/tmp/t`) and ran it next to upstream on CPython 3.12.

The person-granted `py/lib.rs` change is **already in place**, so I edited no code. The ten submodule names are appended to `__all__` (`lib.rs:194-197`), then `portwright_submodules::register_submodules(root)?` runs. I confirmed that `from isodate.duration import Duration` works and that pickling a Duration round-trips. `__all__` has 59 names, against upstream's 49; the extra 10 are the submodule names, as decided.

The core is sound: `#![forbid(unsafe_code)]`, overflow-checks on, `BigInt` where Python ints can grow, and no `unwrap`/`expect` on input-reachable paths. **The source is byte-identical to the last review (state/review.md). Both of its Must-fix items are still open, and both break the person's rulings.**

## Must fix
### 1. Pattern objects claim to be `re.Pattern` and have no match API (`py/tz.rs:602`)
**Rule:** Person's decision: never give a Rust type another type's name or module; keep `py/tz/pattern.rs` on `pc_re` (C-NEWTYPE).
**Confirmed by running:** On the port, `type(build_date_regexps()[0])` and `type(TZ_RE)` print `<class 're.Pattern'>` (a spoof). `hasattr(p, m)` is False for `match`, `search`, `findall` and `groupindex`. Upstream has all four, and `checks/test_review_pattern.py` compares them. `py/tz/pattern.rs` doesn't exist, although `state/tried/fix-final_review-3.md` describes writing it.
**Fix:** Add `py/tz/pattern.rs` with Pattern, Match and Scanner over `pc_re::Pattern`. It needs match/search/fullmatch/findall/finditer/scanner/sub/subn/split and groupindex, plus Match span/group/groups/groupdict/lastgroup/lastindex/re/string/pos/endpos. Errors should raise the same class as upstream. Then:
```rust
#[pyclass(name = "Pattern", module = "isodate.isotzinfo", frozen)]   // not module = "re"
```

### 2. Duration operators claim to be `builtins.function`/`builtins.method`, with the wrong doc and no signature (`py/duration.rs:996`, `:1062`)
**Rule:** Same decision (no `builtins` module, no other type's name). The difference in type identity is accepted, so spoofing it gains nothing.
**Confirmed by running:**
- `Duration.__add__.__doc__` is the rustdoc ("A \`def name(self, other)\` method of upstream's…"). Upstream gives its own docstring, and `None` for `__mul__`.
- `inspect.signature(Duration.__add__)` raises `ValueError: no signature found for builtin`. Upstream gives `(self, other)`.

`checks/test_review_methods.py` compares both.
**Fix:**
```rust
#[pyclass(name = "DurationMethod", module = "isodate.duration", frozen)] // + "BoundDurationMethod"
struct BinaryMethod { name: &'static str, qualname: &'static str, doc: Option<&'static str>, body: BinaryBody }
#[getter] fn __doc__(&self) -> Option<&'static str> { self.doc }          // upstream text verbatim
#[getter] fn __text_signature__(&self) -> &'static str { "(self, other)" }
```
- The bound type forwards `__doc__` and `__module__` to `func`, and its `__text_signature__` is `"(other)"`.
- Turn the `///` rustdoc on `BinaryMethod` (`:990-995`) into `//` comments, so pyo3 can't publish it as `__doc__`.

## Should fix
### 3. Zero-field `Duration` invariant is enforced only by a comment (`py/duration.rs:463`, `:972-986`)
`duration_new` returns `object.__new__(cls)`, a zeroed PyO3 object. That's valid only while `struct Duration {}` has no fields and no `Drop`. If someone adds a field, the result is an uninitialised Rust value and the compiler says nothing.
**Fix:** `const _: () = assert!(std::mem::size_of::<Duration>() == 0 && !std::mem::needs_drop::<Duration>());`

### 4. Two `tp_name` helpers that behave differently (`py/duration.rs:398` vs `py/tz.rs:57-69`)
**Rule:** DRY (Effective Rust item 24).
**Problem:** If `__module__` is missing, `type_tp_name` returns an error, while `tz::tp_name` falls back to `"builtins"`. So the TypeError text for a user type whose metaclass hides `__module__` depends on which operator raised it.
**Fix:** Delete `type_tp_name` and call `crate::tz::tp_name` everywhere.

### 5. Stale crate doc (`py/lib.rs:7-10`)
The doc says submodules are "*not* in `sys.modules`" and that pickle callables must live on the root. Both have been false since `portwright_submodules` was added.
**Fix:** `//! Submodules are registered in sys.modules by portwright_submodules (harness-owned).` `lib.rs` is unfrozen only for the granted change, so the file's owner or the person must make this edit.

## Consider
### 6. Shared helpers live in `py/tz.rs` (`tz.rs:51,57,223,294,330`)
`bind_args`, `match_text`, `str_cps`, `tp_name` and `use_bare_type_name` are also used by duration.rs, parse.rs and strf.rs. Moving them to `py/args.rs` would leave the 792-line tz.rs with only the tz types.

### 7. Global mutex held while compiling (`core/parse/dates.rs:241-251`)
One lock covers both lookup and compile. That's deliberate: no regex is built twice. The cache is unbounded per `yeardigits`, but so is upstream's `DATE_REGEX_CACHE`. The code recovers from a poisoned lock with `PoisonError::into_inner`. That's safe because the insert happens only after a successful build, but add a comment saying so.

### 8. Duration state is in `__dict__`, not Rust fields (`py/duration.rs:463`)
This is deliberate: upstream lets users reassign `d.years` and `d.tdelta`. The cost is a dict lookup per operator. Say so in the struct doc, so nobody "optimises" it away.

### 9. Hand-written fast matchers alongside the regexes (`core/parse/{dates,times,durations}.rs`, `core/tz.rs`)
This is a reasonable trade for speed, and differential tests keep the two in step. Add a comment at each `rx` source naming its matcher, so a regex edit updates both.

### 10. Checks (not port code)
`test_review_methods.py` and `test_review_pattern.py` no longer probe type identity, isfunction/ismethod, `__code__` or `isinstance(re.Pattern)`, as the decisions require. Each probe goes through the Duration or pattern API it is about. No change needed.

## What's already good
- The core has no Python dependency. Duck typing goes through traits (`Operand`, `StrfSource`) with `Error: From<core::Error>`, so a `PyErr` passes through `?` unchanged and is never turned into a string.
- `decimal.rs` is a careful `_pydecimal` port with bounded exponents. Big ints are `BigInt` or checked arithmetic and never wrap.
- Regexes compile once (`rx::Lazy` over `OnceLock`, plus the per-key cache). Nothing recompiles per call.
- The only `unreachable!` in the core (`core/decimal.rs:1089`) is in `#[cfg(test)]` code.


## Usage
Cost is Claude Code's client-side estimate of the API-equivalent price, not a bill.

| Role | Sessions | Output tokens | Estimated cost |
| --- | --- | --- | --- |
| cleanup | 1 | 14,836 | $1.29 |
| debugger | 27 | 413,614 | $28.23 |
| performance | 3 | 144,631 | $9.33 |
| planner | 1 | 59,647 | $3.28 |
| porter | 10 | 297,794 | $16.77 |
| reviewer | 6 | 59,470 | $4.73 |
| test_writer | 9 | 150,319 | $9.87 |

**Total estimate:** $73.51
