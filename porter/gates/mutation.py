"""Plants bugs in the Rust and checks that the differential checks notice.

cargo-mutants normally judges mutants with ``cargo test``. Our real tests are the
Python checks, so it only lists the mutants here and this module runs each one.
"""

from __future__ import annotations

import json
import random
import shutil
import subprocess
from pathlib import Path

from porter.gates import GateResult
from porter.gates.differential import PYTEST_EXIT_OK, PYTEST_EXIT_TESTS_FAILED, run_checks
from porter.toolchain import build_python_package, run_in_toolchain


def list_rust_mutants(workspace, *, target_dir=None) -> list[dict]:
    done = run_in_toolchain(
        ["cargo", "mutants", "--list", "--json", "--diff", "--workspace", "--exclude", "fuzz/**"],
        workspace, target_dir=target_dir,
    )
    if done.returncode != 0:
        raise RuntimeError(f"cargo mutants failed: {done.stderr[-2000:]}")
    return json.loads(done.stdout)


def _apply(diff: str, directory: Path, file: str, *, reverse: bool = False) -> bool:
    """Apply a diff and confirm the file changed.

    ``git apply`` silently does nothing inside another repository's subfolder, which
    made every mutant look missed. A mutant that wasn't applied must never count.
    """
    path = directory / file
    before = path.read_bytes()
    # cargo-mutants writes a description instead of a path on the "+++" line.
    body = diff.split("\n", 2)[2]
    diff = f"--- a/{file}\n+++ b/{file}\n{body}"
    command = ["patch", "-p1", "--quiet", "--no-backup-if-mismatch", "-d", str(directory)]
    if reverse:
        command.append("--reverse")
    done = subprocess.run(command, input=diff, text=True, capture_output=True, check=False)
    return done.returncode == 0 and path.read_bytes() != before


def mutation_gate(
    workspace,
    checks_dir,
    reference: dict,
    crate: str,
    work_dir,
    *,
    sample: int = 40,
    minimum: float = 0.6,
    seed: int = 0,
    max_examples: int = 100,
    target_dir=None,
) -> GateResult:
    """Fail if the checks notice too few of the sampled Rust mutants."""
    work_dir = Path(work_dir).resolve()
    copy = work_dir / "source"
    if copy.exists():
        shutil.rmtree(copy)
    shutil.copytree(workspace, copy, ignore=shutil.ignore_patterns("target", ".git"))
    mutants = list_rust_mutants(copy, target_dir=target_dir)
    if len(mutants) > sample:
        mutants = random.Random(seed).sample(mutants, sample)
    site = work_dir / "site"
    caught, missed, unviable = [], [], 0
    for mutant in mutants:
        diff = mutant["diff"]
        if not _apply(diff, copy, mutant["file"]):
            unviable += 1
            continue
        try:
            built = build_python_package(copy, crate, site, target_dir=target_dir)
            if built.returncode != 0:
                unviable += 1
                continue
            run = run_checks(
                checks_dir, reference, {**reference, "path": str(site)},
                seed=seed, max_examples=max_examples, stop_at_first=True, timeout=5.0,
            )
        finally:
            if not _apply(diff, copy, mutant["file"], reverse=True):
                raise RuntimeError(f"couldn't undo mutant in {mutant['file']}")
        label = {
            "file": mutant["file"],
            "line": mutant.get("span", {}).get("start", {}).get("line"),
            "function": (mutant.get("function") or {}).get("function_name"),
            "change": mutant.get("replacement", ""),
        }
        if run.exit_status == PYTEST_EXIT_TESTS_FAILED and run.divergences:
            caught.append(label)
        elif run.exit_status == PYTEST_EXIT_OK:
            missed.append(label)
        else:
            unviable += 1
    viable = len(caught) + len(missed)
    score = len(caught) / viable if viable else 0.0
    return GateResult(
        "rust_mutation", viable > 0 and score >= minimum,
        f"checks caught {len(caught)}/{viable} Rust mutants ({score:.0%}, minimum {minimum:.0%}); {unviable} not viable",
        {"score": score, "caught": len(caught), "missed": missed, "not_viable": unviable},
    )
