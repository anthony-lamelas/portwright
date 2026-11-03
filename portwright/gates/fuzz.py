"""Runs fuzz targets against the Rust core to look for panics and hangs."""

from __future__ import annotations

from pathlib import Path

from portwright.gates import GateResult
from portwright.toolchain import run_in_toolchain


def list_fuzz_targets(workspace) -> list[str]:
    return sorted(path.stem for path in (Path(workspace) / "fuzz" / "fuzz_targets").glob("*.rs"))


def fuzz_gate(workspace, *, seconds_per_target: int = 60, target_dir=None, parallel: int = 3) -> GateResult:
    """Fail if there are no targets, or any target finds a crash.

    All targets are built once, then run ``parallel`` at a time. Each gets 2 GB, so
    three at once stay inside the memory the agents leave free.
    """
    from concurrent.futures import ThreadPoolExecutor

    targets = list_fuzz_targets(workspace)
    if not targets:
        return GateResult("fuzz", False, "no fuzz targets", {})
    built = run_in_toolchain(["cargo", "+nightly", "fuzz", "build"], workspace, target_dir=target_dir, timeout=1800)
    if built.returncode != 0:
        return GateResult("fuzz", False, "fuzz targets don't build", {"output": (built.stdout + built.stderr)[-3000:]})

    def run(target):
        return target, run_in_toolchain(
            [
                "cargo", "+nightly", "fuzz", "run", target,
                "--", f"-max_total_time={seconds_per_target}", "-timeout=10", "-rss_limit_mb=2048",
            ],
            workspace, target_dir=target_dir, timeout=seconds_per_target + 900,
        )

    crashes = {}
    with ThreadPoolExecutor(max_workers=max(1, parallel)) as pool:
        for target, done in pool.map(run, targets):
            if done.returncode != 0:
                crashes[target] = (done.stdout + done.stderr)[-1500:]
    if crashes:
        return GateResult("fuzz", False, f"crashes in {', '.join(crashes)}", {"crashes": crashes, "targets": targets})
    return GateResult(
        "fuzz", True, f"no crashes in {len(targets)} targets, {seconds_per_target}s each",
        {"targets": targets, "seconds_per_target": seconds_per_target},
    )
