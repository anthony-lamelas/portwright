"""Catches ways a port could pass without being a real port."""

from __future__ import annotations

import hashlib
import re
import tomllib
from pathlib import Path

from porter.gates import GateResult

# Common building blocks. Anything else needs an approved request, which also blocks
# pulling in an existing Rust port of the library being ported.
APPROVED_CRATES = {
    "indexmap", "num-bigint", "num-integer", "num-traits", "num-rational", "regex", "fancy-regex",
    "chrono", "rust_decimal", "bigdecimal", "thiserror", "once_cell", "smallvec", "memchr", "itertools",
    "unicode-segmentation", "unicode-normalization", "criterion", "proptest", "pyo3", "libfuzzer-sys",
    "arbitrary",
}
# Python modules the PyO3 layer may import, to build return types Python users expect.
ALLOWED_PYTHON_IMPORTS = {"datetime", "decimal", "builtins", "fractions"}

_IMPORT = re.compile(r"""(?:\bimport(?:_bound)?\s*\(\s*(?:py\s*,\s*)?(?:c"|")|intern!\s*\(\s*py\s*,\s*")([\w.]*)"?""")
_EXEC_PATTERNS = {
    "runs Python source": re.compile(r"\bfrom_code(?:_bound)?\b|\bpy\.(?:run|eval)(?:_bound)?\b|\bPython::(?:run|eval)\b|\bpy_run!"),
    "uses dynamic Python": re.compile(r'"(?:exec|eval|compile|__import__|importlib|runpy)"'),
    "embeds Python source": re.compile(r'include_(?:str|bytes)!\s*\(\s*"[^"]*\.py"'),
}
_SKIP = {"target", ".git"}


def hash_tree(path: str | Path) -> dict[str, str]:
    """Return a hash for every file under ``path``, ignoring caches."""
    path = Path(path)
    hashes = {}
    for item in sorted(path.rglob("*")):
        if item.is_file() and not {"__pycache__", ".hypothesis", ".pytest_cache"} & set(item.parts):
            hashes[str(item.relative_to(path))] = hashlib.sha256(item.read_bytes()).hexdigest()
    return hashes


def changed_files(expected: dict[str, str], current: dict[str, str]) -> list[str]:
    return sorted(
        name for name in set(expected) | set(current) if expected.get(name) != current.get(name)
    )


def _files(workspace: Path, suffix: str):
    for path in workspace.rglob(f"*{suffix}"):
        if not _SKIP & set(path.relative_to(workspace).parts):
            yield path


def _dependencies(manifest: dict) -> set[str]:
    found = set()
    for section in ("dependencies", "dev-dependencies", "build-dependencies"):
        for name, value in manifest.get(section, {}).items():
            package = value.get("package", name) if isinstance(value, dict) else name
            if isinstance(value, dict) and ("path" in value or value.get("workspace")):
                continue
            found.add(package)
    for name in manifest.get("workspace", {}).get("dependencies", {}):
        found.add(name)
    return found


def integrity_gate(
    workspace: str | Path,
    module: str,
    *,
    core_crate: str,
    locked: dict[str, dict[str, dict[str, str]]] | None = None,
    approved_crates: set[str] | None = None,
) -> GateResult:
    """Fail on any sign the port depends on Python logic instead of implementing it.

    ``locked`` maps a label to ``{"path": ..., "hashes": ...}`` for files that mustn't change.
    """
    workspace = Path(workspace)
    problems: list[str] = []
    allowed_crates = APPROVED_CRATES | (approved_crates or set())

    for path in _files(workspace, ".py"):
        problems.append(f"{path.relative_to(workspace)}: Python source isn't allowed in a port")

    core_seen = False
    for path in _files(workspace, "Cargo.toml"):
        relative = path.relative_to(workspace)
        if "fuzz" in relative.parts:
            continue
        try:
            manifest = tomllib.loads(path.read_text(encoding="utf-8"))
        except tomllib.TOMLDecodeError as error:
            problems.append(f"{relative}: invalid TOML: {error}")
            continue
        dependencies = _dependencies(manifest)
        for crate in sorted(dependencies - allowed_crates):
            problems.append(f"{relative}: crate {crate} isn't approved")
        if manifest.get("package", {}).get("name") == core_crate:
            core_seen = True
            if "pyo3" in dependencies:
                problems.append(f"{relative}: the core crate can't depend on PyO3")
            lib = path.parent / "src" / "lib.rs"
            if not lib.exists() or "forbid(unsafe_code)" not in lib.read_text(encoding="utf-8"):
                problems.append(f"{relative}: core must declare #![forbid(unsafe_code)]")
    if not core_seen:
        problems.append(f"core crate {core_crate} not found")

    for path in _files(workspace, ".rs"):
        relative = path.relative_to(workspace)
        if "fuzz" in relative.parts:
            continue
        text = path.read_text(encoding="utf-8")
        for match in _IMPORT.finditer(text):
            imported = match.group(1)
            if imported.split(".")[0] == module:
                problems.append(f"{relative}: imports the original library {imported}")
            elif "import" in match.group(0) and imported.split(".")[0] not in ALLOWED_PYTHON_IMPORTS:
                problems.append(f"{relative}: imports Python module {imported}, which isn't allowed")
        for label, pattern in _EXEC_PATTERNS.items():
            if pattern.search(text):
                problems.append(f"{relative}: {label}")

    for label, lock in (locked or {}).items():
        changed = changed_files(lock["hashes"], hash_tree(lock["path"]))
        if changed:
            problems.append(f"{label} changed: {', '.join(changed[:10])}")

    summary = f"{len(problems)} integrity problems" if problems else "no integrity problems"
    return GateResult("integrity", not problems, summary, {"problems": problems})
