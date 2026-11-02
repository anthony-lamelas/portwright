"""Runs check files against two versions of a library and judges the result."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
from dataclasses import dataclass, field
from pathlib import Path

from portwright.gates import GateResult

REPO_ROOT = Path(__file__).resolve().parents[2]
# An API counts as tested only if the reference returned normally on at least this
# share of calls. A low bar still catches checks that only ever reach an error.
MIN_RETURN_RATIO = 0.10
PYTEST_EXIT_TESTS_FAILED = 1
PYTEST_EXIT_OK = 0


@dataclass
class CheckRun:
    """Raw results from one pytest run of the check files."""

    exit_status: int
    stats: dict = field(default_factory=dict)
    divergences: dict = field(default_factory=dict)
    failures: list = field(default_factory=list)
    output: str = ""


def run_checks(
    checks_dir: str | Path,
    reference: dict,
    candidate: dict,
    *,
    seed: int = 0,
    max_examples: int = 200,
    timeout: float = 10.0,
    stop_at_first: bool = False,
    run_timeout: float = 1800.0,
) -> CheckRun:
    """Run the check files in a separate process and collect their results."""
    checks_dir = Path(checks_dir).resolve()
    reference = {**reference, "path": str(Path(reference["path"]).resolve())}
    candidate = {**candidate, "path": str(Path(candidate["path"]).resolve())}
    with tempfile.TemporaryDirectory(prefix="portwright-checks-") as scratch:
        results = Path(scratch) / "results.json"
        config = Path(scratch) / "config.json"
        config.write_text(
            json.dumps(
                {
                    "reference": reference,
                    "candidate": candidate,
                    "results": str(results),
                    "max_examples": max_examples,
                    "timeout": timeout,
                }
            )
        )
        command = [
            sys.executable, "-m", "pytest", str(checks_dir),
            "-p", "portwright.compare.pytest_plugin",
            "-p", "no:cacheprovider",
            "--rootdir", str(checks_dir),
            "-c", os.devnull,
            "-q", "--tb=short",
            f"--hypothesis-seed={seed}",
        ]
        if stop_at_first:
            command.append("-x")
        env = {**os.environ, "PORTER_CONFIG": str(config), "PYTHONPATH": str(REPO_ROOT)}
        try:
            done = subprocess.run(
                command, cwd=scratch, env=env, capture_output=True, text=True,
                timeout=run_timeout, check=False,
            )
        except subprocess.TimeoutExpired:
            return CheckRun(exit_status=-1, output="check run timed out")
        output = done.stdout + done.stderr
        if not results.exists():
            return CheckRun(exit_status=done.returncode, output=output)
        data = json.loads(results.read_text())
        return CheckRun(
            exit_status=done.returncode,
            stats=data["stats"],
            divergences=data["divergences"],
            failures=data["failures"],
            output=output,
        )


def _reach_problems(stats: dict, api: list[str] | None) -> dict:
    """Find APIs the checks never call, or call only with inputs that fail."""
    never_called = sorted(set(api or []) - set(stats))
    never_returned = {}
    for name, counts in sorted(stats.items()):
        ratio = counts["returned"] / counts["calls"]
        if ratio < MIN_RETURN_RATIO:
            never_returned[name] = {
                "calls": counts["calls"],
                "returned": counts["returned"],
                "raised": counts["raised"],
            }
    return {"never_called": never_called, "rarely_returned": never_returned}


def differential_gate(
    checks_dir: str | Path,
    reference: dict,
    candidate: dict,
    *,
    api: list[str] | None = None,
    seed: int = 0,
    max_examples: int = 200,
    timeout: float = 10.0,
) -> GateResult:
    """Pass only if the checks really ran, reached the API, and found no differences."""
    run = run_checks(
        checks_dir, reference, candidate, seed=seed, max_examples=max_examples, timeout=timeout
    )
    if run.exit_status not in (PYTEST_EXIT_OK, PYTEST_EXIT_TESTS_FAILED) or not run.stats:
        return GateResult(
            "differential", False,
            f"checks did not run (pytest exit {run.exit_status})",
            {"output": run.output[-4000:]},
        )
    reach = _reach_problems(run.stats, api)
    details = {
        "calls": sum(counts["calls"] for counts in run.stats.values()),
        "stats": run.stats,
        "divergences": run.divergences,
        "failures": run.failures,
        "reach": reach,
    }
    problems = []
    if run.exit_status == PYTEST_EXIT_TESTS_FAILED:
        problems.append(f"{len(run.divergences)} kinds of difference, {len(run.failures)} failed tests")
    if reach["never_called"]:
        problems.append(f"APIs never called: {', '.join(reach['never_called'])}")
    if reach["rarely_returned"]:
        problems.append(f"APIs rarely returning normally: {', '.join(reach['rarely_returned'])}")
    if problems:
        return GateResult("differential", False, "; ".join(problems), details)
    return GateResult(
        "differential", True,
        f"no differences across {details['calls']} calls to {len(run.stats)} APIs",
        details,
    )


def self_comparison_gate(checks_dir, side: dict, *, api=None, seed=0, max_examples=200) -> GateResult:
    """Compare a library with itself. Any difference means it's nondeterministic or the runner is broken."""
    result = differential_gate(
        checks_dir, side, side, api=api, seed=seed, max_examples=max_examples
    )
    return GateResult("self_comparison", result.passed, result.summary, result.details)
