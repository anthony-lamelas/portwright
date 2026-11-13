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

from portwright.gates import GateResult
from portwright.gates.differential import PYTEST_EXIT_OK, PYTEST_EXIT_TESTS_FAILED, run_checks
from portwright.toolchain import build_python_package, run_in_toolchain


def list_rust_mutants(workspace, *, target_dir=None) -> list[dict]:
    done = run_in_toolchain(
        # Vendored crates are frozen and were mutation-tested in their own ports.
        ["cargo", "mutants", "--list", "--json", "--diff", "--workspace", "--exclude", "fuzz/**", "--exclude", "vendor/**"],
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
    parallel: int = 3,
) -> GateResult:
    """Fail if the checks notice too few of the sampled Rust mutants.

    The sample is split across ``parallel`` workers, each with its own copy of the
    source, build directory and package folder, so they never share files.
    """
    from concurrent.futures import ThreadPoolExecutor

    work_dir = Path(work_dir).resolve()
    copy = work_dir / "source"
    if copy.exists():
        shutil.rmtree(copy)
    shutil.copytree(workspace, copy, ignore=shutil.ignore_patterns("target", ".git"))
    mutants = list_rust_mutants(copy, target_dir=target_dir)
    if len(mutants) > sample:
        mutants = random.Random(seed).sample(mutants, sample)
    workers = max(1, min(parallel, len(mutants)))

    def work(index: int) -> list[tuple[str, dict | None]]:
        mine = work_dir / f"source-{index}"
        if mine.exists():
            shutil.rmtree(mine)
        shutil.copytree(copy, mine)
        site = work_dir / f"site-{index}"
        target = Path(target_dir) / f"mutant-{index}" if target_dir else None
        results = []
        for mutant in mutants[index::workers]:
            diff = mutant["diff"]
            if not _apply(diff, mine, mutant["file"]):
                results.append(("unviable", None))
                continue
            try:
                built = build_python_package(mine, crate, site, target_dir=target)
                if built.returncode != 0:
                    results.append(("unviable", None))
                    continue
                run = run_checks(
                    checks_dir, reference, {**reference, "path": str(site)},
                    seed=seed, max_examples=max_examples, stop_at_first=True, timeout=5.0,
                )
            finally:
                if not _apply(diff, mine, mutant["file"], reverse=True):
                    raise RuntimeError(f"couldn't undo mutant in {mutant['file']}")
            label = {
                "file": mutant["file"],
                "line": mutant.get("span", {}).get("start", {}).get("line"),
                "function": (mutant.get("function") or {}).get("function_name"),
                "change": mutant.get("replacement", ""),
            }
            if run.exit_status == PYTEST_EXIT_TESTS_FAILED and run.divergences:
                results.append(("caught", label))
            elif run.exit_status == PYTEST_EXIT_OK:
                results.append(("missed", label))
            else:
                results.append(("unviable", None))
        return results

    caught, missed, unviable = [], [], 0
    with ThreadPoolExecutor(max_workers=workers) as pool:
        for results in pool.map(work, range(workers)):
            for verdict, label in results:
                if verdict == "caught":
                    caught.append(label)
                elif verdict == "missed":
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
