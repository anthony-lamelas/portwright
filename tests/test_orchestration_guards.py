"""Later stages can't quietly weaken what earlier stages established."""

import json
import subprocess
from argparse import Namespace
from types import SimpleNamespace

import pytest

import portwright.orchestrator as orchestrator
from portwright.gates import GateResult
from portwright.gates.rust_quality import vendored_packages
from portwright.orchestrator import Config, Porter


def make_porter(tmp_path, **overrides):
    settings = dict(
        lib="toy", module="toy", source=tmp_path / "src", package_path="toy", tests_path="tests",
        url="https://example.invalid", run_dir=tmp_path / "run",
    )
    settings.update(overrides)
    porter = Porter(Config(**settings), backend=None)
    porter.paths.public.mkdir(parents=True, exist_ok=True)
    (porter.paths.public / "manifest.json").write_text(json.dumps({"api": ["f"]}))
    porter.paths.checks.mkdir(parents=True, exist_ok=True)
    (porter.paths.checks / "test_f.py").write_text("def test_f():\n    pass\n")
    (porter.paths.checks / "regressions.json").write_text(json.dumps([{"target": "f", "args": [], "kwargs": {}}]))
    return porter


@pytest.fixture
def self_comparison(monkeypatch):
    """The checks agree with the original unless a file says otherwise."""
    def gate(checks, reference, **_kwargs):
        broken = any("BROKEN" in path.read_text() for path in checks.glob("*.py"))
        return GateResult("self_comparison", not broken, "differences" if broken else "no differences", {})
    monkeypatch.setattr(orchestrator, "self_comparison_gate", gate)


def run_stage(porter, change):
    def agent(role, label, prompt, mounts, apis=None):
        change(porter.paths.checks)
        return SimpleNamespace(status="ok", text="")
    porter._agent = agent
    return porter._change_locked_checks("strengthen", "self_comparison", "Stage: strengthen.", {})


# ------------------------------------------------------------- strengthening


def test_a_good_strengthening_is_kept_and_locked(tmp_path, self_comparison):
    porter = make_porter(tmp_path)
    result = run_stage(porter, lambda checks: (checks / "test_more.py").write_text("def test_more():\n    pass\n"))
    assert result.passed
    assert (porter.paths.checks / "test_more.py").exists()
    assert "test_more.py" in porter.run_state.read()["checks_hash"]


def test_checks_that_disagree_with_the_original_are_discarded(tmp_path, self_comparison):
    porter = make_porter(tmp_path)
    result = run_stage(porter, lambda checks: (checks / "test_more.py").write_text("# BROKEN\n"))
    assert not result.passed and "discarded" in result.summary
    assert not (porter.paths.checks / "test_more.py").exists()
    assert porter.run_state.read()["gates"]["self_comparison"]["passed"] is False


def test_a_stage_cant_delete_earlier_checks_or_saved_regressions(tmp_path, self_comparison):
    porter = make_porter(tmp_path)
    result = run_stage(porter, lambda checks: (checks / "test_f.py").unlink())
    assert not result.passed and "test_f.py" in result.summary
    assert (porter.paths.checks / "test_f.py").exists(), "restored"

    result = run_stage(porter, lambda checks: (checks / "regressions.json").write_text("[]"))
    assert not result.passed and "regression" in result.summary
    assert json.loads((porter.paths.checks / "regressions.json").read_text()) != []


def test_the_harness_owned_regression_runner_is_restored(tmp_path, self_comparison):
    from portwright.evaluation import REGRESSION_RUNNER

    porter = make_porter(tmp_path)
    run_stage(porter, lambda checks: (checks / "test_porter_regressions.py").write_text("def test_nothing():\n    pass\n"))
    assert (porter.paths.checks / "test_porter_regressions.py").read_text() == REGRESSION_RUNNER


# ------------------------------------------------------------------- fuzzing


def test_the_report_fuzzes_again_when_main_moved_after_hardening(tmp_path, monkeypatch):
    porter = make_porter(tmp_path)
    heads = iter(["aaa", "bbb", "bbb"])
    porter.workspaces = SimpleNamespace(head=lambda: next(heads), commit=lambda *a: None)
    fuzzed = []
    monkeypatch.setattr(orchestrator, "fuzz_gate", lambda *a, **k: fuzzed.append(1) or GateResult("fuzz", True, "ok", {}))
    porter._fuzz()  # hardening fuzzes commit aaa
    assert porter.run_state.read()["fuzzed_commit"] == "aaa"

    porter._verify_main = lambda: SimpleNamespace(site=None)
    porter._holdout_gate = lambda: GateResult("holdout_tests", True, "ok", {})
    monkeypatch.setattr(orchestrator, "run_native_benchmarks", lambda *a, **k: {})
    monkeypatch.setattr(orchestrator, "write_report", lambda *a, **k: None)
    porter._phase_report()  # main is now at bbb
    assert len(fuzzed) == 2
    assert porter.run_state.read()["fuzzed_commit"] == "bbb"


def test_the_report_doesnt_fuzz_again_when_nothing_changed(tmp_path, monkeypatch):
    porter = make_porter(tmp_path)
    porter.workspaces = SimpleNamespace(head=lambda: "aaa", commit=lambda *a: None)
    fuzzed = []
    monkeypatch.setattr(orchestrator, "fuzz_gate", lambda *a, **k: fuzzed.append(1) or GateResult("fuzz", True, "ok", {}))
    porter._fuzz()
    porter._verify_main = lambda: SimpleNamespace(site=None)
    porter._holdout_gate = lambda: GateResult("holdout_tests", True, "ok", {})
    monkeypatch.setattr(orchestrator, "run_native_benchmarks", lambda *a, **k: {})
    monkeypatch.setattr(orchestrator, "write_report", lambda *a, **k: None)
    porter._phase_report()
    assert len(fuzzed) == 1


# ----------------------------------------------------------------- vendoring


def vendorable(tmp_path, profile):
    port = tmp_path / "pycompat" / "ports" / "pc_re"
    core = port / "crates" / "pc-re-port-core"
    (core / "src").mkdir(parents=True)
    (core / "src" / "lib.rs").write_text("pub fn f() {}\n")
    (core / "Cargo.toml").write_text('[package]\nname = "pc-re-port-core"\nversion.workspace = true\n')
    (port / "Cargo.toml").write_text('[workspace]\nmembers = ["crates/pc-re-port-core"]\n\n[profile.release]\n' + profile)
    return core


def test_a_vendored_crate_keeps_its_release_profile(tmp_path):
    from portwright.scaffold import write_scaffold

    core = vendorable(tmp_path, 'overflow-checks = false\nlto = "fat"\ncodegen-units = 1\n')
    porter = make_porter(tmp_path, vendor={"pc_re": str(core)})
    logged = []
    porter._log = logged.append
    write_scaffold(porter.paths.port, "toy", "toy", "MIT", "https://example.invalid", "abc")
    porter._vendor_crates()

    import tomllib
    release = tomllib.loads((porter.paths.port / "Cargo.toml").read_text())["profile"]["release"]
    assert release["lto"] == "fat" and release["codegen-units"] == 1
    assert release["overflow-checks"] is True, "the port's own setting wins"
    assert any("overflow-checks" in line for line in logged)
    assert vendored_packages(porter.paths.port) == ["pc-re-port-core"]


def test_clippy_and_rust_tests_skip_vendored_crates(tmp_path, monkeypatch):
    import portwright.gates.rust_quality as rust_quality

    (tmp_path / "vendor" / "pc_re").mkdir(parents=True)
    (tmp_path / "vendor" / "pc_re" / "Cargo.toml").write_text('[package]\nname = "pc-re-port-core"\n')
    commands = []
    monkeypatch.setattr(rust_quality, "run_in_toolchain",
                        lambda command, *a, **k: commands.append(command) or subprocess.CompletedProcess(command, 0, "", ""))
    assert rust_quality.rust_quality_gate(tmp_path, py_crate_name="toy-port-py", target_dir=tmp_path / "t").passed
    clippy, nextest = commands[1], commands[2]
    assert clippy[clippy.index("--exclude") + 1] == "pc-re-port-core"
    assert "pc-re-port-core" in nextest and "toy-port-py" in nextest


def test_a_resumed_run_refuses_vendor_flags_it_would_ignore(tmp_path):
    from portwright.cli import _load_or_create_config

    porter = make_porter(tmp_path)
    (porter.paths.control / "config.json").write_text(json.dumps({
        "lib": "toy", "module": "toy", "source": str(tmp_path / "src"), "package_path": "toy",
        "tests_path": "tests", "url": "https://example.invalid", "run_dir": str(porter.paths.root),
    }))
    options = Namespace(run_dir=str(porter.paths.root), vendor=["pc_re=/somewhere/pc-re-port-core"], seed=None,
                        max_agents=None, strict_bindings=False, model=None)
    with pytest.raises(SystemExit, match="new --run-dir"):
        _load_or_create_config(options)
    options.vendor = None
    assert _load_or_create_config(options).vendor == {}
