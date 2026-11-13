"""A port that vendors finished crates is judged on its own code, and can't change theirs."""

from pathlib import Path

from portwright.evaluation import _shared_problems
from portwright.gates.coverage import _percent
from portwright.gates.integrity import hash_tree
from portwright.gates.rust_quality import count_rust_lines
from portwright.orchestrator import Config, Porter


def write(path: Path, text: str) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)
    return path


def vendored_workspace(tmp_path):
    workspace = tmp_path / "port"
    write(workspace / "crates" / "lib-port-core" / "src" / "lib.rs", "fn a() {}\nfn b() {}\n")
    write(workspace / "vendor" / "pc_re" / "src" / "lib.rs", "fn c() {}\n" * 500)
    write(workspace / "vendor" / "pc_re" / "Cargo.toml", "[package]\nname = 'pc-re-port-core'\n")
    shared = {f"vendor/pc_re/{key}": value for key, value in hash_tree(workspace / "vendor" / "pc_re").items()}
    return workspace, shared


def test_vendored_lines_dont_count_toward_the_ports_size(tmp_path):
    workspace, _ = vendored_workspace(tmp_path)
    assert count_rust_lines(workspace) == 2


def test_line_counts_dont_depend_on_where_the_run_lives(tmp_path):
    workspace, _ = vendored_workspace(tmp_path / "target" / "runs")
    assert count_rust_lines(workspace) == 2


def test_an_untouched_vendored_crate_passes(tmp_path):
    workspace, shared = vendored_workspace(tmp_path)
    assert _shared_problems(workspace, shared) == []


def test_editing_or_adding_files_in_a_vendored_crate_is_caught(tmp_path):
    workspace, shared = vendored_workspace(tmp_path)
    write(workspace / "vendor" / "pc_re" / "build.rs", "fn main() {}\n")
    problems = _shared_problems(workspace, shared)
    assert any("vendor/pc_re/build.rs was added" in p for p in problems), problems

    workspace, shared = vendored_workspace(tmp_path / "again")
    write(workspace / "vendor" / "pc_re" / "src" / "lib.rs", "fn changed() {}\n")
    assert any("vendor/pc_re/src/lib.rs is frozen" in p for p in _shared_problems(workspace, shared))


def test_new_files_in_the_ports_own_crates_are_still_allowed(tmp_path):
    workspace, shared = vendored_workspace(tmp_path)
    shared["crates/lib-port-core/Cargo.toml"] = "x"
    write(workspace / "crates" / "lib-port-core" / "Cargo.toml", "x")
    write(workspace / "crates" / "lib-port-core" / "src" / "new.rs", "fn n() {}\n")
    problems = _shared_problems(workspace, shared)
    assert not any("new.rs" in p for p in problems), problems


def test_coverage_of_the_ports_own_files_is_weighted_by_size():
    files = [
        {"summary": {"lines": {"count": 100, "covered": 90}}},
        {"summary": {"lines": {"count": 300, "covered": 150}}},
    ]
    assert _percent(files, "lines") == 60.0
    assert _percent([], "lines") == 0.0


def test_a_vendored_pycompat_crate_brings_its_conformance_report(tmp_path):
    port = tmp_path / "pycompat" / "ports" / "pc_re"
    core = port / "crates" / "pc-re-port-core"
    write(core / "src" / "lib.rs", "fn c() {}\n")
    write(port / "CONFORMANCE.md", "# pc_re conformance\n")
    config = Config(
        lib="inflection", module="inflection", source=tmp_path / "src", package_path="inflection",
        tests_path="tests", url="https://example.invalid", run_dir=tmp_path / "run", vendor={"pc_re": str(core)},
    )
    porter = Porter(config, backend=None)
    porter._vendor_crates()
    copied = porter.paths.port / "vendor" / "pc_re" / "CONFORMANCE.md"
    assert copied.read_text() == "# pc_re conformance\n"
    assert "vendor/pc_re/CONFORMANCE.md" in porter.run_state.read()["shared"], "the report is frozen too"
    assert "CONFORMANCE.md" in porter._vendor_note()
