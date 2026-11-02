"""Runs fuzz targets against the Rust core to look for panics and hangs."""

from __future__ import annotations

from pathlib import Path

from portwright.gates import GateResult
from portwright.toolchain import run_in_toolchain


def list_fuzz_targets(workspace) -> list[str]:
    return sorted(path.stem for path in (Path(workspace) / "fuzz" / "fuzz_targets").glob("*.rs"))


def fuzz_gate(workspace, *, seconds_per_target: int = 60, target_dir=None) -> GateResult:
    """Fail if there are no targets, or any target finds a crash."""
    targets = list_fuzz_targets(workspace)
    if not targets:
        return GateResult("fuzz", False, "no fuzz targets", {})
    crashes = {}
    for target in targets:
        done = run_in_toolchain(
            [
                "cargo", "+nightly", "fuzz", "run", target,
                "--", f"-max_total_time={seconds_per_target}", "-timeout=10", "-rss_limit_mb=4096",
            ],
            workspace, target_dir=target_dir, timeout=seconds_per_target + 900,
        )
        if done.returncode != 0:
            crashes[target] = (done.stdout + done.stderr)[-1500:]
    if crashes:
        return GateResult("fuzz", False, f"crashes in {', '.join(crashes)}", {"crashes": crashes, "targets": targets})
    return GateResult(
        "fuzz", True, f"no crashes in {len(targets)} targets, {seconds_per_target}s each",
        {"targets": targets, "seconds_per_target": seconds_per_target},
    )
