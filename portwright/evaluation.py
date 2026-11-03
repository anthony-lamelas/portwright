"""Runs the gates on a port, on the host or inside a sandbox, with the same rules."""

from __future__ import annotations

import hashlib
import json
from dataclasses import dataclass, field
from pathlib import Path

from portwright.gates import GateResult
from portwright.gates.differential import PYTEST_EXIT_OK, PYTEST_EXIT_TESTS_FAILED, run_checks
from portwright.gates.integrity import hash_tree, integrity_gate
from portwright.gates.license import license_gate
from portwright.gates.rust_quality import rust_quality_gate
from portwright.gates.upstream_tests import upstream_gate
from portwright.toolchain import build_python_package

MAX_EXAMPLES_SHOWN = 8
AGENT_TEXT_LIMIT = 8000


@dataclass
class Project:
    """What the gates need to know about one port."""

    module: str
    lib: str
    core_crate: str
    py_crate: str
    reference: dict
    checks: str
    tests_visible: str
    license_text: str | None = None
    locked: dict = field(default_factory=dict)
    shared: dict = field(default_factory=dict)  # relative path to hash; porters can't change these
    approved_differences: list = field(default_factory=list)
    approved_crates: list = field(default_factory=list)
    strict_bindings: bool = False

    @property
    def py_path(self) -> str:
        return f"crates/{self.py_crate}"


@dataclass
class Verification:
    passed: bool
    gates: list[GateResult]
    site: str | None = None

    def for_agent(self) -> str:
        """Short failure text. Full logs stay on disk so agents' context stays small."""
        lines = []
        for gate in self.gates:
            lines.append(f"[{'PASS' if gate.passed else 'FAIL'}] {gate.name}: {gate.summary}")
            if gate.passed:
                continue
            for problem in gate.details.get("problems", [])[:20]:
                lines.append(f"  - {problem}")
            if "output" in gate.details:
                lines.append(gate.details["output"][-3000:])
            for test in gate.details.get("failed", [])[:30]:
                lines.append(f"  - {test}")
            for signature, entry in list(gate.details.get("divergences", {}).items())[:MAX_EXAMPLES_SHOWN]:
                lines.append(f"  - {entry['count']}x {signature}\n    {entry.get('example', '')}")
            hidden = len(gate.details.get("divergences", {})) - MAX_EXAMPLES_SHOWN
            if hidden > 0:
                lines.append(f"  ...and {hidden} more kinds of difference")
            for failure in gate.details.get("other_failures", [])[:3]:
                lines.append(f"  - {failure['test']}: {failure['text'][-800:]}")
        return "\n".join(lines)[-AGENT_TEXT_LIMIT:]


def signature_api(signature: str) -> str:
    return signature.split(" ", 1)[0]


def differential_for(project: Project, site: str, *, apis=None, seed=0, max_examples=150, timeout=10.0) -> GateResult:
    """Compare with the original, counting only differences in ``apis`` while work is partial."""
    candidate = {**project.reference, "path": site}
    run = run_checks(project.checks, project.reference, candidate, seed=seed, max_examples=max_examples, timeout=timeout)
    wanted = None if apis is None else set(apis)
    approved = set(project.approved_differences)
    blocking = {
        signature: entry for signature, entry in run.divergences.items()
        if signature not in approved and (wanted is None or signature_api(signature) in wanted)
    }
    details = {
        "divergences": blocking,
        "all_divergences": len(run.divergences),
        "approved_seen": sorted(set(run.divergences) & approved),
        "calls": sum(counts["calls"] for counts in run.stats.values()),
        "stats": run.stats,
        "not_compared": sum(counts.get("not_compared", 0) for counts in run.stats.values()),
    }
    if run.exit_status not in (PYTEST_EXIT_OK, PYTEST_EXIT_TESTS_FAILED) or not run.stats:
        details["output"] = run.output[-3000:]
        return GateResult("differential", False, f"checks did not run (pytest exit {run.exit_status})", details)
    unexplained = []
    if run.exit_status == PYTEST_EXIT_TESTS_FAILED and not run.divergences:
        unexplained = run.failures
    elif wanted is None and run.failures and not blocking and not details["approved_seen"]:
        unexplained = run.failures
    if unexplained:
        details["other_failures"] = unexplained
        return GateResult("differential", False, f"{len(unexplained)} check failures with no recorded difference", details)
    if blocking:
        return GateResult("differential", False, f"{len(blocking)} kinds of difference", details)
    scope = "all APIs" if wanted is None else f"{len(wanted)} APIs"
    summary = f"no differences for {scope} across {details['calls']} calls"
    if details["not_compared"]:
        summary += f"; {details['not_compared']} not compared because the original timed out or crashed"
    return GateResult("differential", True, summary, details)


def _shared_problems(workspace: Path, shared: dict) -> list[str]:
    problems = []
    current = {}
    for root in {name.split("/")[0] for name in shared}:
        path = workspace / root
        if path.is_dir():
            current.update({f"{root}/{key}": value for key, value in hash_tree(path).items() if not key.startswith("target/")})
        elif path.is_file():  # a frozen top-level file such as Cargo.toml
            current[root] = hashlib.sha256(path.read_bytes()).hexdigest()
    for name, expected in shared.items():
        if current.get(name) != expected:
            problems.append(f"{name} is frozen and can't be changed; file a request instead")
    return problems


def verify(
    project: Project,
    workspace: str | Path,
    site: str | Path,
    *,
    apis=None,
    upstream: bool = True,
    check_license: bool = True,
    seed: int = 0,
    max_examples: int = 150,
    target_dir=None,
) -> Verification:
    """Run gates from cheapest to most expensive, stopping at the first failure."""
    workspace = Path(workspace)
    gates: list[GateResult] = []

    def done(gate: GateResult) -> bool:
        gates.append(gate)
        return gate.passed

    integrity = integrity_gate(
        workspace, project.module, core_crate=project.core_crate,
        locked=project.locked, approved_crates=set(project.approved_crates),
        strict_bindings=project.strict_bindings, bindings_dir=project.py_path,
    )
    shared = _shared_problems(workspace, project.shared)
    if shared:
        integrity.passed = False
        integrity.details.setdefault("problems", []).extend(shared)
        integrity.summary = f"{len(integrity.details['problems'])} integrity problems"
    if not done(integrity):
        return Verification(False, gates)
    if not done(rust_quality_gate(workspace, py_crate_name=project.py_crate, target_dir=target_dir)):
        return Verification(False, gates)
    if check_license and project.license_text is not None:
        if not done(license_gate(workspace, project.license_text, project.lib, target_dir=target_dir)):
            return Verification(False, gates)
    built = build_python_package(workspace, project.py_path, site, target_dir=target_dir)
    if built.returncode != 0:
        gates.append(GateResult("build", False, "the Python package failed to build", {"output": built.stderr[-3000:]}))
        return Verification(False, gates)
    gates.append(GateResult("build", True, "the Python package builds", {}))
    site_text = str(Path(site).resolve())
    if not list(Path(site_text).rglob("*.so")):
        gates.append(GateResult("build", False, "the built package has no compiled module", {}))
        return Verification(False, gates)
    if not done(differential_for(project, site_text, apis=apis, seed=seed, max_examples=max_examples)):
        return Verification(False, gates, site_text)
    if upstream:
        candidate = {**project.reference, "path": site_text}
        if not done(upstream_gate(project.tests_visible, project.reference, candidate)):
            return Verification(False, gates, site_text)
    return Verification(True, gates, site_text)


def add_regressions(checks: str | Path, divergences: dict) -> int:
    """Save failing inputs as permanent tests. Only harness code writes this file."""
    path = Path(checks) / "regressions.json"
    existing = json.loads(path.read_text()) if path.exists() else []
    known = {json.dumps(item, sort_keys=True) for item in existing}
    added = 0
    for entry in divergences.values():
        item = entry.get("input")
        if item and json.dumps(item, sort_keys=True) not in known:
            existing.append(item)
            known.add(json.dumps(item, sort_keys=True))
            added += 1
    if added:
        path.write_text(json.dumps(existing, indent=1))
    runner = Path(checks) / "test_porter_regressions.py"
    if not runner.exists() or runner.read_text() != REGRESSION_RUNNER:  # harness-owned, so keep it current
        runner.write_text(REGRESSION_RUNNER)
    return added


REGRESSION_RUNNER = '''"""Generated by the harness: replays inputs that once found a difference."""

import json
from pathlib import Path

import pytest

from portwright.compare import same
from portwright.compare.worker import Handles, decode

CASES_PATH = Path(__file__).with_name("regressions.json")
CASES = json.loads(CASES_PATH.read_text()) if CASES_PATH.exists() else []


@pytest.mark.parametrize("case", CASES)
def test_regression(case):
    handles = Handles()
    # Callbacks stay as expressions: each worker evaluates them with its own library.
    args = [decode(item, handles, evaluate=False) for item in case["args"]]
    kwargs = {key: decode(item, handles, evaluate=False) for key, item in case["kwargs"].items()}
    same(case["target"], *args, **kwargs)
'''
