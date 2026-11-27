"""Run real projects that use a library against its port, to find behavior the gates miss.

This is a report, not a gate: it never changes a run's result and it exits 0 whatever it
finds. For each dependent listed in ``tools/dependents/<lib>.json`` it builds fresh venvs
(the same Python as the gates), installs the dependent and its locked dependencies, and
runs the dependent's own pytest suite:

- twice with the original library, at the commit the port was made from, so flaky tests
  can be told apart from real differences;
- once (``--port-runs`` to change it) with the port's wheel, installed under the same
  import name.

A test is flagged only if it passed in every original run and failed or errored with
the port. Tests that fail with the original, or flip between the two original runs, are
counted but not flagged.

    python tools/dependents/run.py --lib parse           # builds the wheel in the toolchain image
    python tools/dependents/run.py --lib parse --wheel target/dependents/wheel/parse_rust_port-....whl
    python tools/dependents/run.py --lib parse --lock    # regenerate the lock files

It runs on the host and needs git, uv, and network access to PyPI and GitHub; building
the wheel also needs Docker and the toolchain image. Dependents' tests run unsandboxed on
the host, so list only projects you'd run the tests of anyway.
"""

from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import xml.etree.ElementTree as ElementTree
from dataclasses import dataclass, field
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
IMAGE = "portwright-toolchain:dev"
TEST_TIMEOUT = 30 * 60
FAILING = ("failed", "error")
# Worst outcome wins when a test has more than one entry (a setup error, then a teardown one).
RANK = {"passed": 0, "skipped": 1, "failed": 2, "error": 3}


def normalize(name: str) -> str:
    return re.sub(r"[-_.]+", "-", name).lower()


def run(command: list[str], **kwargs) -> subprocess.CompletedProcess:
    return subprocess.run(command, check=True, text=True, **kwargs)


def checkout(url: str, commit: str, path: Path) -> Path:
    """A clone of ``url`` at exactly ``commit``, reused between runs."""
    if not (path / ".git").is_dir():
        shutil.rmtree(path, ignore_errors=True)
        run(["git", "clone", "-q", url, str(path)])
    head = subprocess.run(["git", "-C", str(path), "rev-parse", "HEAD"], capture_output=True, text=True).stdout.strip()
    if head != commit:
        run(["git", "-C", str(path), "fetch", "-q", "origin", commit], capture_output=True)
        run(["git", "-C", str(path), "checkout", "-q", "--detach", commit])
    run(["git", "-C", str(path), "clean", "-qfdx"])
    return path


def build_wheel(port_dir: Path, out: Path) -> Path:
    """Build the port's Python package in the toolchain image, as the build gate does."""
    source = out / "port-src"
    shutil.rmtree(source, ignore_errors=True)
    shutil.copytree(port_dir, source, ignore=shutil.ignore_patterns("target"))
    wheels = out / "wheel"
    shutil.rmtree(wheels, ignore_errors=True)
    wheels.mkdir(parents=True)
    bindings = sorted(source.glob("crates/*-port-py/Cargo.toml"))
    if len(bindings) != 1:
        sys.exit(f"expected one bindings crate in {port_dir}/crates, found {len(bindings)}")
    run([
        "docker", "run", "--rm", "--user", f"{os.getuid()}:{os.getgid()}", "-e", "HOME=/tmp",
        "-v", f"{source}:/src", "-v", f"{wheels}:/out", "-w", "/src", IMAGE,
        "maturin", "build", "--release", "-m", str(bindings[0].relative_to(source)), "-o", "/out",
    ])
    return next(wheels.glob("*.whl"))


@dataclass
class Variant:
    label: str  # original-1, original-2, port-1, ...
    is_port: bool


@dataclass
class Outcome:
    variant: str
    tests: dict[str, tuple[str, str]] = field(default_factory=dict)  # id -> (outcome, message)
    exit_code: int = 0
    problem: str = ""  # set when the run itself broke, e.g. nothing could be collected
    loaded_from: str = ""


def read_junit(path: Path) -> dict[str, tuple[str, str]]:
    tests: dict[str, tuple[str, str]] = {}
    for case in ElementTree.parse(path).getroot().iter("testcase"):
        test_id = f"{case.get('classname', '')}::{case.get('name', '')}"
        outcome, message = "passed", ""
        for tag in ("error", "failure", "skipped"):
            child = case.find(tag)
            if child is not None:
                outcome = {"failure": "failed"}.get(tag, tag)
                message = (child.get("message") or "") + "\n" + (child.text or "")
                break
        previous = tests.get(test_id)
        if previous is None or RANK[outcome] > RANK[previous[0]]:
            tests[test_id] = (outcome, message.strip())
    return tests


class Runner:
    def __init__(self, config: dict, work: Path, uv: str, wheel: Path | None):
        self.config = config
        self.work = work
        self.uv = uv
        self.wheel = wheel
        # A wheel's file name starts with its distribution name, e.g. parse_rust_port-0.1.0-...
        self.port_distribution = wheel.name.split("-")[0] if wheel else ""
        self.original = checkout(config["original"]["git"], config["original"]["commit"], work / "src" / f"{config['lib']}-original")

    def venv(self, path: Path) -> Path:
        shutil.rmtree(path, ignore_errors=True)
        run([self.uv, "venv", "-q", "--python", self.config["python"], str(path)])
        return path / "bin" / "python"

    def install(self, python: Path, *args: str) -> None:
        run([self.uv, "pip", "install", "-q", "--python", str(python), "--no-deps", *args])

    def lock(self, dependent: dict) -> None:
        """Pin the dependent's dependencies and test tools, without the library or the dependent."""
        source = checkout(dependent["git"], dependent["commit"], self.work / "src" / dependent["name"])
        requirements = self.work / f"{dependent['name']}.in"
        requirements.write_text("\n".join([str(source), *dependent.get("test_requirements", [])]) + "\n")
        compiled = run([
            self.uv, "pip", "compile", "-q", "--python-version", self.config["python"][:4],
            "--no-header", "--no-annotate", str(requirements),
        ], capture_output=True).stdout
        excluded = {normalize(self.config["distribution"]), normalize(dependent["name"])}
        lines = []
        for line in compiled.splitlines():
            if line.strip().startswith((str(source), "file:", "-e ")):
                continue  # the dependent itself, compiled from its checkout
            name = re.split(r"[=<>@ \[;]", line.strip(), maxsplit=1)[0]
            if name and normalize(name) not in excluded:
                lines.append(line.strip())
        header = (f"# Exact versions for {dependent['name']} {dependent['version']}'s tests, without "
                  f"{self.config['distribution']} (the original or the port is installed separately).\n"
                  f"# Regenerate with: python tools/dependents/run.py --lib {self.config['lib']} --lock\n")
        (HERE / dependent["lock"]).write_text(header + "\n".join(lines) + "\n")

    def run_variant(self, dependent: dict, source: Path, variant: Variant) -> Outcome:
        place = self.work / "runs" / dependent["name"] / variant.label
        python = self.venv(place / "venv")
        self.install(python, "-r", str(HERE / dependent["lock"]))
        self.install(python, str(source))
        self.install(python, str(self.wheel if variant.is_port else self.original))
        outcome = Outcome(variant.label)
        module = self.config["import_name"]
        # Which installed distribution provides the import name: the original's, or the wheel's.
        probe = (f"import importlib.metadata as m, {module}; "
                 f"print(','.join(m.packages_distributions().get({module!r}, [])), {module}.__file__)")
        owners, _, outcome.loaded_from = subprocess.run(
            [str(python), "-I", "-c", probe], capture_output=True, text=True,
        ).stdout.strip().partition(" ")
        expected = self.port_distribution if variant.is_port else self.config["distribution"]
        if {normalize(owner) for owner in owners.split(",") if owner} != {normalize(expected)}:
            outcome.problem = (f"`import {module}` came from {owners or 'nothing'} ({outcome.loaded_from}), "
                               f"not only {expected}")
            return outcome
        # A fresh copy each run, so nothing one run writes can reach the next.
        tree = place / "tree"
        shutil.rmtree(tree, ignore_errors=True)
        shutil.copytree(source, tree, ignore=shutil.ignore_patterns(".git"))
        junit = place / "junit.xml"
        junit.unlink(missing_ok=True)
        command = [str(python), "-m", "pytest", *dependent.get("tests", []), "-p", "no:cacheprovider", "-q",
                   f"--junitxml={junit}", *dependent.get("pytest_args", [])]
        # The venv's bin first, so tests that run the dependent's own command-line tools find them.
        env = {**os.environ, "PYTHONDONTWRITEBYTECODE": "1", "PYTHONHASHSEED": "0",
               "VIRTUAL_ENV": str(python.parent.parent), "PATH": f"{python.parent}{os.pathsep}{os.environ.get('PATH', '')}"}
        try:
            result = subprocess.run(command, cwd=tree, env=env, capture_output=True, text=True, timeout=TEST_TIMEOUT)
        except subprocess.TimeoutExpired:
            outcome.problem = f"the tests took longer than {TEST_TIMEOUT // 60} minutes"
            return outcome
        (place / "pytest.log").write_text(result.stdout + result.stderr)
        outcome.exit_code = result.returncode
        if junit.exists():
            outcome.tests = read_junit(junit)
        if not outcome.tests:
            tail = "\n".join((result.stdout + result.stderr).strip().splitlines()[-15:])
            outcome.problem = f"no test results (pytest exit {result.returncode}):\n{tail}"
        return outcome

    def run_dependent(self, dependent: dict, original_runs: int, port_runs: int) -> dict:
        source = checkout(dependent["git"], dependent["commit"], self.work / "src" / dependent["name"])
        variants = ([Variant(f"original-{n}", False) for n in range(1, original_runs + 1)]
                    + [Variant(f"port-{n}", True) for n in range(1, port_runs + 1)])
        outcomes = []
        for variant in variants:
            print(f"  {dependent['name']}: {variant.label}", flush=True)
            outcomes.append(self.run_variant(dependent, source, variant))
        return {"dependent": dependent, "outcomes": outcomes}


def classify(result: dict) -> dict:
    originals = [o for o in result["outcomes"] if o.variant.startswith("original")]
    ports = [o for o in result["outcomes"] if o.variant.startswith("port")]
    every = set().union(*(o.tests for o in result["outcomes"]))
    summary = {"tests": len(every), "stable_pass": 0, "original_fails": [], "flaky": [], "flagged": [],
               "port_flaky": [], "missing_in_port": []}
    if any(o.problem for o in originals):
        return summary
    for test in sorted(every):
        seen = [o.tests.get(test, ("missing", ""))[0] for o in originals]
        if len(set(seen)) > 1:
            summary["flaky"].append(test)
            continue
        if seen[0] != "passed":
            if seen[0] in FAILING:
                summary["original_fails"].append(test)
            continue
        summary["stable_pass"] += 1
        port_seen = [o.tests.get(test, ("missing", "")) for o in ports if not o.problem]
        bad = [entry for entry in port_seen if entry[0] in FAILING]
        if port_seen and all(entry[0] == "missing" for entry in port_seen):
            summary["missing_in_port"].append(test)
        elif bad and len(bad) == len(port_seen):
            summary["flagged"].append((test, bad[0][0], bad[0][1]))
        elif bad:
            summary["port_flaky"].append(test)
    return summary


def excerpt(message: str, lines: int = 12) -> str:
    kept = [line for line in message.splitlines() if line.strip()]
    if len(kept) > lines:
        kept = kept[:4] + ["..."] + kept[-(lines - 5):]
    return "\n".join(kept)


def write_report(config: dict, wheel: Path, results: list[dict], path: Path) -> None:
    digest = hashlib.sha256(wheel.read_bytes()).hexdigest()
    repo_commit = subprocess.run(["git", "-C", str(REPO), "rev-parse", "--short", "HEAD"], capture_output=True, text=True).stdout.strip()
    lines = [
        f"# Dependents report: {config['lib']}", "",
        "Real projects' own test suites, run against the original library and against the port. "
        "This is a report, not a gate. A test is flagged only if it passed in every run with the "
        "original and failed with the port.", "",
        f"- **Date:** {datetime.date.today().isoformat()}",
        f"- **Original:** {config['original']['git']} at `{config['original']['commit']}`",
        f"- **Port:** `{config['port']}` at Portwright commit `{repo_commit}`, wheel `{wheel.name}` (sha256 `{digest[:16]}`)",
        f"- **Python:** {config['python']}", "",
        "| Dependent | Version | Tests | Pass with original | Fail with original | Flaky with original | Flagged |",
        "| --- | --- | --- | --- | --- | --- | --- |",
    ]
    details = []
    for result in results:
        dependent, summary = result["dependent"], classify(result)
        lines.append(f"| {dependent['name']} | {dependent['version']} (`{dependent['commit'][:10]}`) | {summary['tests']} | "
                     f"{summary['stable_pass']} | {len(summary['original_fails'])} | {len(summary['flaky'])} | {len(summary['flagged'])} |")
        details += ["", f"## {dependent['name']} {dependent['version']}", ""]
        for outcome in result["outcomes"]:
            counts = {}
            for state, _ in outcome.tests.values():
                counts[state] = counts.get(state, 0) + 1
            shown = ", ".join(f"{counts[k]} {k}" for k in RANK if k in counts) or "no results"
            details.append(f"- `{outcome.variant}`: {shown}; pytest exit {outcome.exit_code}"
                           + (f". **Problem:** {outcome.problem}" if outcome.problem else ""))
        if summary["original_fails"]:
            details += ["", f"Failing with the original too (not flagged): " + ", ".join(f"`{t}`" for t in summary["original_fails"][:20])
                        + (" ..." if len(summary["original_fails"]) > 20 else "")]
        if summary["flaky"]:
            details += ["", "Flaky between the two original runs (not flagged): " + ", ".join(f"`{t}`" for t in summary["flaky"])]
        if summary["port_flaky"]:
            details += ["", "Failed in only some port runs: " + ", ".join(f"`{t}`" for t in summary["port_flaky"])]
        if summary["missing_in_port"]:
            details += ["", "Passed with the original but didn't run with the port: " + ", ".join(f"`{t}`" for t in summary["missing_in_port"])]
        if summary["flagged"]:
            details += ["", "### Flagged", ""]
            for test, state, message in summary["flagged"]:
                details += [f"- `{test}` ({state} with the port)", "", "  ```", *("  " + line for line in excerpt(message).splitlines()), "  ```"]
        elif not any(o.problem for o in result["outcomes"]):
            details += ["", "Nothing flagged."]
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines + details) + "\n")
    raw = [{"dependent": r["dependent"]["name"], "outcomes": [o.__dict__ for o in r["outcomes"]]} for r in results]
    path.with_suffix(".json").write_text(json.dumps(raw, indent=1))


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--lib", required=True, help="reads tools/dependents/<lib>.json")
    parser.add_argument("--wheel", type=Path, help="the port's wheel; built from the config's port folder if left out")
    parser.add_argument("--work", type=Path, default=REPO / "target" / "dependents")
    parser.add_argument("--uv", default=shutil.which("uv") or "uv")
    parser.add_argument("--only", action="append", help="run only this dependent (repeatable)")
    parser.add_argument("--original-runs", type=int, default=2)
    parser.add_argument("--port-runs", type=int, default=1)
    parser.add_argument("--report", type=Path, help="where to write the report (default: <work>/report-<lib>.md)")
    parser.add_argument("--lock", action="store_true", help="regenerate the dependents' lock files and stop")
    options = parser.parse_args(argv)
    if options.original_runs < 2:
        parser.error("--original-runs must be at least 2, to tell flaky tests from differences")
    config = json.loads((HERE / f"{options.lib}.json").read_text())
    options.work.mkdir(parents=True, exist_ok=True)
    dependents = [d for d in config["dependents"] if not options.only or d["name"] in options.only]
    if options.lock:
        runner = Runner(config, options.work, options.uv, None)
        for dependent in dependents:
            runner.lock(dependent)
            print(f"wrote {HERE / dependent['lock']}")
        return 0
    wheel = options.wheel.resolve() if options.wheel else build_wheel(REPO / config["port"], options.work)
    runner = Runner(config, options.work, options.uv, wheel)
    results = [runner.run_dependent(dependent, options.original_runs, options.port_runs) for dependent in dependents]
    report = options.report or options.work / f"report-{options.lib}.md"
    write_report(config, wheel, results, report)
    flagged = sum(len(classify(r)["flagged"]) for r in results)
    print(f"{flagged} flagged; report: {report}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
