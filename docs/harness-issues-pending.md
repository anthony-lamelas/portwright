# Harness issues waiting to go into the log

These were found or fixed in a cloud session while the `pc_datetime` run and an unpushed
commit (#35, the `grant` command) were still on another machine. Fold them into
[`harness-issues.md`](harness-issues.md) once #35 is pushed, numbering fixed rows after the
last one there, then delete this file.

## Fixed (move from Open)

| Was | Found in | Issue | Cause | Fix |
|---|---|---|---|---|
| K | pc_unicode | A task merged, then ran again after a restart, costing $0.40 | The merge into main and the board's "merged" status are written separately. Stopping the process between them leaves the task pending | Just before the fast-forward, the merge records the commit it is landing under `refs/portwright/landing/<task>`. On resume, a task whose marker is part of main is marked merged without another session. A marker for work that never reached main, or a worktree that never changed, doesn't count (`tests/test_resume.py`) |
| N | pc_re | After a person approves the only difference blocking a task, the task still goes through another agent session before the gate runs again | Nothing re-checked the existing work when a ruling changed what its gate accepts | Each gate run records a fingerprint of the approved differences, approved crates and frozen files. If that fingerprint has changed by the next attempt, the harness gates the last rejected work again first (on a seed no session sees) and starts a session only if it still fails. That session sees the new failure text |
| — | vendoring (before its first use) | Whole-workspace gates would have judged a vendoring port on its vendored crates: about 77k frozen lines from pc_re, pc_unicode and pc_format against a few hundred of its own | `cargo mutants --workspace`, Rust coverage totals and `count_rust_lines` all included `vendor/`. Most sampled mutants would land in frozen code, which could trigger wasted strengthen sessions. Cleanup would be pointed at "dead" vendored code it can't change, and its 5% growth limit would allow thousands of lines | Mutants exclude `vendor/**`. Coverage totals and per-file hints use only the port's own files. Line counts skip `vendor/`, and they now ignore the run directory's own path, which used to count 0 for any run under a `target/` folder (`tests/test_vendoring.py`) |
| — | vendoring (before its first use) | Agents were told to read each vendored crate's README and CONFORMANCE.md, which weren't there | A pycompat core crate's report sits two folders up, at its port root, and wasn't copied | `CONFORMANCE.md` is copied in and frozen with the crate. The prompt names only that file |
| — | vendoring (before its first use) | A file added inside a vendored crate went unnoticed, such as a `build.rs` that Cargo would run | The frozen-file check only compares files that were hashed when the crate was vendored | Any file under `vendor/<name>/` that wasn't vendored counts as a frozen-file change |

## Open (add to the Open table)

| Found in | Issue | Idea |
|---|---|---|
| vendoring | `--vendor` only applies on a fresh run. A resumed run loads its saved config and silently ignores new `--vendor` flags, and crates are only copied when `port/.git` doesn't exist | Refuse `--vendor` on a resume whose saved config differs, or vendor on resume before porting starts |
| vendoring | A vendored crate loses its release profile. pc_re's `lto = "fat"` and `codegen-units = 1` live in its own workspace root, which isn't copied, so it runs slower inside a port than its report says | Merge vendored crates' profile settings into the port's workspace root when vendoring |
| vendoring | Clippy (`--all-targets`) and nextest (`--workspace`) lint and test every vendored crate, including its criterion benches, on every gate | Exclude vendored packages from those two steps |
| intake | Several common upstream layouts can't start. `--tests` must be a directory. Upstream tests run with `-c /dev/null` and only the library on `PYTHONPATH`, so helpers like `from common import ...` fail. A license file must be one of the known names (`LICENSE.rst` isn't), and only MIT, BSD and Apache wording is recognised, though cargo-deny allows ISC and Unlicense | Accept a single test file, put the tests folder on `PYTHONPATH`, and widen license names and wordings to what cargo-deny already allows. Found choosing the first vendoring port: tabulate, pyhumps and pytimeparse were ruled out by these alone |
