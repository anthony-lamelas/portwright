"""Runs a port from start to finish. Agents do the writing, and this code judges it."""

from __future__ import annotations

import json
import shutil
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path

from portwright.agents import AgentResult, AgentSession, Backend, LimitReached
from portwright.evaluation import Project, Verification, add_regressions, differential_for, verify
from portwright.gates import GateResult
from portwright.gates.bench import load_benchmarks, performance_gate, run_benchmarks, run_native_benchmarks, speedups
from portwright.gates.canaries import canary_gate
from portwright.gates.coverage import coverage_gate, rust_coverage
from portwright.gates.differential import self_comparison_gate
from portwright.gates.fuzz import fuzz_gate
from portwright.gates.integrity import hash_tree
from portwright.gates.license import is_permissive
from portwright.gates.mutation import mutation_gate
from portwright.gates.rust_quality import count_rust_lines
from portwright.gates.upstream_tests import strip_holdouts, upstream_gate
from portwright.manifest import extract_api
from portwright.report import write_report
from portwright.scaffold import crate_names, init_repo, write_scaffold
from portwright.scheduler import BLOCKED, MERGED, RETRY, TaskBoard, Workspaces, run_board
from portwright.state import JsonStore, RunPaths, append_capped, atomic_write

PHASES = ["intake", "checks", "design", "port", "harden", "cleanup", "performance", "final_review", "report"]
LICENSE_NAMES = ("LICENSE", "LICENSE.txt", "LICENSE.md", "LICENCE", "COPYING")


class RunStopped(RuntimeError):
    """The run can't honestly continue. The report says why."""


@dataclass
class Config:
    lib: str
    module: str
    source: Path
    package_path: str
    tests_path: str
    url: str
    run_dir: Path
    # Starting values. The planted-bug minimum gets reset from the first real library.
    canary_minimum: float = 0.7
    canary_sample: int | None = 150
    canary_examples: int = 100
    coverage_minimum: float = 80.0
    check_rounds: int = 3
    design_rounds: int = 2
    max_attempts: int = 5
    debugger_after: int = 2
    fix_rounds: int = 5
    harden_cycles: int = 3
    max_agents: int = 1
    examples: int = 150
    hard_examples: int = 1000
    holdout_fraction: float = 0.2
    fuzz_seconds: int = 120
    mutation_sample: int = 40
    mutation_minimum: float = 0.6
    performance_rounds: int = 3
    max_turns: int = 250
    session_timeout: float = 2 * 60 * 60
    model: str | None = None
    seed: int = 20261101


class Porter:
    def __init__(self, config: Config, backend: Backend):
        self.config = config
        self.backend = backend
        self.paths = RunPaths(Path(config.run_dir).resolve())
        self.paths.create()
        self.run_state = JsonStore(
            self.paths.control / "run.json",
            {"phases_done": [], "gates": {}, "usage": {}, "sessions": 0, "stopped": None,
             "approved_differences": [], "approved_crates": [], "shared": {}, "rejected": []},
        )
        self.board = TaskBoard(self.paths.control / "board.json")
        self.workspaces = Workspaces(self.paths.port, self.paths.worktrees)
        self.core_crate, self.py_crate = crate_names(config.lib)

    # ---------------------------------------------------------------- run loop

    def run(self) -> dict:
        """Run every unfinished phase. Safe to call again after any interruption."""
        for phase in PHASES:
            state = self.run_state.read()
            if phase in state["phases_done"]:
                continue
            if state["stopped"] and phase != "report":
                continue
            self._log(f"phase {phase}")
            try:
                getattr(self, f"_phase_{phase}")()
            except RunStopped as stop:
                self._log(f"stopped: {stop}")
                self.run_state.update(lambda d: d.update(stopped=str(stop)))
                continue
            self.run_state.update(lambda d: d["phases_done"].append(phase))
        return self.run_state.read()

    def _log(self, message: str) -> None:
        line = f"{time.strftime('%H:%M:%S')} {message}"
        print(line, flush=True)
        with open(self.paths.control / "progress.log", "a", encoding="utf-8") as stream:
            stream.write(line + "\n")

    def _record(self, gate: GateResult, label: str | None = None) -> GateResult:
        name = label or gate.name
        self.run_state.update(lambda d: d["gates"].__setitem__(name, gate.to_dict()))
        self._log(f"  {'PASS' if gate.passed else 'FAIL'} {name}: {gate.summary}")
        return gate

    # ---------------------------------------------------------------- shared helpers

    @property
    def reference(self) -> dict:
        import sys

        return {"path": str(self.paths.upstream_src), "module": self.config.module, "python": sys.executable}

    @property
    def api(self) -> list[str]:
        return json.loads((self.paths.public / "manifest.json").read_text())["api"]

    def project(self) -> Project:
        state = self.run_state.read()
        locked = {}
        if state.get("checks_hash"):
            locked["checks"] = {"path": str(self.paths.checks), "hashes": state["checks_hash"]}
        return Project(
            module=self.config.module, lib=self.config.lib,
            core_crate=self.core_crate, py_crate=self.py_crate,
            reference=self.reference, checks=str(self.paths.checks),
            tests_visible=str(self.paths.tests_visible),
            license_text=(self.paths.root / "upstream" / "LICENSE").read_text(encoding="utf-8"),
            locked=locked, shared=state["shared"],
            approved_differences=state["approved_differences"],
            approved_crates=state["approved_crates"],
        )

    def _target(self, name: str) -> Path:
        return self.paths.build / "targets" / name

    def _site(self, name: str) -> Path:
        return self.paths.build / "sites" / name

    def _agent_config(self, apis=None) -> dict:
        state = self.run_state.read()
        return {
            "module": self.config.module, "lib": self.config.lib,
            "core_crate": self.core_crate, "py_crate": self.py_crate,
            "reference": {"path": "/work/upstream/src", "module": self.config.module},
            "api": self.api if (self.paths.public / "manifest.json").exists() else [],
            "apis": apis, "coverage_minimum": self.config.coverage_minimum,
            "approved_differences": state["approved_differences"],
            "approved_crates": state["approved_crates"], "shared": state["shared"],
        }

    def _agent(self, role: str, label: str, prompt: str, mounts: dict[str, str], apis=None) -> AgentResult:
        """Run a session, record its usage, and bring in the notes it left."""
        number = self.run_state.update(lambda d: d.update(sessions=d["sessions"] + 1))["sessions"]
        scratch = self.paths.sessions / f"{number:03d}-{role}-{label}"
        out = scratch / "out"
        out.mkdir(parents=True, exist_ok=True)
        (scratch / "home").mkdir(exist_ok=True)
        atomic_write(scratch / "home" / "porter.json", json.dumps(self._agent_config(apis)))
        all_mounts = {
            str(self.paths.upstream_src): "/work/upstream/src:ro",
            str(self.paths.tests_visible): "/work/upstream/tests:ro",
            str(self.paths.public): "/work/state:ro",
            str(out): "/work/out",
            **mounts,
        }
        self._log(f"  session {number} {role} ({label})")
        result = self.backend.run(
            AgentSession(
                role=role, prompt=prompt, mounts=all_mounts, scratch=scratch,
                max_turns=self.config.max_turns, timeout=self.config.session_timeout, model=self.config.model,
            )
        )

        def add_usage(data):
            totals = data["usage"].setdefault(role, {"sessions": 0})
            totals["sessions"] += 1
            for key, value in result.usage.items():
                totals[key] = totals.get(key, 0) + value
        self.run_state.update(add_usage)
        self._log(f"  session {number} ended: {result.status}, ~${result.usage.get('estimated_api_cost_usd', 0):.2f} API-equivalent")
        self._collect_notes(out, label)
        if result.status == "limit":
            raise LimitReached(result.text)
        return result

    def _collect_notes(self, out: Path, label: str) -> None:
        """Bring agent notes in through harness code, capped, so they can't grow forever."""
        decisions = out / "decisions.md"
        if decisions.exists():
            append_capped(self.paths.public / "decisions.md", decisions.read_text(encoding="utf-8"), 200)
        tried = out / "tried.md"
        if tried.exists():
            append_capped(self.paths.public / "tried" / f"{label}.md", tried.read_text(encoding="utf-8"), 100)
        bugs = out / "upstream_bugs.md"
        if bugs.exists():
            append_capped(self.paths.public / "upstream_bugs.md", bugs.read_text(encoding="utf-8"), 300)
        for kind in ("differences", "requests"):
            folder = out / kind
            if folder.is_dir():
                for item in folder.glob("*.json"):
                    destination = self.paths.control / kind / f"{label}-{item.name}"
                    destination.parent.mkdir(exist_ok=True)
                    shutil.copyfile(item, destination)
        review = out / "review.md"
        if review.exists():
            atomic_write(self.paths.public / "review.md", review.read_text(encoding="utf-8"))

    def _port_mounts(self, workspace: Path, name: str, *, read_only: bool = False) -> dict[str, str]:
        target = self._target(name)
        target.mkdir(parents=True, exist_ok=True)
        (workspace / "target").mkdir(exist_ok=True)  # Docker would create it as root
        return {
            str(workspace): f"/work/port{':ro' if read_only else ''}",
            str(target): "/work/target",
            str(self.paths.checks): "/work/checks:ro",
        }

    def _lock_checks(self) -> None:
        hashes = hash_tree(self.paths.checks)
        self.run_state.update(lambda d: d.update(checks_hash=hashes))

    # ---------------------------------------------------------------- 1. intake

    def _phase_intake(self) -> None:
        config = self.config
        source = Path(config.source).resolve()
        commit = subprocess.run(["git", "rev-parse", "HEAD"], cwd=source, capture_output=True, text=True, check=False).stdout.strip() or "unknown"
        license_text = next(
            (path.read_text(encoding="utf-8") for name in LICENSE_NAMES if (path := source / name).exists()), None
        )
        if license_text is None or not is_permissive(license_text):
            raise RunStopped("the library has no permissive license we recognize")
        upstream = self.paths.root / "upstream"
        for path in (self.paths.upstream_src, self.paths.tests_full, self.paths.tests_visible):
            if path.exists():
                shutil.rmtree(path)
        package = source / config.package_path
        destination = self.paths.upstream_src / config.package_path
        destination.parent.mkdir(parents=True, exist_ok=True)
        if package.is_dir():
            shutil.copytree(package, destination)
        else:
            shutil.copyfile(package, destination)
        (upstream / "LICENSE").write_text(license_text, encoding="utf-8")
        shutil.copytree(source / config.tests_path, self.paths.tests_full)
        holdouts = strip_holdouts(self.paths.tests_full, self.paths.tests_visible, seed=config.seed, fraction=config.holdout_fraction)
        atomic_write(self.paths.control / "holdout_tests.json", json.dumps(holdouts, indent=1))
        api = extract_api(destination if package.is_dir() else self.paths.upstream_src)
        atomic_write(self.paths.public / "manifest.json", json.dumps({"api": api, "module": config.module, "commit": commit, "url": config.url}, indent=1))
        baseline = upstream_gate(self.paths.tests_full, self.reference, self.reference, name="upstream_baseline")
        self._record(baseline)
        if not baseline.passed:
            raise RunStopped(f"the original's own tests don't pass here: {baseline.summary}")
        if not self.paths.port.exists() or not (self.paths.port / ".git").exists():
            write_scaffold(self.paths.port, config.lib, config.module, license_text, config.url, commit)
            init_repo(self.paths.port)
        self.run_state.update(lambda d: d.update(commit=commit, holdout_count=len(holdouts)))
        self._log(f"  {len(api)} public APIs, {len(holdouts)} tests held back, commit {commit[:9]}")

    # ---------------------------------------------------------------- 2. checks

    def _judge_checks(self) -> tuple[bool, str]:
        config = self.config
        self_check = self._record(
            self_comparison_gate(self.paths.checks, self.reference, api=self.api, seed=config.seed, max_examples=config.examples)
        )
        coverage = self._record(
            coverage_gate(self.paths.checks, self.reference, minimum=config.coverage_minimum, seed=config.seed, max_examples=config.examples)
        )
        canaries = self._record(
            canary_gate(
                self.paths.checks, self.paths.upstream_src, config.module,
                minimum=config.canary_minimum, sample=config.canary_sample, seed=config.seed,
                max_examples=config.canary_examples, workers=8,
            )
        )
        feedback = []
        if not self_check.passed:
            details = {k: self_check.details.get(k) for k in ("reach", "divergences", "output")}
            feedback.append(f"Self-comparison: {self_check.summary}\n{json.dumps(details, indent=1)[:4000]}")
        if not coverage.passed:
            feedback.append(f"Coverage: {coverage.summary}\n{json.dumps(coverage.details.get('uncovered'), indent=1)[:3000]}")
        missed = canaries.details.get("missed", [])
        if missed:
            feedback.append(f"Planted bugs: {canaries.summary}\nNot caught:\n{json.dumps(missed[:60], indent=1)}")
        return all(g.passed for g in (self_check, coverage, canaries)), "\n\n".join(feedback)

    def _test_writer_mounts(self, *extra: tuple[str, str]) -> dict[str, str]:
        return {str(self.paths.checks): "/work/checks", **dict(extra)}

    def _phase_checks(self) -> None:
        config = self.config
        api_list = "\n".join(self.api)
        feedback = ""
        passed = False
        for round_number in range(1, config.check_rounds + 1):
            prompt = (
                f"Write the checks for `{config.module}`.\nPublic API from the manifest:\n{api_list}\n"
                f"Python source: /work/upstream/src. Visible upstream tests: /work/upstream/tests.\n"
                f"Write into /work/checks. Round {round_number} of {config.check_rounds}."
            )
            if feedback:
                prompt += f"\n\nThe harness found these gaps in your current checks:\n{feedback}"
            self._agent("test_writer", f"checks{round_number}", prompt, self._test_writer_mounts())
            passed, feedback = self._judge_checks()
            if passed:
                break
        add_regressions(self.paths.checks, {})
        self._lock_checks()
        if not passed:
            raise RunStopped(f"the checks are too weak to trust after {config.check_rounds} rounds")

    # ---------------------------------------------------------------- 3. design

    def _validate_plan(self, plan: dict) -> list[str]:
        problems = []
        covered = {api for task in plan.get("tasks", []) for api in task.get("apis", [])}
        missing = sorted(set(self.api) - covered)
        if missing:
            problems.append(f"these APIs aren't in any task: {', '.join(missing)}")
        ids = {task.get("id") for task in plan.get("tasks", [])}
        for task in plan.get("tasks", []):
            for dependency in task.get("depends_on", []):
                if dependency not in ids:
                    problems.append(f"task {task.get('id')} depends on unknown task {dependency}")
        seen: set = set()
        remaining = list(plan.get("tasks", []))
        while remaining:
            ready = [t for t in remaining if set(t.get("depends_on", [])) <= seen]
            if not ready:
                problems.append("the tasks have a dependency cycle")
                break
            seen |= {t.get("id") for t in ready}
            remaining = [t for t in remaining if t not in ready]
        if not plan.get("tasks"):
            problems.append("the plan has no tasks")
        return problems

    def _phase_design(self) -> None:
        name = "design"
        workspace = self.workspaces.create(name)
        feedback = ""
        plan_path = self.paths.public / "plan.json"
        for round_number in range(1, self.config.design_rounds + 2):
            prompt = (
                f"Plan the port of `{self.config.module}`. Every one of these APIs must be in a task:\n"
                + "\n".join(self.api)
                + "\n\nWrite /work/out/plan.json and shared types in /work/port as your role says."
            )
            if feedback:
                prompt += f"\n\nProblems with your last attempt:\n{feedback}"
            label = f"design{round_number}"
            self._agent("planner", label, prompt, self._port_mounts(workspace, name))
            produced = self.paths.sessions.glob(f"*-planner-{label}/out/plan.json")
            produced = sorted(produced)
            if not produced:
                feedback = "You didn't write /work/out/plan.json."
                continue
            plan = json.loads(produced[-1].read_text())
            problems = self._validate_plan(plan)
            self.workspaces.commit(workspace, "Add design and shared types")
            if problems:
                feedback = "\n".join(problems)
                continue
            target = self._target(name)

            def gate(path: Path) -> tuple[bool, str]:
                result = verify(self.project(), path, self._site(name), apis=[], upstream=False, check_license=False, target_dir=target, max_examples=1)
                ok = all(g.passed for g in result.gates if g.name != "differential")
                return ok, result.for_agent()
            merged = self.workspaces.merge(name, workspace, gate)
            if not merged.merged:
                feedback = merged.detail
                continue
            atomic_write(plan_path, json.dumps(plan, indent=1))
            for task in plan["tasks"]:
                self.board.add(task["id"], task["apis"], task.get("depends_on", []), task.get("notes", ""))
            shared = {}
            for relative in plan.get("shared_paths", []):
                path = self.paths.port / relative
                if path.is_file():
                    import hashlib

                    shared[relative] = hashlib.sha256(path.read_bytes()).hexdigest()
            self.run_state.update(lambda d: d.update(shared=shared))
            self.workspaces.discard(name)
            return
        raise RunStopped(f"no valid plan after {self.config.design_rounds + 1} attempts: {feedback[:500]}")

    # ---------------------------------------------------------------- 4. port

    def _task_prompt(self, task_id: str, task: dict, role: str) -> str:
        parts = [
            f"Task `{task_id}` in the port of `{self.config.module}`.",
            f"APIs to implement:\n" + "\n".join(task["apis"]),
        ]
        if task.get("notes"):
            parts.append(f"Planner's notes: {task['notes']}")
        if task.get("failure"):
            parts.append(f"The harness rejected the last attempt:\n{task['failure']}")
        tried = self.paths.public / "tried" / f"{task_id}.md"
        if tried.exists():
            parts.append(f"Approaches already tried:\n{tried.read_text(encoding='utf-8')}")
        parts.append(f"Check your work with: python -m portwright.agent_check port {' '.join(task['apis'])}")
        return "\n\n".join(parts)

    def _handle_requests(self) -> str:
        """Tell agents what happened to their requests. Frozen files change only through the planner."""
        folder = self.paths.control / "requests"
        if not folder.is_dir():
            return ""
        lines = []
        for path in sorted(folder.glob("*.json")):
            if path.name.startswith("done-"):
                continue
            try:
                request = json.loads(path.read_text())
            except ValueError:
                path.unlink()
                continue
            lines.append(f"Request {request}: recorded for human review; not granted during this run.")
            path.rename(folder / f"done-{path.name}")
        return "\n".join(lines)

    def _work_task(self, task_id: str) -> str | tuple[str, str]:
        task = self.board.tasks()[task_id]
        if task["attempts"] >= self.config.max_attempts:
            return BLOCKED, f"gave up after {task['attempts']} attempts: {task['failure'][-1500:]}"
        role = "debugger" if task["attempts"] >= self.config.debugger_after else "porter"
        workspace = self.workspaces.create(task_id)
        label = f"{task_id}-a{task['attempts'] + 1}"
        result = self._agent(role, label, self._task_prompt(task_id, task, role), self._port_mounts(workspace, task_id), apis=task["apis"])
        self.workspaces.commit(workspace, f"{role}: {task_id}, attempt {task['attempts'] + 1}")
        notes = self._handle_requests()
        if result.status != "ok" and not self.workspaces.changed_since_branch(workspace):
            return RETRY, f"the session ended with {result.status}: {result.text[-800:]}\n{notes}"
        done_apis = [api for t in self.board.tasks().values() if t["status"] == MERGED for api in t["apis"]]
        seed = self.config.seed + task["attempts"] + 1  # inputs the agent's own preview didn't use

        def gate(path: Path) -> tuple[bool, str]:
            verdict = verify(
                self.project(), path, self._site(task_id), apis=[*done_apis, *task["apis"]],
                upstream=False, seed=seed, max_examples=self.config.examples, target_dir=self._target(task_id),
            )
            for item in verdict.gates:
                if item.name == "differential" and not item.passed:
                    add_regressions_later = item.details.get("divergences", {})
                    self._pending_regressions(add_regressions_later)
            return verdict.passed, verdict.for_agent()
        merged = self.workspaces.merge(task_id, workspace, gate)
        if merged.merged:
            self._log(f"  merged {task_id}")
            self.workspaces.discard(task_id)
            shutil.rmtree(self._target(task_id), ignore_errors=True)
            return MERGED
        self._log(f"  rejected {task_id}: {merged.reason}")
        text = merged.detail if merged.reason == "gate" else f"Your work conflicts with work merged since you started:\n{merged.detail}"
        return RETRY, f"{text}\n{notes}"

    def _pending_regressions(self, divergences: dict) -> None:
        pending = self.paths.control / "pending_regressions.json"
        existing = json.loads(pending.read_text()) if pending.exists() else {}
        existing.update(divergences)
        atomic_write(pending, json.dumps(existing))

    def _flush_regressions(self) -> None:
        """Add saved failures to the locked checks. Only harness code may do this."""
        pending = self.paths.control / "pending_regressions.json"
        if not pending.exists():
            return
        add_regressions(self.paths.checks, json.loads(pending.read_text()))
        pending.unlink()
        self._lock_checks()

    def _phase_port(self) -> None:
        run_board(self.board, self._work_task, max_agents=self.config.max_agents, on_error=lambda t, e: self._log(f"  harness error in {t}: {e}"))
        self._flush_regressions()
        unfinished = self.board.unfinished()
        if unfinished:
            summary = {task_id: task["status"] for task_id, task in unfinished.items()}
            raise RunStopped(f"tasks not finished: {summary}")
        self._fix_until("final_parity", lambda: self._verify_main(), "The whole port must match the original now.")

    def _verify_main(self, *, seed: int | None = None, examples: int | None = None) -> Verification:
        verdict = verify(
            self.project(), self.paths.port, self._site("main"), seed=self.config.seed if seed is None else seed,
            max_examples=examples or self.config.examples, target_dir=self._target("main"),
        )
        for gate in verdict.gates:
            self._record(gate, f"main_{gate.name}")
            if gate.name == "differential" and not gate.passed:
                self._pending_regressions(gate.details.get("divergences", {}))
        return verdict

    def _fix_until(self, name: str, check, intro: str) -> Verification:
        """Send failures to a debugger until the check passes or the rounds run out."""
        verdict = check()
        carried: str | None = None  # the last rejected attempt, so its work isn't redone
        rejection = ""
        for round_number in range(1, self.config.fix_rounds + 1):
            if verdict.passed:
                return verdict
            self._flush_regressions()
            task = f"fix-{name}-{round_number}"
            workspace = self.workspaces.create(task, start=carried)
            prompt = f"{intro}\n\nThe main branch fails with:\n{verdict.for_agent()}\n\n"
            if rejection:
                prompt += (
                    "Your workspace already holds the previous attempt. Its fixes are there, so build on them. "
                    f"The harness rejected that attempt, using new random inputs, because:\n{rejection}\n\n"
                )
            prompt += "Check your work with: python -m portwright.agent_check port"
            self._agent("debugger", task, prompt, self._port_mounts(workspace, task))
            self.workspaces.commit(workspace, f"debugger: {task}")
            merged = self.workspaces.merge(task, workspace, lambda path: self._gate_full(path, task))
            if merged.merged:
                carried, rejection = None, ""
            else:
                rejection = merged.detail[-4000:]
                self._log(f"  rejected {task}: {merged.reason}\n{rejection[-1500:]}")
                if merged.reason == "gate":
                    carried = self.workspaces.revision(workspace)
                    self.workspaces.keep(carried)
                else:  # a conflict can't be built on, so start again from main
                    carried = None
            self.workspaces.discard(task)
            shutil.rmtree(self._target(task), ignore_errors=True)
            verdict = check()
        if not verdict.passed:
            raise RunStopped(f"{name} still failing after {self.config.fix_rounds} rounds: {verdict.for_agent()[:1500]}")
        return verdict

    def _gate_full(self, path: Path, name: str) -> tuple[bool, str]:
        verdict = verify(
            self.project(), path, self._site(name), seed=self.config.seed + 7,
            max_examples=self.config.examples, target_dir=self._target(name),
        )
        return verdict.passed, verdict.for_agent()

    # ---------------------------------------------------------------- 5. harden

    def _add_fuzz_targets(self) -> None:
        if (self.paths.port / "fuzz" / "fuzz_targets").is_dir():
            return
        fuzz_out = self.paths.build / "fuzz_out"
        fuzz_out.mkdir(parents=True, exist_ok=True)
        self._agent(
            "test_writer", "fuzz",
            f"Stage: fuzz targets. Write cargo-fuzz targets for `{self.core_crate}` in /work/fuzz (Cargo.toml and "
            f"fuzz_targets/*.rs), one per public entry point that takes untrusted input. Read the Rust in /work/port. "
            f"The path dependency must be ../crates/{self.core_crate}.",
            {str(self.paths.port): "/work/port:ro", str(self.paths.checks): "/work/checks:ro", str(fuzz_out): "/work/fuzz"},
        )
        task = "add-fuzz"
        workspace = self.workspaces.create(task)
        if not (fuzz_out / "Cargo.toml").exists():
            self.workspaces.discard(task)
            return
        shutil.copytree(fuzz_out, workspace / "fuzz", ignore=shutil.ignore_patterns("target", "corpus", "artifacts"), dirs_exist_ok=True)
        (workspace / "fuzz" / ".gitignore").write_text("target/\ncorpus/\nartifacts/\ncoverage/\n")
        self.workspaces.commit(workspace, "test_writer: add fuzz targets")
        self.workspaces.merge(task, workspace, lambda path: (True, ""))
        self.workspaces.discard(task)
        shared = {f"fuzz/{key}": value for key, value in hash_tree(self.paths.port / "fuzz").items() if key.split("/")[0] in ("fuzz_targets", "Cargo.toml")}
        self.run_state.update(lambda d: d["shared"].update(shared))

    def _phase_harden(self) -> None:
        config = self.config
        self._add_fuzz_targets()
        for cycle in range(1, config.harden_cycles + 1):
            self._log(f"  harden cycle {cycle}")
            seed = config.seed + 1000 * cycle
            self._fix_until(
                f"long{cycle}",
                lambda: self._verify_main(seed=seed, examples=config.hard_examples),
                "A long run with new random inputs found differences. Fix the Rust.",
            )
            holdout = self._record(self._holdout_gate(), "holdout_tests")
            fuzz = self._record(fuzz_gate(self.paths.port, seconds_per_target=config.fuzz_seconds, target_dir=self._target("fuzz")))
            problems = []
            if not holdout.passed:
                # Names only: the tests themselves stay hidden.
                problems.append("Held-back upstream tests fail, which suggests fitting to visible tests:\n" + "\n".join(holdout.details.get("failed", [])))
            if not fuzz.passed:
                problems.append(f"Fuzzing found crashes:\n{json.dumps(fuzz.details.get('crashes', {}), indent=1)[:5000]}")
            if problems:
                text = "\n\n".join(problems)
                task = f"fix-harden-{cycle}"
                workspace = self.workspaces.create(task)
                self._agent("debugger", task, f"{text}\n\nCheck your work with: python -m portwright.agent_check port", self._port_mounts(workspace, task))
                self.workspaces.commit(workspace, f"debugger: {task}")
                self.workspaces.merge(task, workspace, lambda path: self._gate_full(path, task))
                self.workspaces.discard(task)
                continue
            mutation = self._record(
                mutation_gate(
                    self.paths.port, self.paths.checks, self.reference, f"crates/{self.py_crate}", self.paths.build / "mutation",
                    sample=config.mutation_sample, minimum=config.mutation_minimum, seed=seed, target_dir=self._target("mutation"),
                )
            )
            missed = mutation.details.get("missed", [])
            if missed and cycle < config.harden_cycles:
                self._strengthen(missed)
                continue
            break
        self._record(self._holdout_gate(), "holdout_tests")
        self._record(
            rust_coverage(self.paths.port, self.paths.checks, self.paths.upstream_src, config.module, f"crates/{self.py_crate}", target_dir=self._target("coverage"))
        )
        self._review("harden")

    def _holdout_gate(self) -> GateResult:
        site = self._site("main")
        if not list(site.rglob("*.so")):
            self._verify_main()
        return upstream_gate(self.paths.tests_full, self.reference, {**self.reference, "path": str(site)}, name="holdout_tests")

    def _strengthen(self, missed: list[dict]) -> None:
        """Undetected Rust bugs mean a blind spot, so the test writer fixes them, not the porter."""
        self._agent(
            "test_writer", "strengthen",
            "Stage: strengthen. These bugs planted in the Rust weren't caught. Add input rules to /work/checks so "
            "they would be, or explain in /work/out/tried.md why a change can't alter behavior. Don't weaken anything.\n"
            + json.dumps(missed, indent=1),
            {str(self.paths.checks): "/work/checks", str(self.paths.port): "/work/port:ro"},
        )
        self._record(self_comparison_gate(self.paths.checks, self.reference, api=self.api, seed=self.config.seed, max_examples=self.config.examples))
        self._lock_checks()

    def _review(self, label: str) -> None:
        self._agent(
            "reviewer", f"review-{label}",
            "Review the Rust in /work/port. Write /work/out/review.md. You can't change code.",
            {str(self.paths.port): "/work/port:ro", str(self.paths.checks): "/work/checks:ro"},
        )

    # ---------------------------------------------------------------- 6. optimize

    def _try_change(self, task: str, role: str, prompt: str, extra_gate) -> GateResult:
        """Let an agent change the code. It merges only if every gate still passes."""
        self.workspaces.discard(task)
        workspace = self.workspaces.create(task)
        self._agent(role, task, prompt, self._port_mounts(workspace, task))
        if not self.workspaces.commit(workspace, f"{role}: {task}") and not self.workspaces.changed_since_branch(workspace):
            self.workspaces.discard(task)
            return GateResult(role, False, "the agent changed nothing", {})
        outcome: dict = {}

        def gate(path: Path) -> tuple[bool, str]:
            verdict = verify(
                self.project(), path, self._site(task), seed=self.config.seed + 11,
                max_examples=self.config.examples, target_dir=self._target(task),
            )
            if not verdict.passed:
                outcome["gate"] = GateResult(role, False, f"parity broke: {verdict.for_agent()[:600]}", {})
                return False, verdict.for_agent()
            extra = extra_gate(path, verdict)
            outcome["gate"] = extra
            return extra.passed, extra.summary
        merged = self.workspaces.merge(task, workspace, gate)
        result = outcome.get("gate") or GateResult(role, False, merged.detail[:500] or "the merge failed", {})
        if not merged.merged:
            self.run_state.update(lambda d: d["rejected"].append({"task": task, "reason": result.summary}))
        self.workspaces.discard(task)
        shutil.rmtree(self._target(task), ignore_errors=True)
        return result

    def _phase_cleanup(self) -> None:
        # Shared Rust files were frozen so that parallel porters couldn't collide. Porting
        # is over now, and workarounds for the freeze are exactly what cleanup should remove.
        self.run_state.update(lambda d: d.update(shared={k: v for k, v in d["shared"].items() if not k.startswith("crates/")}))
        before = count_rust_lines(self.paths.port)
        review = self.paths.public / "review.md"
        findings = review.read_text(encoding="utf-8") if review.exists() else "No review available."

        def simpler(path: Path, _verdict: Verification) -> GateResult:
            after = count_rust_lines(path)
            passed = after <= before * 1.05
            return GateResult("cleanup", passed, f"Rust lines went from {before} to {after} (5% growth allowed)", {"before": before, "after": after})
        prompt = f"Make the Rust simpler and more idiomatic without changing behavior.\n\nReviewer's findings:\n{findings}\n\nCheck your work with: python -m portwright.agent_check port"
        self._record(self._try_change("cleanup", "cleanup", prompt, simpler))

    def _phase_performance(self) -> None:
        benchmarks = load_benchmarks(self.paths.checks)
        if not benchmarks:
            self._record(GateResult("performance", False, "no benchmarks were written", {}))
            return
        verdict = self._verify_main()
        if not verdict.passed:
            raise RunStopped("parity broke before performance work")
        for round_number in range(1, self.config.performance_rounds + 1):
            baseline = run_benchmarks({**self.reference, "path": verdict.site}, benchmarks)
            prompt = (
                "Make the port faster without changing behavior. Profile first, then change the slowest code. "
                f"Benchmarks through Python are in /work/checks/benchmarks.json. Current nanoseconds per call:\n"
                f"{json.dumps({k: v.get('median_ns') for k, v in baseline.items()}, indent=1)}\n"
                f"Also keep Criterion benchmarks in crates/{self.core_crate}/benches.\n"
                "Check your work with: python -m portwright.agent_check port"
            )

            def faster(_path: Path, candidate: Verification) -> GateResult:
                return performance_gate(baseline, run_benchmarks({**self.reference, "path": candidate.site}, benchmarks))
            task = f"performance-{round_number}"
            result = self._record(self._try_change(task, "performance", prompt, faster), task)
            if not result.passed:
                break  # keep going only while it's still finding real gains
            verdict = self._verify_main()

    # ---------------------------------------------------------------- 7. final review and report

    def _phase_final_review(self) -> None:
        self._review("final")

    def _phase_report(self) -> None:
        state = self.run_state.read()
        benchmarks = load_benchmarks(self.paths.checks) if self.paths.checks.exists() else []
        speed = {}
        native = {}
        finished = not state["stopped"]
        if finished:
            verdict = self._verify_main()
            self._record(self._holdout_gate(), "holdout_tests")
            if benchmarks and verdict.site:
                reference = run_benchmarks(self.reference, benchmarks)
                port = run_benchmarks({**self.reference, "path": verdict.site}, benchmarks)
                speed = {"reference": reference, "port": port, "speedups": speedups(reference, port)}
            native = run_native_benchmarks(self.paths.port, self.core_crate, target_dir=self._target("main"))
        write_report(self.paths, self.config, self.run_state.read(), self.board.tasks(), speed, native)
        self.workspaces.commit(self.paths.port, "Add conformance report")
