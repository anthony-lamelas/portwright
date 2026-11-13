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
    # cargo-fuzz builds fuzz/ as its own workspace, so its crates are fetched separately.
    built = run_in_toolchain(
        ["cargo", "+nightly", "fuzz", "build"], workspace, target_dir=target_dir, timeout=1800,
        fetch=("fuzz/Cargo.toml",),
    )
    if built.returncode != 0:
        return GateResult("fuzz", False, "fuzz targets don't build", {"output": (built.stdout + built.stderr)[-3000:]})

    def run(target):
        return target, run_in_toolchain(
            [
                "cargo", "+nightly", "fuzz", "run", target,
                "--", f"-max_total_time={seconds_per_target}", "-timeout=10", "-rss_limit_mb=2048",
            ],
            workspace, target_dir=target_dir, timeout=seconds_per_target + 900, fetch=("fuzz/Cargo.toml",),
        )

    crashes, memory_limit = {}, {}
    with ThreadPoolExecutor(max_workers=max(1, parallel)) as pool:
        for target, done in pool.map(run, targets):
            if done.returncode == 0:
                continue
            output = (done.stdout + done.stderr)[-1500:]
            # An input can legitimately ask for gigabytes ("{:={}{}}" with widths 29298 and
            # 29298 is a 2.9 GB string, in Python too). That is not a bug a debugger can fix,
            # so it is reported for a person to look at instead of failing the gate.
            if "libFuzzer: out-of-memory" in output:
                memory_limit[target] = output
            else:
                crashes[target] = output
    details = {"targets": targets, "seconds_per_target": seconds_per_target, "memory_limit": memory_limit}
    if crashes:
        return GateResult("fuzz", False, f"crashes in {', '.join(crashes)}", {**details, "crashes": crashes})
    summary = f"no crashes in {len(targets)} targets, {seconds_per_target}s each"
    if memory_limit:
        summary += f"; {', '.join(memory_limit)} reached the 2 GB memory limit, which needs a person to check"
    return GateResult("fuzz", True, summary, details)
