"""Formatting, lints, and Rust tests. Lint rules are passed on the command line, so
agents can't weaken them by editing a config file in the port."""

from __future__ import annotations

from pathlib import Path

from portwright.gates import GateResult
from portwright.toolchain import host_target_dir, in_sandbox, run_in_toolchain

CLIPPY_RULES = [
    "-D", "warnings",
    "-D", "clippy::unwrap_used",
    "-D", "clippy::expect_used",
    "-D", "clippy::panic",
    "-D", "clippy::allow_attributes_without_reason",
]
CLIPPY_CONFIG = 'allow-unwrap-in-tests = true\nallow-expect-in-tests = true\nallow-panic-in-tests = true\n'
OUTPUT_LIMIT = 3000


def rust_quality_gate(workspace, *, py_crate_name: str, target_dir=None, timeout: float = 1800.0) -> GateResult:
    """Run rustfmt, Clippy, and the Rust tests. The first failure ends the gate."""
    config_dir = host_target_dir(workspace, target_dir) / "porter-clippy"
    config_dir.mkdir(parents=True, exist_ok=True)
    (config_dir / "clippy.toml").write_text(CLIPPY_CONFIG)
    env = {"CLIPPY_CONF_DIR": str(config_dir) if in_sandbox() else "/work/target/porter-clippy"}
    steps = [
        ("rustfmt", ["cargo", "fmt", "--all", "--check"]),
        ("clippy", ["cargo", "clippy", "--workspace", "--all-targets", "--", *CLIPPY_RULES]),
        ("rust_tests", [
            "cargo", "nextest", "run", "--workspace", "--exclude", py_crate_name,
            "--no-tests=pass", "--cargo-quiet",
        ]),
    ]
    for label, command in steps:
        done = run_in_toolchain(command, workspace, target_dir=target_dir, env=env, timeout=timeout)
        if done.returncode != 0:
            output = (done.stdout + "\n" + done.stderr)[-OUTPUT_LIMIT:]
            return GateResult("rust_quality", False, f"{label} failed", {"step": label, "output": output})
    return GateResult("rust_quality", True, "rustfmt, Clippy, and Rust tests pass", {})


def count_rust_lines(workspace) -> int:
    """Count non-blank Rust lines outside build output and fuzz targets."""
    total = 0
    for path in Path(workspace).rglob("*.rs"):
        if {"target", "fuzz", "seed"} & set(path.parts):
            continue
        total += sum(1 for line in path.read_text(encoding="utf-8").splitlines() if line.strip())
    return total
