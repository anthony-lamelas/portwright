"""Measures how much of the original the checks reach, and how much of the Rust."""

from __future__ import annotations

import json
from pathlib import Path

import coverage as coverage_api

from portwright import isolation
from portwright.gates import GateResult
from portwright.gates.differential import REPO_ROOT, run_checks
from portwright.toolchain import run_in_toolchain


def coverage_gate(checks_dir, side: dict, *, minimum: float = 80.0, seed: int = 0, max_examples: int = 200) -> GateResult:
    """Fail if the checks reach too little of the original, by branch coverage."""
    with isolation.scratch("porter-coverage-") as scratch:
        data = str(Path(scratch) / "data")
        run = run_checks(
            checks_dir, {**side, "coverage_file": data}, side, seed=seed, max_examples=max_examples,
            writable=[scratch],
        )
        collector = coverage_api.Coverage(data_file=str(Path(scratch) / "combined"), branch=True)
        try:
            collector.combine([str(path) for path in Path(scratch).glob("data.*")], strict=True)
        except coverage_api.CoverageException as error:
            return GateResult("coverage", False, f"no coverage data: {error}", {"output": run.output[-2000:]})
        collector.save()
        report_path = Path(scratch) / "report.json"
        collector.json_report(outfile=str(report_path))
        report = json.loads(report_path.read_text())
    percent = report["totals"]["percent_covered"]
    uncovered = {
        Path(name).name: {
            "missing_lines": info["missing_lines"],
            "missing_branches": info.get("missing_branches", []),
        }
        for name, info in report["files"].items()
        if info["missing_lines"] or info.get("missing_branches")
    }
    return GateResult(
        "coverage", percent >= minimum,
        f"checks reach {percent:.1f}% of the original (minimum {minimum:.0f}%)",
        {"percent": percent, "uncovered": uncovered},
    )


RUST_COVERAGE_SCRIPT = r"""
set -e
export PATH=$PATH
eval "$(cargo llvm-cov show-env --export-prefix)"
cargo llvm-cov clean --workspace
maturin build --release --compatibility linux -i python3.12 -m {crate}/Cargo.toml -o /work/target/cov-wheels
rm -rf /tmp/site && mkdir -p /tmp/site
python - <<'PY'
import glob, zipfile
zipfile.ZipFile(sorted(glob.glob('/work/target/cov-wheels/*.whl'))[-1]).extractall('/tmp/site')
PY
cat > /tmp/config.json <<'JSON'
{config}
JSON
cd /tmp
PORTER_CONFIG=/tmp/config.json PYTHONPATH=/harness python -m pytest /checks -p portwright.compare.pytest_plugin \
  -p no:cacheprovider --rootdir /checks -c /dev/null -q --hypothesis-seed=0 > /tmp/pytest.log 2>&1 || true
cd /work
cargo llvm-cov report --json --summary-only --release --output-path /work/target/rust-coverage.json \
  --ignore-filename-regex '(/opt/cargo|rustc)'
"""


def rust_coverage(workspace, checks_dir, upstream_path, module: str, crate: str, *, target_dir=None, max_examples: int = 100) -> GateResult:
    """Measure which Rust the checks reach. Uncovered Rust may be invented or untested."""
    config = json.dumps(
        {
            "reference": {"path": "/upstream", "module": module, "python": "python"},
            "candidate": {"path": "/tmp/site", "module": module, "python": "python"},
            "results": "/tmp/results.json",
            "max_examples": max_examples,
            "timeout": 30,
        }
    )
    done = run_in_toolchain(
        ["bash", "-c", RUST_COVERAGE_SCRIPT.format(crate=crate, config=config)],
        workspace,
        target_dir=target_dir,
        extra_mounts={str(REPO_ROOT): "/harness:ro", str(checks_dir): "/checks:ro", str(upstream_path): "/upstream:ro"},
    )
    report = (Path(target_dir) if target_dir else Path(workspace) / "target") / "rust-coverage.json"
    if done.returncode != 0 or not report.exists():
        return GateResult("rust_coverage", False, "Rust coverage failed to run", {"output": (done.stdout + done.stderr)[-2000:]})
    data = json.loads(report.read_text())["data"][0]
    # Vendored crates are frozen and were measured in their own ports. Counting their
    # tens of thousands of lines here would swamp the port's own number.
    own = [item for item in data.get("files", []) if not item["filename"].removeprefix("/work/").startswith("vendor/")]
    if len(own) == len(data.get("files", [])):
        lines_percent, functions_percent = data["totals"]["lines"]["percent"], data["totals"]["functions"]["percent"]
    else:
        lines_percent = _percent(own, "lines")
        functions_percent = _percent(own, "functions")
    files = [
        {
            "file": item["filename"].removeprefix("/work/"),
            "lines": item["summary"]["lines"]["percent"],
            "functions": item["summary"]["functions"]["count"],
            "functions_covered": item["summary"]["functions"]["covered"],
        }
        for item in own
    ]
    return GateResult(
        "rust_coverage", True,
        f"checks reach {lines_percent:.1f}% of Rust lines and {functions_percent:.1f}% of functions",
        {"lines": lines_percent, "functions": functions_percent, "files": sorted(files, key=lambda f: f["lines"])},
    )


def _percent(files: list[dict], kind: str) -> float:
    count = sum(item["summary"][kind]["count"] for item in files)
    covered = sum(item["summary"][kind]["covered"] for item in files)
    return 100.0 * covered / count if count else 0.0
