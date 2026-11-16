"""A task's new Rust is measured against the checks before it merges (open item A)."""

import subprocess

from portwright.gates.coverage import new_code_coverage, parse_lcov
from portwright.orchestrator import _ranges
from portwright.scheduler import Workspaces

LCOV = """SF:/work/crates/core/src/lib.rs
DA:1,5
DA:2,0
DA:4,3
end_of_record
SF:/work/crates/core/src/tool.rs
DA:1,0
DA:2,0
end_of_record
"""


def test_lcov_lines_are_read_per_file():
    assert parse_lcov(LCOV) == {"crates/core/src/lib.rs": {1: 5, 2: 0, 4: 3}, "crates/core/src/tool.rs": {1: 0, 2: 0}}


def test_only_instrumented_new_lines_count():
    hits = parse_lcov(LCOV)
    added = {"crates/core/src/lib.rs": {1, 2, 3}, "crates/core/src/tool.rs": {1, 2}, "crates/core/src/types.rs": {1, 2, 3}}
    found = new_code_coverage(added, hits)
    # line 3 isn't instrumented (a comment, say); types.rs holds no code at all.
    assert (found["run"], found["lines"]) == (1, 4)
    assert found["unrun"] == {"crates/core/src/lib.rs": [2], "crates/core/src/tool.rs": [1, 2]}
    assert found["uninstrumented_files"] == ["crates/core/src/types.rs"]
    assert new_code_coverage({}, hits)["percent"] == 100.0


def test_line_ranges_read_compactly():
    assert _ranges([1, 2, 3, 7, 9, 10]) == "1-3, 7, 9-10"


def test_added_lines_come_from_the_tasks_own_commits(tmp_path):
    repo = tmp_path / "repo"
    (repo / "crates" / "core" / "src").mkdir(parents=True)
    lib = repo / "crates" / "core" / "src" / "lib.rs"
    lib.write_text("fn a() {}\nfn b() {}\n")
    (repo / "README.md").write_text("x\n")
    identity = ["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false"]
    subprocess.run(["git", "init", "-q", "-b", "main", str(repo)], check=True)
    subprocess.run(["git", *identity, "add", "-A"], cwd=repo, check=True)
    subprocess.run(["git", *identity, "commit", "-qm", "start"], cwd=repo, check=True)
    workspaces = Workspaces(repo, tmp_path / "worktrees")
    task = workspaces.create("task")
    (task / "crates" / "core" / "src" / "lib.rs").write_text("fn a() {}\nfn new() {}\nfn b() {}\nfn c() {}\n")
    (task / "crates" / "core" / "src" / "tool.rs").write_text("fn t() {}\n")
    (task / "README.md").write_text("changed\n")
    workspaces.commit(task, "task work")
    assert workspaces.added_lines(task) == {"crates/core/src/lib.rs": {2, 4}, "crates/core/src/tool.rs": {1}}
