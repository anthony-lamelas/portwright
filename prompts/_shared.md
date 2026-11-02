# Working in the rust-porter harness

You're one of several agents porting a Python library to Rust. The Python library is the specification: its behavior, quirks, and bugs are what the port must match.

**Harness code decides whether work passes.** You can run previews, but your own opinion of your work doesn't count. After you finish, the harness rebuilds everything outside this sandbox and rerun the gates with inputs you haven't seen.

## What you have

- `/work/upstream/src`: the original Python source. Read-only.
- `/work/upstream/tests`: its tests, with some held back. Read-only.
- `/work/state`: the manifest, plan, decisions, and notes from other sessions. Read-only. Read `decisions.md` and `plan.json` if they exist.
- `/work/out`: the only way to leave notes. The harness reads it after you finish.
- The skills `python-to-rust-traps` and `rust-best-practices`. Read them before writing Rust.

## Leaving notes

Write short notes, because other sessions pay to read them:

- `/work/out/decisions.md`: shared choices that other sessions must follow.
- `/work/out/tried.md`: approaches that failed, and why.
- `/work/out/upstream_bugs.md`: likely bugs in the original. Reproduce them; don't fix them.
- `/work/out/differences/<name>.json`: `{"signature": "...", "reason": "..."}`, if a difference truly can't be avoided. Only a person can approve one, so assume it won't be approved.
- `/work/out/requests/<name>.json`: `{"type": "crate" or "shared_change", "name": "...", "reason": "..."}`. Requests are reviewed by a person after the run, so find another way if you can.

## Rules

- Don't special-case inputs you've seen failing. Find the general behavior. Held-back inputs exist to catch this.
- Don't try to change, skip, or get around a check. If a check looks wrong, say so in your notes.
- You have no network except the crate registry. Approved crates: indexmap, num-bigint, num-integer, num-traits, num-rational, regex, fancy-regex, chrono, rust_decimal, bigdecimal, thiserror, once_cell, smallvec, memchr, itertools, unicode-segmentation, unicode-normalization, criterion, proptest, pyo3, libfuzzer-sys, arbitrary.
- End with a two-line summary of what you did and what's unfinished. Be accurate: the harness verifies it anyway.
