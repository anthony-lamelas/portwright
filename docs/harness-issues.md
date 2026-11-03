# Harness issues log

Every problem that real runs have exposed in Portwright itself, with the cause and the fix. Port-specific
differences belong in each port's `CONFORMANCE.md`, not here. New entries go at the bottom of the right table.

## Fixed

| # | Found in | What went wrong | Cause | Fix |
|---|---|---|---|---|
| 1 | parse | Five correct debugger fixes were rejected | The gates ran the original on the host's Python 3.12.3, and agents previewed on 3.12.16. `strptime`'s `%I` pattern differs between them | The run refuses to start unless the host and toolchain Pythons match |
| 2 | parse | Debuggers repeated failed approaches | Each round started over from main, without the previous attempt or why it was rejected | The rejected attempt carries forward, with the rejection in the prompt |
| 3 | parse | A port that broke on lone surrogates passed | Hypothesis's `st.text()` never produces surrogates | `portwright.compare.strategies.text()` mixes them in, with characters whose Unicode data changed between versions |
| 4 | parse | A correct fix was rejected with `ModuleNotFoundError` | Saved failing inputs were replayed by evaluating callbacks in the test process, which can't import the library | Callbacks stay as expressions and are evaluated inside each worker |
| 5 | humanize | Intake couldn't import a `src/` layout package | The package was copied to `upstream/src/src/<name>` | Copy the package under its own name |
| 6 | humanize | The checks phase failed three rounds and stopped the run | The API manifest listed submodule names as if the package root exported them (`humanize.Unit`), so the checks had to call names that can't exist | Name submodule APIs by the path callers use (`time.Unit`) |
| 7 | humanize | Every `i18n` attempt was rejected as changing a frozen file | The frozen-file check hashed only directories, so a frozen top-level `Cargo.toml` always counted as changed. A debugger agent found this | Compare frozen top-level files by their own hash |
| 8 | humanize | The machine ran out of memory | An agent's scratch script reached 13 GB. The container memory limit (16 GB) was larger than the machine (15 GB), so it never applied | All agents together get at most 60% of RAM, with no swap |
| 9 | humanize | Agents kept running, and spending usage, after the orchestrator was killed. Their token copies stayed on disk | Nothing cleaned up a killed run's containers. Snap Docker refuses `docker kill` | Containers are labeled by run and cleaned up at startup, at exit and on SIGTERM. Each container also has its own time limit |
| 10 | humanize | A resumed run would have waited 3 hours to restart tasks | Claims left by the killed run held their lease | A run frees all claims when porting starts |
| 11 | humanize | A porter built its own comparison tool, which caused #8 | The planner put Python's formatting in a "foundation" task whose own APIs never used it, so the harness couldn't check that work | Planner rule: no infrastructure-only tasks. Porter rule: rely on the check command and flag unreachable work |
| 12 | pycompat | Planted bugs mean nothing for a shim over built-ins | The original's logic is CPython's C code, so there is no Python to plant bugs in | Runs can supply hand-written planted bugs, kept from agents |
| 13 | pycompat | Two of my own 42 hand-written planted bugs changed nothing | One reproduced Python's output exactly, and one's trigger condition was never true | Caught by applying each to the shim and checking that an output changes. See open item C |
| 14 | pycompat | Checks could make the host run out of memory | Comparison workers run on the host without a limit, and a formatting check can ask for a width of billions | Each worker's address space is capped at 1 GB (`PORTWRIGHT_WORKER_MEMORY_MB`), so huge requests raise `MemoryError` in both versions and the outcome is still compared. Covers part of open item E |
| 15 | pycompat | A port of the built-ins could pass without porting anything | The PyO3 layer may import `builtins` and call `repr()`, `str()` and methods on Python objects, which run CPython's own code. Found by review before any agent tried it | `--strict-bindings`: the binding crate may only convert values and raise exceptions. A short list of conversion calls, such as `encode` with `surrogatepass`, is allowed |

## Open

| # | Found in | Issue | Idea |
|---|---|---|---|
| A | humanize | The planner rule in #11 is only in the prompt. Code can't tell from a plan whether a task's notes hide unchecked work | Measure Rust coverage of each task's new code with its own checks, at merge time |
| B | parse | Fuzz targets and mutation runs go one at a time, which is most of the 2.5-hour hardening phase | Run them in parallel within the memory budget |
| C | pycompat | The harness can't tell a no-op hand-written planted bug from a blind spot in the checks. Both count as "missed" | Before the checks phase, confirm each written bug changes some output, using the upstream tests' inputs |
| D | humanize | If the orchestrator dies by SIGKILL, its containers run until their own time limit (up to about 2 hours) | A heartbeat file the container watches |
| E | humanize | The host-side gate runs (original library, Rust builds) have no memory limit | Run them under a cgroup limit as well |
| F | parse | The final reviewer's findings arrive after the last fix round, so they can only be reported | Allow one more fix round when the final review finds confirmed behavior differences |
