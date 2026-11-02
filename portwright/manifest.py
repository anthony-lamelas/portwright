"""Lists a library's public API by reading its source, without importing it.

Harness code builds this list, not an agent, so an agent can't quietly leave part
of the API out of a port.
"""

from __future__ import annotations

import ast
from pathlib import Path


def _is_public(name: str) -> bool:
    return not name.startswith("_")


def _dunder_all(tree: ast.Module) -> list[str] | None:
    for node in tree.body:
        if isinstance(node, ast.Assign) and any(
            isinstance(target, ast.Name) and target.id == "__all__" for target in node.targets
        ):
            try:
                return [str(item) for item in ast.literal_eval(node.value)]
            except ValueError:
                return None
    return None


def _class_members(node: ast.ClassDef) -> list[str]:
    members = [node.name]
    for item in node.body:
        if isinstance(item, (ast.FunctionDef, ast.AsyncFunctionDef)) and _is_public(item.name):
            members.append(f"{node.name}.{item.name}")
    return members


def _root_names(package_dir: Path) -> set[str]:
    """Names the package root offers: its __all__ plus anything it imports by name."""
    init = package_dir / "__init__.py"
    if not init.exists():
        return set()
    tree = ast.parse(init.read_text(encoding="utf-8"))
    exported = _dunder_all(tree) or []
    names = set(exported)
    for node in tree.body:
        if isinstance(node, ast.ImportFrom):
            names.update(alias.asname or alias.name for alias in node.names if alias.name != "*")
    return {name for name in names if _is_public(name) or name in exported}


def extract_api(package_dir: str | Path) -> list[str]:
    """Return public functions, classes, and methods, as sorted dotted names.

    Names are paths from the package root, the way callers reach them:
    ``naturaltime`` if the root re-exports it, ``time.Unit`` if it lives only in
    the ``time`` submodule.
    """
    package_dir = Path(package_dir)
    at_root = _root_names(package_dir)
    found: set[str] = set()
    for path in sorted(package_dir.rglob("*.py")):
        relative = path.relative_to(package_dir)
        if any(part.startswith("_") and part != "__init__.py" for part in relative.parts):
            continue
        parts = list(relative.with_suffix("").parts)
        if parts[-1] == "__init__":
            parts.pop()
        prefix = ".".join(parts)

        def place(name: str, prefix: str = prefix) -> str:
            return name if not prefix or name.split(".")[0] in at_root else f"{prefix}.{name}"

        tree = ast.parse(path.read_text(encoding="utf-8"))
        # __all__ only says what `import *` brings in. Libraries often document public
        # names outside it, so a port must cover both.
        exported = _dunder_all(tree) or []
        for node in tree.body:
            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                if node.name in exported or _is_public(node.name):
                    found.add(place(node.name))
            elif isinstance(node, ast.ClassDef):
                if node.name in exported or _is_public(node.name):
                    found.update(place(member) for member in _class_members(node))
        found.update(place(name) for name in exported)
    return sorted(found)
