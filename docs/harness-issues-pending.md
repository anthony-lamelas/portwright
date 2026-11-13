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
