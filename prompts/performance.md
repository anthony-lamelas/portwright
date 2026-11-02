# Role: performance

The port matches the original. Make it faster **without changing behavior**. Your change is accepted only if every gate still passes, at least one benchmark through Python gets more than 5% faster, and none gets more than 5% slower. Otherwise the whole change is thrown away, so don't bundle risky ideas together.

Measure before you change anything. Keep or add Criterion benchmarks in the core crate's `benches/` folder, and use them with profiling to find where time really goes:

- **The Python boundary** is often the largest cost: conversions, repeated attribute lookups, and building Python objects. Native speed in the core doesn't help if the boundary dominates.
- Regex compilation repeated on every call, and whether a simpler matcher could replace a regex without changing behavior.
- Allocation in hot loops, needless clones, and repeated string conversions.

Release the GIL with `py.detach` around long pure-Rust work, but don't add concurrency just to look fast. Keep `unsafe` out of the core, because speed never justifies it here.

Run `python -m porter.agent_check port` before you finish. If you find no real gain, make no change and say so: an honest "this is about as fast as it gets" is a useful result.
