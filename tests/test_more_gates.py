"""Each gate must fail when it should, and pass on a good port."""

import shutil
import sys
import textwrap
from pathlib import Path

import pytest

from porter.gates.bench import load_benchmarks, performance_gate, run_benchmarks, speedups
from porter.gates.coverage import coverage_gate, rust_coverage
from porter.gates.fuzz import fuzz_gate
from porter.gates.integrity import hash_tree, integrity_gate
from porter.gates.license import is_permissive, license_gate, write_attribution
from porter.gates.mutation import mutation_gate
from porter.gates.rust_quality import count_rust_lines, rust_quality_gate
from porter.gates.upstream_tests import private_test_names, strip_holdouts, upstream_gate
from porter.toolchain import build_python_package, image_available

FIXTURES = Path(__file__).parent / "fixtures"
TEXTKIT = FIXTURES / "textkit"
RUST = FIXTURES / "textkit_rust"
GOOD = FIXTURES / "checks_good"
SIDE = {"path": str(TEXTKIT), "module": "textkit", "python": sys.executable}
REPO_TARGET = Path(__file__).parent.parent / "target" / "gate-tests"

docker = pytest.mark.skipif(not image_available(), reason="toolchain image not built")


def make_upstream_tests(directory):
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "test_lib.py").write_text(textwrap.dedent("""
        import textkit
        from unittest import mock

        def test_public():
            assert textkit.truncate("abc", 2) == "ab"

        def test_uses_private_state():
            stack = textkit.Stack()
            assert stack._items == []

        def test_uses_mock():
            with mock.patch("textkit.slug", return_value="x"):
                assert textkit.slug("a") == "x"

        class TestGroup:
            def test_in_class(self):
                assert textkit.sum_digits(12) == 3
    """))
    return directory


def test_upstream_gate_blocks_public_failures_only(tmp_path):
    tests = make_upstream_tests(tmp_path / "tests")
    broken = tmp_path / "broken" / "textkit"
    broken.parent.mkdir()
    shutil.copytree(TEXTKIT / "textkit", broken)
    source = broken / "__init__.py"
    source.write_text(source.read_text().replace("return text[:n]", "return text[n:]"))
    result = upstream_gate(tests, SIDE, {**SIDE, "path": str(broken.parent)})
    assert not result.passed
    assert any("test_public" in test for test in result.details["failed"])
    assert "test_uses_private_state" in private_test_names(tests)
    assert "test_uses_mock" in private_test_names(tests)


def test_upstream_gate_passes_on_the_original(tmp_path):
    result = upstream_gate(make_upstream_tests(tmp_path / "tests"), SIDE, SIDE)
    assert result.passed, result.summary
    assert result.details["required"] == 4


def test_private_failures_are_reported_but_do_not_block(tmp_path):
    tests = make_upstream_tests(tmp_path / "tests")
    changed = tmp_path / "changed" / "textkit"
    changed.parent.mkdir()
    shutil.copytree(TEXTKIT / "textkit", changed)
    source = changed / "__init__.py"
    source.write_text(source.read_text().replace("self._items", "self._hidden"))
    result = upstream_gate(tests, SIDE, {**SIDE, "path": str(changed.parent)})
    assert result.passed
    assert any("test_uses_private_state" in test for test in result.details["private_failed"])


def test_holdouts_are_removed_from_the_agents_copy(tmp_path):
    tests = make_upstream_tests(tmp_path / "tests")
    removed = strip_holdouts(tests, tmp_path / "visible", seed=3, fraction=0.5)
    assert removed
    visible = (tmp_path / "visible" / "test_lib.py").read_text()
    for name in removed:
        assert name.rsplit(".", 1)[-1].split("::")[-1] not in visible
    assert "test_public" in (tests / "test_lib.py").read_text()


def test_coverage_gate_measures_the_original():
    result = coverage_gate(GOOD, SIDE, minimum=90, max_examples=50)
    assert result.passed, result.summary
    weak = coverage_gate(FIXTURES / "checks_weak", SIDE, minimum=50, max_examples=10)
    assert not weak.passed


def make_port(tmp_path, name="port"):
    port = tmp_path / name
    shutil.copytree(RUST, port, ignore=shutil.ignore_patterns("target"))
    return port


def check_integrity(port, **kwargs):
    return integrity_gate(port, "textkit", core_crate="textkit-core", **kwargs)


def test_integrity_passes_a_clean_port(tmp_path):
    result = check_integrity(make_port(tmp_path))
    assert result.passed, result.details


@pytest.mark.parametrize(
    ("insert", "expected"),
    [
        ('fn a(py: Python<\'_>) { let _ = py.import("textkit"); }', "imports the original"),
        ('fn a(py: Python<\'_>) { let _ = PyModule::import(py, "textkit"); }', "imports the original"),
        ('fn a(py: Python<\'_>) { let _ = py.import("re"); }', "imports Python module re"),
        ('fn a() { let _ = PyModule::from_code; }', "runs Python source"),
        ('const S: &str = include_str!("../../original.py");', "embeds Python source"),
        ('fn a() { let _ = "__import__"; }', "uses dynamic Python"),
    ],
)
def test_integrity_rejects_a_port_that_uses_python_logic(tmp_path, insert, expected):
    port = make_port(tmp_path)
    source = port / "crates/textkit-py/src/lib.rs"
    source.write_text(source.read_text() + "\n" + insert + "\n")
    result = check_integrity(port)
    assert not result.passed
    assert any(expected in problem for problem in result.details["problems"])


def test_integrity_rejects_python_files_pyo3_in_core_and_unsafe_core(tmp_path):
    port = make_port(tmp_path)
    (port / "crates/textkit-py/helper.py").write_text("x = 1")
    manifest = port / "crates/textkit-core/Cargo.toml"
    manifest.write_text(manifest.read_text() + 'pyo3 = "0.29"\n')
    source = port / "crates/textkit-core/src/lib.rs"
    source.write_text(source.read_text().replace("#![forbid(unsafe_code)]", ""))
    problems = " ".join(check_integrity(port).details["problems"])
    assert "Python source" in problems
    assert "can't depend on PyO3" in problems
    assert "forbid(unsafe_code)" in problems


def test_integrity_rejects_unapproved_crates_unless_approved(tmp_path):
    port = make_port(tmp_path)
    manifest = port / "crates/textkit-core/Cargo.toml"
    manifest.write_text(manifest.read_text() + 'textkit-existing-port = "1"\n')
    assert "isn't approved" in str(check_integrity(port).details["problems"])
    assert check_integrity(port, approved_crates={"textkit-existing-port"}).passed


def test_integrity_rejects_edited_or_added_check_files(tmp_path):
    port = make_port(tmp_path)
    checks = tmp_path / "checks"
    shutil.copytree(GOOD, checks)
    locked = {"checks": {"path": str(checks), "hashes": hash_tree(checks)}}
    assert check_integrity(port, locked=locked).passed
    (checks / "test_textkit.py").write_text("# weakened")
    (checks / "self_made_fixture.json").write_text("{}")
    problems = str(check_integrity(port, locked=locked).details["problems"])
    assert "test_textkit.py" in problems
    assert "self_made_fixture.json" in problems


def test_license_detection():
    assert is_permissive("Permission is hereby granted, free of charge, to any person")
    assert not is_permissive("GNU GENERAL PUBLIC LICENSE")


def test_performance_gate_needs_a_real_gain_and_no_regressions():
    fast = {"kind": "benchmark", "median_ns": 100}
    slow = {"kind": "benchmark", "median_ns": 200}
    similar = {"kind": "benchmark", "median_ns": 102}
    assert performance_gate({"a": slow}, {"a": fast}).passed
    assert not performance_gate({"a": fast}, {"a": similar}).passed, "noise isn't a gain"
    assert not performance_gate({"a": slow, "b": fast}, {"a": fast, "b": slow}).passed
    assert not performance_gate({"a": slow}, {"a": {"kind": "raised"}}).passed


def test_benchmarks_run_through_python(tmp_path):
    (tmp_path / "benchmarks.json").write_text('[{"name": "cut", "target": "truncate", "args": ["abcdef", 3]}]')
    benches = load_benchmarks(tmp_path)
    results = run_benchmarks(SIDE, benches, samples=3)
    assert results["cut"]["kind"] == "benchmark"
    assert speedups(results, results)["cut"] == 1.0


@pytest.fixture(scope="module")
def rust_workspace():
    REPO_TARGET.mkdir(parents=True, exist_ok=True)
    port = REPO_TARGET / "port"
    if port.exists():
        shutil.rmtree(port)
    shutil.copytree(RUST, port, ignore=shutil.ignore_patterns("target"))
    return port


@docker
def test_rust_quality_passes_then_catches_each_problem(rust_workspace):
    result = rust_quality_gate(rust_workspace, py_crate_name="textkit-py")
    assert result.passed, result.details
    source = rust_workspace / "crates/textkit-core/src/lib.rs"
    original = source.read_text()
    source.write_text(original + "\npub fn f(x: Option<u8>) -> u8 { x.unwrap() }\n")
    try:
        bad = rust_quality_gate(rust_workspace, py_crate_name="textkit-py")
    finally:
        source.write_text(original)
    assert not bad.passed
    assert bad.details["step"] in {"rustfmt", "clippy"}


def test_line_count_ignores_build_output(tmp_path):
    (tmp_path / "target").mkdir()
    (tmp_path / "target/a.rs").write_text("fn a() {}\n")
    (tmp_path / "b.rs").write_text("fn b() {}\n\n")
    assert count_rust_lines(tmp_path) == 1


@docker
def test_license_gate(rust_workspace):
    assert not license_gate(rust_workspace, "Permission is hereby granted, free of charge", "textkit").passed
    write_attribution(rust_workspace, "Permission is hereby granted, free of charge", "textkit", "https://example.invalid", "abc")
    result = license_gate(rust_workspace, "Permission is hereby granted, free of charge", "textkit")
    assert result.passed, result.details


@docker
def test_rust_mutation_finds_checks_catch_most_mutants(rust_workspace):
    result = mutation_gate(
        rust_workspace, GOOD, SIDE, "crates/textkit-py", REPO_TARGET / "mutation",
        sample=6, minimum=0.5, max_examples=100,
    )
    assert result.passed, f"{result.summary}\n{result.details}"


@docker
def test_rust_coverage_reports_numbers(rust_workspace):
    result = rust_coverage(rust_workspace, GOOD, TEXTKIT, "textkit", "crates/textkit-py", max_examples=30)
    assert result.passed, result.details
    assert result.details["lines"] > 50


@docker
def test_fuzz_gate_needs_targets_and_finds_crashes(rust_workspace):
    assert not fuzz_gate(rust_workspace).passed
    fuzz = rust_workspace / "fuzz"
    (fuzz / "fuzz_targets").mkdir(parents=True)
    (fuzz / "Cargo.toml").write_text(textwrap.dedent("""
        [package]
        name = "textkit-fuzz"
        version = "0.0.0"
        edition = "2021"
        publish = false
        [package.metadata]
        cargo-fuzz = true
        [dependencies]
        libfuzzer-sys = "0.4"
        textkit-core = { path = "../crates/textkit-core" }
        [workspace]
        [[bin]]
        name = "digits"
        path = "fuzz_targets/digits.rs"
        test = false
        doc = false
    """))
    target = fuzz / "fuzz_targets/digits.rs"
    target.write_text(textwrap.dedent("""
        #![no_main]
        use libfuzzer_sys::fuzz_target;
        fuzz_target!(|data: &[u8]| {
            let points: Vec<u32> = data.iter().map(|b| u32::from(*b)).collect();
            let _ = textkit_core::slug(&points, &[45]);
        });
    """))
    good = fuzz_gate(rust_workspace, seconds_per_target=5, target_dir=REPO_TARGET / "fuzz-target")
    assert good.passed, good.details
    target.write_text(target.read_text().replace("let _ = textkit_core", 'if data.first() == Some(&b"a"[0]) { panic!("found") }\n let _ = textkit_core'))
    bad = fuzz_gate(rust_workspace, seconds_per_target=20, target_dir=REPO_TARGET / "fuzz-target")
    shutil.rmtree(fuzz)
    assert not bad.passed
    assert "digits" in bad.details["crashes"]


@docker
def test_clean_build_output_after_gates(rust_workspace):
    site = REPO_TARGET / "site-check"
    built = build_python_package(rust_workspace, "crates/textkit-py", site)
    assert built.returncode == 0, built.stderr[-1000:]
    assert list(site.rglob("*.so"))
