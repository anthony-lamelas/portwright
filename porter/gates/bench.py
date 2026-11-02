"""Measures speed through Python, compares versions, and reads native benchmarks.

A speed result is only meaningful for a port that matches, so the report shows these
numbers next to the parity results.
"""

from __future__ import annotations

import json
from pathlib import Path

from porter.compare import worker as wire
from porter.compare.session import Side, Worker
from porter.gates import GateResult
from porter.toolchain import host_target_dir, run_in_toolchain

# Changes smaller than this are treated as noise.
NOISE = 0.05


def load_benchmarks(checks_dir) -> list[dict]:
    path = Path(checks_dir) / "benchmarks.json"
    return json.loads(path.read_text()) if path.exists() else []


def run_benchmarks(side: dict, benchmarks: list[dict], *, samples: int = 15) -> dict[str, dict]:
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
    improved, regressed = {}, {}
    for name, old in before.items():
        new = after.get(name, {})
        if old.get("kind") != "benchmark" or new.get("kind") != "benchmark":
            regressed[name] = "benchmark didn't run"
            continue
        change = old["median_ns"] / new["median_ns"] - 1
        if change > noise:
            improved[name] = change
        elif change < -noise:
            regressed[name] = change
    passed = bool(improved) and not regressed
    summary = f"{len(improved)} faster, {len(regressed)} slower (noise threshold {noise:.0%})"
    return GateResult("performance", passed, summary, {"improved": improved, "regressed": regressed})


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
