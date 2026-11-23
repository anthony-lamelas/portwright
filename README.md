# Portwright

**Experimental.** Portwright is a harness that has Claude agents port a Python library to Rust. It produces a Rust core crate, a thin Python package on top of it, and a conformance report that shows how closely the port matches the original and where it doesn't.

Agents write the Rust. Ordinary code decides whether the work counts. The original Python library is the reference throughout. Agents choose test inputs, but the original supplies every expected answer, and no agent judges its own work.

> Status: two real ports so far: [`ports/parse`](ports/parse), from [`parse`](https://github.com/r1chardj0n3s/parse), and [`ports/isodate`](ports/isodate), from [`isodate`](https://github.com/gweis/isodate). Not actively maintained. Unofficial; not affiliated with the authors of any library it ports.

## What a run does

```
intake ─► checks ─► design ─► port ─► harden ─► fix review ─► cleanup ─► performance ─► final review ─► report
 code     agent      agent    agents   code+agents  agents        agent       agent          agent          code
```

1. **Intake (code only).** Pins the upstream commit, refuses non-permissive licenses, lists the public API, runs the upstream tests on the original, and hides about 20% of them from every agent.
2. **Checks (test-writer agent).** Writes Hypothesis input generators, not expected values. Before any Rust exists, the harness checks the checks:
   - They must compare the original with itself and find no differences.
   - They must reach 80% branch coverage of the original.
   - They must catch planted bugs in copies of the original (at least 85%, set from the first real run, which caught 92%).
   - They must call every public API and get past input validation.
3. **Design (planner agent).** Splits the work into tasks and freezes shared types, so parallel porters can't collide.
4. **Port (porter agents, then debugger agents).** One task at a time, or in parallel (`--max-agents`). Each task merges only when its gates pass on fresh random inputs the agent never saw.
5. **Harden.** Long differential runs, the hidden upstream tests, cargo-fuzz, and cargo-mutants on the Rust. Rust mutants the checks miss go back to the test writer, not the porter.
6. **Fix review.** A read-only reviewer looks for bugs. The test writer turns each behavior claim into new checks, and debuggers fix whatever those checks expose. The Python still supplies every expected answer, so a wrong review finding can't make the port worse.
7. **Cleanup and performance agents.** Each change is accepted only if every gate still passes. Performance changes must also measurably beat the benchmarks.
8. **Report.** [`CONFORMANCE.md`](ports/parse/CONFORMANCE.md) lists:
   - every gate, and planted bugs and Rust mutants that went uncaught
   - upstream tests that couldn't run
   - proposed differences, none of which are approved unless a person approves them
   - likely upstream bugs the port reproduces on purpose
   - speed and estimated cost

The verdict is never "equivalent". At best it is **"matched on what was tested"**.

### How the comparison works

Each version of the library runs in its own process, and each call records one of two outcomes:
- **A return:** the exact type and value. `1` and `1.0` differ, floats compare bit for bit, and dict order matters.
- **A raise:** the exception type, args, message and base classes.

Both outcomes also record changes to the arguments, printed output, and the point at which a lazy iterator fails. Crashes and hangs count as differences, and the worker restarts. Objects stay alive across calls, so checks can drive classes and callbacks on both sides.

### Isolation

Agents run Claude Code in Docker containers:
- **Network:** none, except an allowlisting proxy for the Anthropic API and the crate registry.
- **Files:** the original library and the checks are mounted read-only. The hidden tests are never mounted.
- **Crates:** dependencies are limited to an approved list.

Integrity gates reject a port that imports the original, edits a check, or changes frozen files.

Gates run agent code in throwaway containers too, and a built port's comparison workers get a container of their own: just the port's package, no original, checks or run files, no network and an empty environment. An audit hook in the worker refuses any Python source the port tries to compile and run. Three red-team agents tried to cheat the gates (#88, #91): a port that borrowed the original's answers passed the old comparison, and 27 of 30 attacks got past the old static scan. Both are fixed, the attacks are kept as tests, and what still gets through is listed under Limits.

## Results: `parse`

[`parse`](https://github.com/r1chardj0n3s/parse) is about 1,000 lines of Python. It turns `format()`-style patterns into regular expressions, and it depends on the details of Python's `re` and `strptime`. The port contains its own sre-compatible regex engine, so the error messages and positions match CPython's exactly.

**Verdict: matched on what was tested.** Full report: [`ports/parse/CONFORMANCE.md`](ports/parse/CONFORMANCE.md).

| Check | Result |
| --- | --- |
| Differential, final gate | no differences across 71,073 calls to every public API (long hardening runs: about 204,000 calls) |
| Upstream tests, visible | 78/80 pass |
| Upstream tests, hidden from agents | 113/115 pass |
| Checks catch planted bugs in the Python | 137/149 (92%) |
| Checks reach the original's branches | 98.1% |
| Checks catch Rust mutants | 17/27 (63%) |
| Checks reach Rust lines | 69.3% |
| cargo-fuzz | 11 targets, 120s each, no crashes |

The same two tests fail in both upstream runs. Both depend on the current date through a mock that patches the original module's internals, so they can't reach a Rust port. The 12 uncaught planted bugs and 10 surviving Rust mutants are listed in the report. They show where the checks are weakest, mostly in error message text and in rarely used `%`-format handling. The planted-bug gate ran with a 70% minimum. The default is now 85%, set from this run's 92%.

**Speed through Python** (median per call, one machine):

| Benchmark | Original | Port | Speedup |
| --- | --- | --- | --- |
| `compile` of a complex pattern | 23.4 µs | 0.16 µs | 147x |
| `findall` numbers | 4.7 µs | 0.22 µs | 21x |
| `strftime`-style field | 21.1 µs | 1.13 µs | 19x |
| simple `parse` | 11.7 µs | 0.69 µs | 17x |
| ISO datetime | 21.9 µs | 1.32 µs | 17x |
| Apache log line | 37.8 µs | 2.41 µs | 16x |
| mixed numbers | 22.6 µs | 1.42 µs | 16x |
| `search` in a log | 9.9 µs | 0.67 µs | 15x |

Some of the speed comes from caching compiled patterns. Most real code calls `parse()` with the same few patterns, and the original rebuilds its `Parser` on every `parse()` call. The benchmarks don't cover worst cases. The reviewer measured a lazy `{}` field scanning 200,000 characters at 25 ms in the port against 1.6 ms in the original.

**Known differences the checks don't cover.** These come from the final read-only review and haven't been fixed:
- The port assumes Python's default recursion limit of 1000. After `sys.setrecursionlimit()`, a deeply nested custom pattern can raise where the original matches, or the reverse.
- Integers longer than 4,300 digits fail with a slightly different message, and the port ignores `sys.set_int_max_str_digits()`.
- The port stores lone surrogates as private code points, so an input that contains both a surrogate and one of those code points can match differently.

**Proposed differences, not approved:**
- A `%`-style width over 2²⁸ characters raises `ValueError` instead of exhausting memory.
- `\N{...}` names in custom patterns are unsupported.

**Upstream quirks the port reproduces on purpose:** 13 are listed in the report. For example, `{` in literal text becomes a regex quantifier, the `|` fill character injects an alternation, and `{a}{a[b]}` raises `TypeError`.

The port is about 11,000 lines of Rust and contains its own sre-compatible regex engine. Before you use it as a crate, read the reviewer's notes in the report. It has no Rust-level `parse()`, and errors are carried as strings in a few places.

### What went wrong along the way (and is fixed in the harness)

The first real run exposed four harness bugs. Each one made correct work look wrong:

- **Python versions:** the gates ran the original on the host's Python 3.12.3, but agents previewed against 3.12.16 in the container. `strptime`'s `%I` pattern differs between those patch releases, so five correct debugger fixes were rejected. Now the harness refuses to start unless both Pythons are the same version.
- **Rejected fixes:** each debugger round started over from scratch and never learned why the last attempt failed. Rejected work now carries forward, along with the reason.
- **Lone surrogates:** Hypothesis's `st.text()` never generates them, so a port that can't handle them passed. `portwright.compare.strategies.text()` now mixes them in, along with characters whose Unicode data changed between versions.
- **Saved failures:** they were replayed by evaluating callbacks in the test process, where the library can't be imported. They are now sent to the workers.

The `parse` run was resumed after each fix, not restarted, so its report shows a run that was paused and resumed. Every harness problem found by real runs since then is logged, with its cause and fix, in [docs/harness-issues.md](docs/harness-issues.md).

### Other ports of `parse`

Others have ported `parse` to Rust, and I found them only after this run: [formatparse](https://github.com/eddiethedean/formatparse) (a PyO3 package that aims to be a drop-in replacement for common patterns) and [parse_rust](https://github.com/lucien2k/parse_rust) (Rust only). This port isn't meant to compete with them. It's a demonstration of the harness, and what's new here is the conformance evidence.

## Results: `isodate`

[`isodate`](https://github.com/gweis/isodate) (BSD-3) is about 1,400 lines of Python that parse and format ISO 8601 dates, times, durations and time zones. It was the first third-party run with the parallel scheduler: the planner split it into four tasks (duration, time zones, parsing, formatting), and two porters worked at once. Its regex and `strftime` behavior comes from the pycompat crates `pc_re` and `pc_datetime`, vendored into the port and frozen. Both were re-vendored after the fixes in #84 and #86, and the port still shows no differences across 37,856 calls and passes 278/280 upstream tests.

**Verdict: matched on what was tested.** Full report: [`ports/isodate/CONFORMANCE.md`](ports/isodate/CONFORMANCE.md).

| Check | Result |
| --- | --- |
| Differential, final gate | no differences across 37,856 calls to 125 APIs (hardening: about 165,600 calls) |
| Upstream tests, visible | 275/275 pass |
| Upstream tests, hidden from agents | 278/280 pass |
| Checks catch planted bugs in the Python | 138/144 (96%) |
| Checks reach the original's branches | 97.5% |
| Checks catch Rust mutants | 10/10 (30 not viable); re-run: 26/33, see below |
| Checks reach Rust lines | 84.0% |
| cargo-fuzz | 10 targets, 120s each, no crashes |

Through Python it is 2.1x to 17.7x faster (parsing a `timedelta` duration: 5.7 µs to 0.32 µs). The run cost about $74 of API-equivalent usage, including review rounds that were rejected.

The two hidden tests that fail patch a fixture with `monkeypatch`, so they can't reach a Rust port. The 6 uncaught planted bugs are listed in the report. The run's mutation gate could judge only 10 of its 40 sampled mutants, because cargo-mutants writes `PyResult::new(..)` mutants that never build. With those dropped, a re-run caught 26 of 33 (#90). Five of the seven misses can't change any result isodate returns, and a new checks file catches the other two.

**Rulings by a person.** The run stopped three times because agents asked for something only a person can decide. Each ruling is printed in the report:
- Submodules such as `isodate.duration` are registered in `sys.modules` by a harness-written file the integrity gate checks byte for byte. The bindings import nothing else from Python.
- `Duration`'s operator methods are Rust objects, not Python functions, so `inspect.isfunction` and function-only attributes differ. Names, docstrings, signatures, calls and errors are still compared. A request to compile Python inside the bindings to fake them was refused.
- The internal helpers `build_date_regexps`, `build_time_regexps` and `TZ_RE` return Rust-backed pattern objects, not `re.Pattern`. Their type is an approved difference; their matching results, spans, groups and errors are still compared. A request to let the bindings import Python's `re` was refused.

The run exposed harness issues #62 to #67, all fixed. The worst was that a review fix loop could keep spending rounds on a request only a person could answer (#65).

## pycompat: Python's built-ins as Rust crates (in progress)

Most Python libraries lean on the same built-ins: `str.format` and `%`, `repr` of floats, int parsing, floor division, Unicode string methods, `re`, and `datetime`. Every port has to reproduce them exactly, so rebuilding them per library wastes effort and invites small differences. pycompat ports them once, as crates later ports can vendor.

There is no Python source to port for a built-in, so each one gets a **shim**: a small package of one-line functions that call the built-in, such as `def float_repr(x): return repr(x)`. CPython 3.12.14 is then "the original", and Portwright ports the shim like any other library. Three things keep that honest:

- **`--strict-bindings`:** the PyO3 layer may only convert values and raise exceptions. It can't call `repr()`, `str()`, or any Python method that would run CPython's own code. A short list of conversion calls, such as `encode` with `surrogatepass`, is allowed.
- **Hand-written planted bugs (`--canaries`):** the harness can't plant bugs in C code, so each shim has 38 to 42 subtle wrong versions, kept from agents. The checks must notice them.
- **Declared domains:** shims accept only core built-in values. A `Decimal` raises a fixed `TypeError`, because its formatting is its own module's code. Before the checks are written, a reviewer reads the shim for inputs that still reach behavior outside the built-ins, and the run stops for a person if it finds any (#74). `--domain-review` asks for the same review of any library (#85).

| Crate | Covers | Status |
|---|---|---|
| `pc_format` | `format()`, `str.format`, `%`, float `repr`, `int`/`float` parsing, `round`, `//`, `%`, `divmod`, `/` | matched on what was tested ([report](pycompat/ports/pc_format/CONFORMANCE.md)) |
| `pc_unicode` | case mapping, `is*` predicates, split/strip/pad, `unicodedata` (Unicode 15.0) | matched on what was tested ([report](pycompat/ports/pc_unicode/CONFORMANCE.md)) |
| `pc_re` | `re` for str patterns, starting from the regex engine written for `parse` | matched on what was tested ([report](pycompat/ports/pc_re/CONFORMANCE.md)) |
| `pc_datetime` | dates, times, timedeltas, ISO 8601, `strftime`, `strptime` | matched on what was tested ([report](pycompat/ports/pc_datetime/CONFORMANCE.md)) |

**`pc_format`** matched CPython on 28,852 compared calls. All 252 shim tests pass, including the 6 held back from agents, and the checks caught 41 of 42 planted bugs. Through Python, it is 1.7x to 6.1x faster than the built-ins. It was rebuilt with strict bindings after the shim started refusing attribute fields up to a parse error (#61), starting from the previous port's code. That dropped the 2,736-line table of CPython attribute lookups, and the crate went from 10,587 to 8,338 Rust lines. The rebuild cost about $25, for about $98 of API-equivalent usage over four runs. The final reviewer found no behavior differences and nothing it had to fix. Before trusting it, read these:

- `round(n, -k)` with a huge `k` returns 0 at once, where CPython spends minutes building `10**k`. That behavior was a person's decision in an earlier run and carried over in the seed code. Calls where CPython timed out or crashed (14 of them) weren't compared.
- Strings are stored as 4 bytes per character, so a result with hundreds of millions of characters needs several times Python's memory.

**`pc_unicode`** matched CPython on 62,093 compared calls and passed all 54 shim tests, including the 6 held back. The checks caught 36 of 38 planted bugs. It took one run and about $21. Through Python it ranges from 0.81x (`split`, which is slower) to 3.1x faster. The final reviewer found one difference: a keyword argument whose name contains a lone surrogate comes out garbled in the port's error message. Its Unicode tables were generated from CPython's own `unicodedata`.

**`pc_re`** matched CPython's `re` on 68,641 compared calls and passed all 195 shim tests. The checks caught all 39 planted bugs. It took one run and about $49, starting from the regex engine written for `parse`. Through Python it is 1.3x to 7.7x faster. `re.DEBUG` output was an approved difference, because strict bindings can't write to Python's `sys.stdout`; the shim and the port now refuse `DEBUG` with a fixed error instead (#76). Since its report, the bindings dropped PyO3's `rust_decimal` conversion, which ran the `decimal` module's code, so the port now passes strict bindings (#77). That leaves one difference, the message of `error(Decimal('1'))`, which its report lists. Comparing warnings (#72) found that the port never emitted `re`'s `FutureWarning`; it now does, at CPython's positions (#84). The final reviewer reported that the port stops at 900 levels of nesting, where CPython allows about 996 when called from a script. The harness's comparison runs at a different Python stack depth and couldn't reproduce it.

**`pc_datetime`** matched CPython on 17,485 compared calls and passed all 30 shim tests. It was built against the shim before its domain checks (#55), and was brought up to the current shim by hand (#86): it now passes all 36 shim tests and strict bindings, with no differences across 17,485 calls. The checks caught 37 of 41 planted bugs, and fuzzing found no crashes in 34 targets. It took about $36 over two runs, interrupted when the machine was off. Through Python it is 1.8x to 5.1x faster. Its core reproduces glibc's `strftime` extensions (`%-d`, `%_H` and others), which the shim now refuses, so they're available only to Rust callers (`strftime::format_glibc`). It also reproduces several quirks of CPython's parsers, which the report lists. The final review's two "Must fix" findings and the two proposed differences in its report predate the final fix round. I checked by hand that all four are fixed and covered by checks: long `strptime` formats that hit a regex size limit, keyword names with lone surrogates, `timezone_name(None)` and the `fromtimestamp` `OSError`. Reports from new runs list which review findings their checks reproduced and fixed (#60).

All six published ports, `parse` and `isodate` included, were re-checked in the toolchain image after warnings joined the comparison (#84). None shows a difference beyond `pc_re`'s `error(Decimal)` message.

Building `pc_format` exposed thirteen harness bugs, issues 18 to 30 in the log. The worst, #29, was that the comparison worker crashed on ints over 4,300 digits on both sides, so those calls passed without being compared. The `parse` run recorded no crashed calls, so its results aren't affected.

## Using it

Requirements: Linux, Docker, [uv](https://docs.astral.sh/uv/), and a Claude account.

```sh
git clone https://github.com/anthony-lamelas/portwright && cd portwright
docker build --network host -t portwright-toolchain:dev -f docker/toolchain.Dockerfile docker
uv venv --python 3.12.14 .venv && uv pip install --python .venv/bin/python -e .
git clone https://github.com/r1chardj0n3s/parse target/src/parse

.venv/bin/python -m portwright.cli run --run-dir runs/parse \
  --lib parse --module parse --source target/src/parse --package parse --tests tests \
  --url https://github.com/r1chardj0n3s/parse --auth api_key --wait
```

- **Your own API key:** use `--auth api_key` with `ANTHROPIC_API_KEY` set. Subscription mode (the default) is for personal runs on your own account, and it refuses to start if an API key is set.
- **Resume:** run the same command again. Runs survive crashes and usage limits; `--wait` waits out limits for up to 72 hours.
- **Approve:** `portwright.cli approve --run-dir ... "<difference>"` approves a proposed difference, `--crate` adds a crate to the approved list, and `--domain` accepts a shim domain review's findings. Only a person should do any of these.
- **Cost:** the `parse` run took 31 agent sessions and about $56 of API-equivalent usage, as estimated by Claude Code: porters $12, debuggers $15, performance $18, everything else $11. That total includes debugger sessions whose fixes were rejected because of the harness bugs above. It doesn't include building the harness.
- **Parallel agents:** `--max-agents 2` runs independent tasks at once, each in its own worktree. All agent containers on the machine together, across runs, are capped at 60% of RAM; a session that doesn't fit waits (#73). Gate containers get 40% of RAM, at most 6 GB each (`PORTWRIGHT_GATE_MEMORY`), and all of them on the machine together share that 40%; one that doesn't fit waits (#83).
- **Disk:** sessions, gates and builds refuse to start with less than 10 GB free (`PORTWRIGHT_MIN_FREE_GB`). The run pauses with exit code 4; free space and run the same command again.
- **A killed run:** every container ends itself within about three minutes of the run's heartbeat stopping, even after SIGKILL (#70).
- **Built-in shims:** `--strict-bindings`, `--canaries DIR` (hand-written planted bugs, relative to `--source`) and `--reference-note TEXT` (what "the original" is, for the report).
- **Reusing earlier ports:** `--vendor NAME=DIR` copies a finished crate into `vendor/NAME` and freezes it. `--seed NAME=DIR` copies earlier code agents may start from, which isn't frozen and has to pass the gates.
- **Harness tests:** `.venv/bin/python -m pytest tests` (about 20 minutes, no Claude usage). The tests use a small fixture library with a hand-written Rust port and scripted agents. A passing run deletes its build folders under `target/`; `PORTWRIGHT_KEEP_TEST_SCRATCH=1` keeps them.

## Limits, read before trusting a report

- A pass means **matched on the inputs tried**. Uncaught planted bugs and surviving mutants in the report mark blind spots.
- **The original library and its tests run on the host, outside Docker.** Only port libraries you trust.
- **Known holes against a determined cheat** (docs/harness-issues.md AB to AD): under strict bindings, a Python object reaching `str()` through a generic or an untyped variable isn't seen by the source scan; the comparison doesn't look past 40 levels of nesting, 1,000 iterator items or `at 0x` in a repr; one approved difference covers every difference at the same place; and a port can tell when it's being compared.
- Docker setup:
  - Agent containers run with `--network none` and `--cap-drop ALL`.
  - Snap Docker breaks `no-new-privileges`, so it isn't set.
  - The short-lived session token is passed into the sandbox, and the proxy allows only the hosts listed above.
  - Only the trusted toolchain image build uses host networking.
- **Python version:** behavior is matched against one Python version (3.12.14 here). Unicode data and standard library details differ between versions.
- **Tested only here:** the harness has only been tested on Linux with snap Docker.
- **Parallel scheduling** works and is tested on the fixture library, but the `parse` run used one agent at a time.

## Repository layout

```
portwright/        the harness (orchestrator, gates, comparison layer, sandbox, scheduler, report)
prompts/           one prompt per agent role
skills/            Rust guidance mounted into agent sessions (see skills/ATTRIBUTION.md)
docker/            the toolchain image
tests/             harness tests with a fixture library and scripted agents
ports/parse/       the generated port, its checks, and CONFORMANCE.md
ports/isodate/     the same for isodate, with its vendored pc_re and pc_datetime crates
pycompat/shims/    one-line shims over Python's built-ins, with their tests and planted bugs
pycompat/ports/    finished pycompat crates, their checks, and CONFORMANCE.md
docs/              the harness issues log
```

## Credits and related work

- **Rust guidance:** the `rust-best-practices` and `rust-review` skills are from [BeautifulRust](https://github.com/morganlinton/BeautifulRust) (MIT).
- **Ideas from other ports:** differential testing against the original, held-back tests, and agent roles draw on earlier write-ups:
  - the [LSST C-to-Rust port](https://dmtn-349.lsst.io)
  - an [agent port of textdistance](https://dev.to/amitthecoder)
  - Anthropic's agent-built C compiler

MIT licensed. Ported libraries keep their own licenses; see each port's `LICENSE.upstream`.
