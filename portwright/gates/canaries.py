"""Tests the checks themselves by planting bugs in copies of the Python.

This works before any Rust exists. Each copy gets one small bug and acts as a
buggy port. If the checks still pass, they have a blind spot at that line.
"""

from __future__ import annotations

import ast
import copy
import random
import shutil
import tempfile
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass
from pathlib import Path

from portwright.gates import GateResult
from portwright.gates.differential import PYTEST_EXIT_OK, PYTEST_EXIT_TESTS_FAILED, run_checks

_COMPARE_SWAPS = {
    ast.Lt: ast.LtE, ast.LtE: ast.Lt, ast.Gt: ast.GtE, ast.GtE: ast.Gt,
    ast.Eq: ast.NotEq, ast.NotEq: ast.Eq, ast.In: ast.NotIn, ast.NotIn: ast.In,
    ast.Is: ast.IsNot, ast.IsNot: ast.Is,
}
_BINOP_SWAPS = {
    ast.Add: ast.Sub, ast.Sub: ast.Add, ast.Mult: ast.Add,
    ast.FloorDiv: ast.Mod, ast.Mod: ast.FloorDiv,
}


@dataclass(frozen=True)
class Mutant:
    """One planted bug: which file and node to change, and how."""

    file: str
    node_index: int
    line: int
    description: str


def _docstring_nodes(tree) -> set[int]:
    found = set()
    for node in ast.walk(tree):
        if isinstance(node, (ast.Module, ast.ClassDef, ast.FunctionDef, ast.AsyncFunctionDef)):
            body = node.body
            if body and isinstance(body[0], ast.Expr) and isinstance(body[0].value, ast.Constant):
                found.add(id(body[0].value))
    return found


def _protected_nodes(tree) -> set[int]:
    """Nodes that shouldn't be changed: docstrings, ``__all__``, and annotations."""
    protected = _docstring_nodes(tree)
    for node in ast.walk(tree):
        if isinstance(node, ast.Assign) and any(
            isinstance(target, ast.Name) and target.id == "__all__" for target in node.targets
        ):
            protected.update(id(child) for child in ast.walk(node))
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            annotations = [node.returns] + [arg.annotation for arg in ast.walk(node.args) if isinstance(arg, ast.arg)]
            for annotation in annotations:
                if annotation is not None:
                    protected.update(id(child) for child in ast.walk(annotation))
        if isinstance(node, ast.AnnAssign):
            protected.update(id(child) for child in ast.walk(node.annotation))
    return protected


def _describe(node) -> str | None:
    """Return a label if this node can be mutated, otherwise None."""
    if isinstance(node, ast.Compare) and type(node.ops[0]) in _COMPARE_SWAPS:
        return f"swap comparison {type(node.ops[0]).__name__}"
    if isinstance(node, ast.BinOp) and type(node.op) in _BINOP_SWAPS:
        return f"swap operator {type(node.op).__name__}"
    if isinstance(node, ast.BoolOp):
        return f"swap {type(node.op).__name__}"
    if isinstance(node, ast.UnaryOp) and isinstance(node.op, (ast.Not, ast.USub)):
        return f"remove {type(node.op).__name__}"
    if isinstance(node, ast.Constant):
        if isinstance(node.value, bool):
            return "flip boolean"
        if isinstance(node.value, int):
            return f"change {node.value}"
        if isinstance(node.value, str):
            return "change string"
    if isinstance(node, ast.If):
        return "negate condition"
    if isinstance(node, ast.Return) and node.value is not None:
        return "return None"
    return None


def _apply(tree, index: int) -> None:
    """Change the node at ``index`` in walk order, in place."""
    protected = _protected_nodes(tree)
    parents = {id(child): parent for parent in ast.walk(tree) for child in ast.iter_child_nodes(parent)}
    position = 0
    for node in ast.walk(tree):
        if id(node) in protected or _describe(node) is None:
            continue
        if position == index:
            _mutate(node, parents)
            return
        position += 1
    raise IndexError(index)


def _mutate(node, parents) -> None:
    if isinstance(node, ast.Compare):
        node.ops[0] = _COMPARE_SWAPS[type(node.ops[0])]()
    elif isinstance(node, ast.BinOp):
        node.op = _BINOP_SWAPS[type(node.op)]()
    elif isinstance(node, ast.BoolOp):
        node.op = ast.Or() if isinstance(node.op, ast.And) else ast.And()
    elif isinstance(node, ast.UnaryOp):
        replacement = node.operand
        parent = parents[id(node)]
        for name, value in ast.iter_fields(parent):
            if value is node:
                setattr(parent, name, replacement)
            elif isinstance(value, list):
                for position, item in enumerate(value):
                    if item is node:
                        value[position] = replacement
    elif isinstance(node, ast.Constant):
        if isinstance(node.value, bool):
            node.value = not node.value
        elif isinstance(node.value, int):
            node.value = node.value + 1
        else:
            node.value = node.value + "X"
    elif isinstance(node, ast.If):
        node.test = ast.UnaryOp(op=ast.Not(), operand=node.test)
    elif isinstance(node, ast.Return):
        node.value = ast.Constant(value=None)


def list_mutants(source_dir: str | Path) -> list[Mutant]:
    """List every possible planted bug in a library's Python files."""
    source_dir = Path(source_dir)
    mutants = []
    for path in sorted(source_dir.rglob("*.py")):
        tree = ast.parse(path.read_text(encoding="utf-8"))
        protected = _protected_nodes(tree)
        position = 0
        for node in ast.walk(tree):
            if id(node) in protected:
                continue
            label = _describe(node)
            if label is None:
                continue
            mutants.append(
                Mutant(str(path.relative_to(source_dir)), position, getattr(node, "lineno", 0), label)
            )
            position += 1
    return mutants


def build_mutant(source_dir: str | Path, mutant: Mutant, target_dir: str | Path) -> None:
    """Copy the library to ``target_dir`` with one bug planted."""
    shutil.copytree(source_dir, target_dir, dirs_exist_ok=True)
    path = Path(target_dir) / mutant.file
    tree = ast.parse(path.read_text(encoding="utf-8"))
    _apply(tree, mutant.node_index)
    path.write_text(ast.unparse(tree), encoding="utf-8")


def _run_one(mutant, source_dir, checks_dir, module, python, seed, max_examples, timeout):
    with tempfile.TemporaryDirectory(prefix="portwright-mutant-") as scratch:
        build_mutant(source_dir, mutant, scratch)
        reference = {"path": str(source_dir), "module": module, "python": python}
        candidate = {"path": scratch, "module": module, "python": python}
        run = run_checks(
            checks_dir, reference, candidate,
            seed=seed, max_examples=max_examples, timeout=timeout, stop_at_first=True,
        )
    if run.exit_status == PYTEST_EXIT_TESTS_FAILED and run.divergences:
        return mutant, "caught"
    if run.exit_status == PYTEST_EXIT_OK:
        return mutant, "missed"
    # A planted bug can stop the module from importing at all. That proves nothing
    # about the checks, so it isn't counted for or against them.
    return mutant, "not_viable"


def canary_gate(
    checks_dir: str | Path,
    source_dir: str | Path,
    module: str,
    *,
    python: str | None = None,
    minimum: float = 0.8,
    sample: int | None = None,
    seed: int = 0,
    max_examples: int = 100,
    timeout: float = 5.0,
    workers: int = 4,
) -> GateResult:
    """Fail if the checks catch too small a share of planted bugs."""
    import sys

    python = python or sys.executable
    source_dir = Path(source_dir).resolve()
    checks_dir = Path(checks_dir).resolve()
    mutants = list_mutants(source_dir)
    if sample is not None and sample < len(mutants):
        mutants = sorted(random.Random(seed).sample(mutants, sample), key=lambda m: (m.file, m.line))
    with ThreadPoolExecutor(max_workers=workers) as pool:
        outcomes = list(
            pool.map(
                lambda m: _run_one(m, source_dir, checks_dir, module, python, seed, max_examples, timeout),
                mutants,
            )
        )
    caught = [m for m, verdict in outcomes if verdict == "caught"]
    missed = [m for m, verdict in outcomes if verdict == "missed"]
    not_viable = [m for m, verdict in outcomes if verdict == "not_viable"]
    viable = len(caught) + len(missed)
    score = len(caught) / viable if viable else 0.0
    details = {
        "score": score,
        "caught": len(caught),
        "not_viable": len(not_viable),
        "missed": [{"file": m.file, "line": m.line, "bug": m.description} for m in missed],
    }
    passed = viable > 0 and score >= minimum
    summary = f"caught {len(caught)}/{viable} planted bugs ({score:.0%}, minimum {minimum:.0%})"
    return GateResult("canaries", passed, summary, details)
