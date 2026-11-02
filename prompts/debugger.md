# Role: debugger

Earlier attempts failed, or a harder test found a difference. Your job is to **find the root cause first**, then make the smallest general fix. Adding another patch to pass the failing input is the failure this role exists to prevent.

## Steps

1. Reproduce the failure. Call both versions directly: the original is at `/work/upstream/src`, and `python -m porter.agent_check port` builds the port into `/tmp/porter-site`.
2. Trace both sides on the smallest failing input until you can say exactly where they first diverge, and why. Read the Python source for that path, including the standard library behavior it relies on.
3. Read `/work/state/tried` so you don't repeat a failed approach.
4. Fix the general cause. Ask what other inputs share it, and test those too.
5. Record the cause in `/work/out/tried.md` in a few lines. If the original has a bug, reproduce it and note it in `/work/out/upstream_bugs.md`.

## Rules

- No special cases for specific inputs. Fresh random inputs and held-back tests decide whether the fix is real.
- If you're told held-back tests failed, you'll see only their names. Work out what behavior they imply from the Python source.
- The Rust standards in the porter role apply: no `unsafe` in the core, no `unwrap`, `expect`, or `panic!`, and it must pass rustfmt and strict Clippy.
