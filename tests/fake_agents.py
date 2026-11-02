"""Scripted agents, so the whole pipeline can be tested without using any Claude usage."""

from __future__ import annotations

import json
import re
import shutil
from pathlib import Path

from portwright.agents import AgentResult, AgentSession, LimitReached  # noqa: F401

FIXTURES = Path(__file__).parent / "fixtures"
CORE = "textkit-port-core"
PY = "textkit-port-py"


def mount(session: AgentSession, container: str) -> Path:
    for host, target in session.mounts.items():
        if target.split(":")[0] == container:
            return Path(host)
    raise KeyError(container)


def copy_real_port(port: Path) -> None:
    """Fill the scaffold from the hand-written port, renamed to the scaffold's crates."""
    source = FIXTURES / "textkit_rust" / "crates"
    (port / "crates" / CORE / "src" / "lib.rs").write_text((source / "textkit-core/src/lib.rs").read_text())
    binding = (source / "textkit-py/src/lib.rs").read_text().replace("textkit_core", "textkit_port_core")
    (port / "crates" / PY / "src" / "lib.rs").write_text(binding)
    core_manifest = port / "crates" / CORE / "Cargo.toml"
    if "indexmap" in core_manifest.read_text():
        return  # a retry in the same worktree
    core_manifest.write_text(core_manifest.read_text().replace(
        "[dependencies]\n", '[dependencies]\nindexmap = "2"\nnum-bigint = "0.4"\nnum-integer = "0.1"\nnum-traits = "0.2"\n'))
    py_manifest = port / "crates" / PY / "Cargo.toml"
    py_manifest.write_text(py_manifest.read_text().replace(
        "[dependencies]\n", '[dependencies]\nnum-bigint = "0.4"\nnum-traits = "0.2"\n').replace(
        '"extension-module"', '"extension-module", "num-bigint"'))


FUZZ_MANIFEST = f"""[package]
name = "textkit-fuzz"
version = "0.0.0"
edition = "2021"
publish = false
[package.metadata]
cargo-fuzz = true
[dependencies]
libfuzzer-sys = "0.4"
{CORE} = {{ path = "../crates/{CORE}" }}
[workspace]
[[bin]]
name = "slug"
path = "fuzz_targets/slug.rs"
test = false
doc = false
"""
FUZZ_TARGET = """#![no_main]
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    let points: Vec<u32> = data.iter().map(|byte| u32::from(*byte)).collect();
    let _ = textkit_port_core::slug(&points, &[45]);
});
"""


class FakeAgents:
    """Plays each role with a script. ``behaviors`` can change a role's behavior for one run."""

    def __init__(self, plan=None, behaviors=None):
        self.calls: list[str] = []
        self.plan = plan or {
            "tasks": [{"id": "everything", "apis": ALL_APIS, "depends_on": []}],
            "shared_paths": [],
        }
        self.behaviors = behaviors or {}

    def run(self, session: AgentSession) -> AgentResult:
        self.calls.append(f"{session.role}:{session.scratch.name}")
        behavior = self.behaviors.get(session.role)
        if behavior:
            result = behavior(self, session)
            if result is not None:
                return result
        getattr(self, session.role)(session)
        return AgentResult("ok", "done", {"output_tokens": 10, "estimated_api_cost_usd": 0.01})

    def test_writer(self, session):
        if "/work/fuzz" in [t.split(":")[0] for t in session.mounts.values()]:
            fuzz = mount(session, "/work/fuzz")
            (fuzz / "fuzz_targets").mkdir(parents=True, exist_ok=True)
            (fuzz / "Cargo.toml").write_text(FUZZ_MANIFEST)
            (fuzz / "fuzz_targets" / "slug.rs").write_text(FUZZ_TARGET)
            return
        if "strengthen" in session.scratch.name:
            return
        checks = mount(session, "/work/checks")
        shutil.copy(FIXTURES / "checks_good" / "test_textkit.py", checks / "test_textkit.py")
        (checks / "benchmarks.json").write_text(json.dumps([
            {"name": "slug", "target": "slug", "args": ["Hello Big World 123"]},
            {"name": "digits", "target": "sum_digits", "args": [123456789012345678901234567890]},
        ]))

    def planner(self, session):
        (mount(session, "/work/out") / "plan.json").write_text(json.dumps(self.plan))

    def porter(self, session):
        copy_real_port(mount(session, "/work/port"))

    def debugger(self, session):
        copy_real_port(mount(session, "/work/port"))

    def reviewer(self, session):
        (mount(session, "/work/out") / "review.md").write_text("No must-fix findings.\n")

    def cleanup(self, session):
        pass

    def performance(self, session):
        pass


ALL_APIS = [
    "EmptyError", "ParseError", "Stack", "Stack.peek", "Stack.pop", "Stack.push",
    "first_chunks", "floor_bucket", "normalize_in_place", "parse_kv", "slug", "sum_digits",
    "truncate", "word_count",
]


def cheat_import_original(agents, session):
    """A porter that tries to pass by calling the original Python library."""
    if sum(call.startswith("porter") for call in agents.calls) == 1:
        port = mount(session, "/work/port")
        copy_real_port(port)
        binding = port / "crates" / PY / "src" / "lib.rs"
        binding.write_text(binding.read_text() + '\nfn cheat(py: Python<\'_>) { let _ = py.import("textkit"); }\n')
        return AgentResult("ok", "done, it all passes!", {})
    return None


def limit_once(agents, session):
    """Hits a usage limit the first time, then works normally."""
    if sum(call.startswith("porter") for call in agents.calls) == 1:
        return AgentResult("limit", "Claude usage limit reached", {})
    return None


def broken_port(agents, session):
    """Writes wrong Rust the first two times, so the debugger has to take over."""
    if session.role == "porter":
        port = mount(session, "/work/port")
        copy_real_port(port)
        core = port / "crates" / CORE / "src" / "lib.rs"
        core.write_text(core.read_text().replace("n.div_mod_floor(size)", "n.div_rem(size)"))
        return AgentResult("ok", "done", {})
    return None


def weak_checks_only(agents, session):
    checks = mount(session, "/work/checks")
    shutil.copy(FIXTURES / "checks_weak" / "test_weak.py", checks / "test_weak.py")
    return AgentResult("ok", "done", {})


def regressing_cleanup(agents, session):
    """A cleanup that quietly changes behavior must be rejected."""
    core = mount(session, "/work/port") / "crates" / CORE / "src" / "lib.rs"
    text = core.read_text()
    new = re.sub(r"n\.div_mod_floor\(size\)", "n.div_rem(size)", text)
    assert new != text
    core.write_text(new)
    return AgentResult("ok", "cleaned up", {})
