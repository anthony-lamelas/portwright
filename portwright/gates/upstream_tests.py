"""Runs the library's own tests, unmodified, against the original and the port."""

from __future__ import annotations

import ast
import hashlib
import os
import shutil
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

from portwright import isolation
from portwright.gates import GateResult

MOCK_NAMES = {"mock", "Mock", "MagicMock", "patch", "monkeypatch"}


def _test_files(tests_dir: Path) -> list[Path]:
    return sorted(path for path in tests_dir.rglob("*.py") if path.name.startswith("test_"))


def _held_out(name: str, seed: int, fraction: float) -> bool:
    digest = hashlib.sha256(f"{seed}:{name}".encode()).digest()
    return digest[0] / 256 < fraction


def strip_holdouts(tests_dir: str | Path, out_dir: str | Path, *, seed: int, fraction: float = 0.2) -> list[str]:
    """Copy tests to ``out_dir`` with some removed, and return the removed names.

    Agents only ever see the copy. The originals stay outside the sandbox.
    """
    tests_dir, out_dir = Path(tests_dir), Path(out_dir)
    shutil.copytree(tests_dir, out_dir, dirs_exist_ok=True)
    removed = []
    for path in _test_files(out_dir):
        tree = ast.parse(path.read_text(encoding="utf-8"))
        changed = False
        for owner in [tree, *[n for n in tree.body if isinstance(n, ast.ClassDef)]]:
            kept = []
            prefix = "" if owner is tree else f"{owner.name}."
            for node in owner.body:
                is_test = isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and node.name.startswith("test")
                if is_test and _held_out(f"{path.name}::{prefix}{node.name}", seed, fraction):
                    removed.append(f"{path.name}::{prefix}{node.name}")
                    changed = True
                else:
                    kept.append(node)
            owner.body = kept or [ast.Pass()]
        if changed:
            path.write_text(ast.unparse(tree), encoding="utf-8")
    return removed


def private_test_names(tests_dir: str | Path) -> set[str]:
    """Find tests that reach into private names or use mocks, which a port can't honor."""
    names = set()
    for path in _test_files(Path(tests_dir)):
        tree = ast.parse(path.read_text(encoding="utf-8"))
        private_imports = {
            alias.asname or alias.name
            for node in ast.walk(tree) if isinstance(node, ast.ImportFrom)
            for alias in node.names if alias.name.startswith("_")
        }
        for function in ast.walk(tree):
            if not isinstance(function, (ast.FunctionDef, ast.AsyncFunctionDef)) or not function.name.startswith("test"):
                continue
            for node in ast.walk(function):
                private = isinstance(node, ast.Attribute) and node.attr.startswith("_") and not node.attr.startswith("__")
                named = isinstance(node, ast.Name) and (node.id in private_imports or node.id in MOCK_NAMES)
                argument = isinstance(node, ast.arg) and node.arg in MOCK_NAMES
                if private or named or argument:
                    names.add(function.name)
                    break
    return names


def run_tests(tests_dir: str | Path, side: dict, *, timeout: float = 900.0) -> dict[str, str]:
    """Run tests against one version and return ``{test id: passed|failed}``."""
    with isolation.scratch("porter-upstream-") as scratch:
        scratch = Path(scratch)
        shutil.copytree(tests_dir, scratch / "tests", dirs_exist_ok=True)
        report = scratch / "report.xml"
        library = str(Path(side["path"]).resolve())
        if isolation.enabled():
            env = {"PYTHONPATH": library}
        else:
            env = {
                key: value for key, value in os.environ.items()
                if not key.startswith(("PORTER_", "PYTEST_"))
            }
            env["PYTHONPATH"] = library
        done = isolation.run(
            [
                isolation.python(side.get("python", sys.executable)), "-m", "pytest", "tests",
                "-q", "-p", "no:cacheprovider", "--import-mode=importlib",
                "-c", os.devnull, "--rootdir", str(scratch), f"--junitxml={report}",
            ],
            cwd=scratch, env=env, timeout=timeout, read=[library],
        )
        if not report.exists():
            raise RuntimeError(f"tests didn't run: {(done.stdout + done.stderr)[-2000:]}")
        outcomes = {}
        for case in ET.parse(report).getroot().iter("testcase"):
            test_id = f"{case.get('classname')}::{case.get('name')}"
            outcomes[test_id] = "failed" if case.find("failure") is not None or case.find("error") is not None else (
                "skipped" if case.find("skipped") is not None else "passed"
            )
        return outcomes


def upstream_gate(tests_dir: str | Path, reference: dict, candidate: dict, *, name: str = "upstream_tests") -> GateResult:
    """Pass if every test that passes on the original also passes on the port.

    Tests using private names or mocks are reported, because they test the original's
    internals. They don't block, and they're never silently dropped.
    """
    baseline = run_tests(tests_dir, reference)
    outcome = run_tests(tests_dir, candidate)
    required = [test for test, status in baseline.items() if status == "passed"]
    if not required:
        return GateResult(name, False, "no upstream test passes on the original", {"baseline": baseline})
    private = private_test_names(tests_dir)

    def is_private(test_id: str) -> bool:
        return test_id.rsplit("::", 1)[-1].split("[")[0] in private

    failed = [test for test in required if outcome.get(test) != "passed"]
    blocking = [test for test in failed if not is_private(test)]
    details = {
        "required": len(required),
        "failed": blocking,
        "private_failed": [test for test in failed if is_private(test)],
        "private_total": sum(1 for test in required if is_private(test)),
        "not_passing_on_original": sorted(set(baseline) - set(required)),
    }
    passed_count = len(required) - len(failed)
    summary = f"{passed_count}/{len(required)} upstream tests pass"
    if details["private_failed"]:
        summary += f" ({len(details['private_failed'])} failures use private internals or mocks)"
    return GateResult(name, not blocking, summary, details)
