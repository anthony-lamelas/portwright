"""Measures speed through Python, compares versions, and reads native benchmarks.

A speed result is only meaningful for a port that matches, so the report shows these
numbers next to the parity results.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

from portwright import isolation
from portwright.compare import worker as wire
from portwright.compare.session import Side, Worker
from portwright.gates import GateResult
from portwright.toolchain import host_target_dir, run_in_toolchain

# Changes smaller than this are treated as noise.
NOISE = 0.05


def load_benchmarks(checks_dir) -> list[dict]:
    path = Path(checks_dir) / "benchmarks.json"
    return json.loads(path.read_text()) if path.exists() else []


def run_benchmarks(side: dict, benchmarks: list[dict], *, samples: int = 15) -> dict[str, dict]:
    """Time each benchmark on one side. Benchmarks call the port, so this runs contained."""
    if not isolation.enabled():
        return _run_benchmarks_here(side, benchmarks, samples)
    with isolation.scratch("porter-bench-") as scratch:
        request = Path(scratch) / "request.json"
        answer = Path(scratch) / "results.json"
        side = {**side, "path": str(Path(side["path"]).resolve()), "python": isolation.python(sys.executable)}
        request.write_text(json.dumps({"side": side, "benchmarks": benchmarks, "samples": samples}))
        done = isolation.run(
            [isolation.python(sys.executable), "-m", "portwright.gates.bench", str(request), str(answer)],
            cwd=scratch, env={"PYTHONPATH": str(isolation.REPO_ROOT)}, read=[side["path"]],
            timeout=300.0 * max(1, len(benchmarks)) + 60,
        )
        if not answer.exists():
            raise RuntimeError(f"benchmarks didn't run: {(done.stdout + done.stderr)[-2000:]}")
        return json.loads(answer.read_text())


def _run_benchmarks_here(side: dict, benchmarks: list[dict], samples: int) -> dict[str, dict]:
    worker = Worker(Side(**side), timeout=300.0)
    worker.start()
    results = {}
    try:
        for bench in benchmarks:
            results[bench["name"]] = worker.request(
                {
                    "op": "bench",
                    "path": bench["target"],
                    "args": [wire.encode(item) for item in bench.get("args", [])],
                    "kwargs": {key: wire.encode(item) for key, item in bench.get("kwargs", {}).items()},
                    "samples": samples,
                }
            )
    finally:
        worker.close()
    return results


def speedups(reference: dict, candidate: dict) -> dict[str, float | None]:
    """Return how many times faster the candidate is, for each benchmark."""
    ratios = {}
    for name, result in reference.items():
        other = candidate.get(name, {})
        if result.get("kind") == "benchmark" and other.get("kind") == "benchmark":
            ratios[name] = result["median_ns"] / other["median_ns"]
        else:
            ratios[name] = None
    return ratios


def performance_gate(before: dict, after: dict, *, noise: float = NOISE) -> GateResult:
    """Accept a change only if something got measurably faster and nothing got slower."""
    improved, regressed, broken = {}, {}, {}
    for name, old in before.items():
        new = after.get(name, {})
        if old.get("kind") != "benchmark":
            # It never measured anything (it raises on the code before the change, too),
            # so it can't show a regression. Counting it as one rejected every change.
            broken[name] = old.get("message") or old.get("kind")
            continue
        if new.get("kind") != "benchmark":
            regressed[name] = "benchmark didn't run"
            continue
        change = old["median_ns"] / new["median_ns"] - 1
        if change > noise:
            improved[name] = change
        elif change < -noise:
            regressed[name] = change
    passed = bool(improved) and not regressed
    summary = f"{len(improved)} faster, {len(regressed)} slower (noise threshold {noise:.0%})"
    if broken:
        summary += f"; {len(broken)} benchmarks don't run and were ignored: {', '.join(broken)}"
    return GateResult("performance", passed, summary, {"improved": improved, "regressed": regressed, "broken": broken})


def run_native_benchmarks(workspace, core_crate: str, *, target_dir=None) -> dict:
    """Run Criterion benchmarks in the core crate, if there are any."""
    done = run_in_toolchain(
        ["cargo", "bench", "-p", core_crate, "--", "--quick", "--noplot"],
        workspace, target_dir=target_dir, timeout=1800,
    )
    if done.returncode != 0:
        return {"ran": False, "output": done.stderr[-1500:]}
    results = {}
    root = host_target_dir(workspace, target_dir) / "criterion"
    for estimate in root.rglob("new/estimates.json"):
        name = str(estimate.parent.parent.relative_to(root))
        results[name] = json.loads(estimate.read_text())["median"]["point_estimate"]
    return {"ran": bool(results), "median_ns": results}


if __name__ == "__main__":  # inside the gate container: python -m portwright.gates.bench REQUEST RESULTS
    _request = json.loads(Path(sys.argv[1]).read_text())
    Path(sys.argv[2]).write_text(json.dumps(_run_benchmarks_here(_request["side"], _request["benchmarks"], _request["samples"])))
