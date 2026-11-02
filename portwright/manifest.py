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


def extract_api(package_dir: str | Path) -> list[str]:
    """Return public functions, classes, and methods, as sorted dotted names."""
    package_dir = Path(package_dir)
    found: set[str] = set()
    for path in sorted(package_dir.rglob("*.py")):
        relative = path.relative_to(package_dir)
        if any(part.startswith("_") and part != "__init__.py" for part in relative.parts):
            continue
        tree = ast.parse(path.read_text(encoding="utf-8"))
        # __all__ only says what `import *` brings in. Libraries often document public
        # names outside it, so a port must cover both.
        exported = _dunder_all(tree) or []
        for node in tree.body:
            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                if node.name in exported or _is_public(node.name):
                    found.add(node.name)
            elif isinstance(node, ast.ClassDef):
                if node.name in exported or _is_public(node.name):
                    found.update(_class_members(node))
        found.update(exported)
    return sorted(found)
