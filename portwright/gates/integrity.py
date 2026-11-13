"""Catches ways a port could pass without being a real port."""

from __future__ import annotations

import hashlib
import os
import re
import tomllib
from pathlib import Path

from portwright.gates import GateResult
from portwright.gates.rust_source import RustFile, call_args, literal, read_rust
from portwright.gates.strict_bindings import bindings_problems, feature_problems, macro_problems

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

_INTERN = re.compile(r"\bintern!\s*\(\s*py\s*,\s*")
_IMPORT_CALL = re.compile(r"\bimport(?:_bound)?\s*\(")
_EXEC_PATTERNS = {
    "runs Python source": re.compile(r"\bfrom_code(?:_bound)?\b|\bpy\.(?:run|eval)(?:_bound)?\b|\bPython::(?:run|eval)\b|\bpy_run!"),
    "uses dynamic Python": re.compile(r'"(?:exec|eval|compile|__import__|importlib|runpy)"'),
    "embeds Python source": re.compile(r'include_(?:str|bytes)!\s*\(\s*"[^"]*\.py"'),
    "imports through the C API": re.compile(r"\bPyImport_\w+"),
}
_DEPENDENCY_SECTIONS = ("dependencies", "dev-dependencies", "build-dependencies")
# Build output, skipped only at the top of the workspace: anywhere else, a folder named
# target is the port's own code (maturin will pack one into the wheel if told to).
_TOP_LEVEL_SKIP = {"target"}
_SKIP = {".git"}
# Keys a port's pyproject.toml may set under [tool.maturin]. Others, such as
# python-source, include or data, would pack Python into the wheel.
_MATURIN_KEYS = {"module-name"}
# Lines in the __init__.py maturin generates for a crate with no Python source.
_MATURIN_INIT = 'from .{0} import *\n\n__doc__ = {0}.__doc__\nif hasattr({0}, "__all__"):\n    __all__ = {0}.__all__'


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


def _walk(workspace: Path):
    """Every file the port owns: all of the workspace but .git and the top-level target/."""
    for root, folders, files in os.walk(workspace):
        here = Path(root)
        folders[:] = [
            name for name in folders
            if name not in _SKIP and not (here == workspace and name in _TOP_LEVEL_SKIP)
        ]
        for name in files:
            yield here / name


def _files(workspace: Path, suffix: str):
    for path in _walk(workspace):
        if path.name.endswith(suffix):
            yield path


def _is_fuzz(relative: Path) -> bool:
    """The harness-written fuzz workspace at the top of the port, which is frozen."""
    return bool(relative.parts) and relative.parts[0] == "fuzz"


def _within(path: Path, folder: Path) -> bool:
    return path == folder or folder in path.parents


def _dependency_entries(manifest: dict):
    """(name, spec) for every dependency table, including target-specific ones."""
    tables = [manifest.get(section, {}) for section in _DEPENDENCY_SECTIONS]
    for target in manifest.get("target", {}).values():
        if isinstance(target, dict):
            tables += [target.get(section, {}) for section in _DEPENDENCY_SECTIONS]
    for table in tables:
        if isinstance(table, dict):
            yield from table.items()


def _package(name: str, spec, workspace_deps: dict) -> str:
    if isinstance(spec, dict) and spec.get("workspace"):
        spec = workspace_deps.get(name, spec)
    return spec.get("package", name) if isinstance(spec, dict) else name


def _dependencies(manifest: dict) -> set[str]:
    found = set()
    for name, value in _dependency_entries(manifest):
        package = value.get("package", name) if isinstance(value, dict) else name
        if isinstance(value, dict) and ("path" in value or value.get("workspace")):
            continue
        found.add(package)
    for name in manifest.get("workspace", {}).get("dependencies", {}):
        found.add(name)
    return found


class _Crate:
    """A manifest, the PyO3 dependency specs it has, and the crates it depends on by path."""

    def __init__(self, path: Path, relative: Path, manifest: dict, root: Path, workspace_deps: dict):
        self.directory = path.parent.resolve()
        self.relative = relative
        self.manifest = manifest
        self.name = manifest.get("package", {}).get("name")
        self.pyo3_specs = []
        self.path_deps: set[Path] = set()
        for name, spec in _dependency_entries(manifest):
            inherited = isinstance(spec, dict) and spec.get("workspace")
            resolved = workspace_deps.get(name, spec) if inherited else spec
            if _package(name, spec, workspace_deps) in ("pyo3", "pyo3-ffi"):
                self.pyo3_specs.append(resolved)
            if isinstance(resolved, dict) and "path" in resolved:
                self.path_deps.add(((root if inherited else path.parent) / resolved["path"]).resolve())
        # Source folders: the crate's own, and any a [lib] or [[bin]] path points elsewhere.
        self.source_dirs = {self.directory}
        targets = [manifest.get("lib", {})]
        for kind in ("bin", "test", "bench", "example"):
            targets += manifest.get(kind, []) if isinstance(manifest.get(kind), list) else []
        for target in targets:
            if isinstance(target, dict) and isinstance(target.get("path"), str):
                self.source_dirs.add((path.parent / target["path"]).resolve().parent)
        build = manifest.get("package", {}).get("build")
        self.build_script = isinstance(build, str) or (build is not False and (path.parent / "build.rs").is_file())


def _manifest_problems(workspace: Path, relative: Path, manifest: dict) -> list[str]:
    """Dependencies that come from somewhere other than crates.io or the port itself."""
    problems = []
    for key in ("patch", "replace"):
        if key in manifest:
            problems.append(f"{relative}: [{key}] overrides dependencies, which isn't allowed")
    if "maturin" in manifest.get("package", {}).get("metadata", {}):
        problems.append(f"{relative}: [package.metadata.maturin] changes what the wheel holds, which isn't allowed")
    specs = list(_dependency_entries(manifest)) + list(manifest.get("workspace", {}).get("dependencies", {}).items())
    for name, spec in specs:
        if not isinstance(spec, dict):
            continue
        for source in ("git", "registry", "registry-index"):
            if source in spec:
                problems.append(f"{relative}: dependency {name} comes from {source} {spec[source]!r}, not crates.io")
        if "path" in spec:
            target = (workspace / relative).parent / spec["path"]
            resolved = target.resolve()
            if not _within(resolved, workspace.resolve()):
                problems.append(f"{relative}: dependency {name} is outside the port: {spec['path']}")
            elif _within(resolved, workspace.resolve() / "target"):
                problems.append(f"{relative}: dependency {name} is in build output: {spec['path']}")
    return problems


def _pyproject_problems(workspace: Path, module: str, bindings_dir: str | None) -> list[str]:
    """maturin reads [tool.maturin], and settings such as python-source pack Python into the wheel."""
    problems = []
    # maturin reads the one beside the crate's Cargo.toml, or else the one where it runs.
    read = [workspace / "pyproject.toml"]
    if bindings_dir:
        read.append(workspace / bindings_dir / "pyproject.toml")
    for path in (read if bindings_dir else _files(workspace, "pyproject.toml")):
        if not path.is_file():
            continue
        relative = path.relative_to(workspace)
        try:
            data = tomllib.loads(path.read_text(encoding="utf-8"))
        except tomllib.TOMLDecodeError as error:
            problems.append(f"{relative}: invalid TOML: {error}")
            continue
        settings = data.get("tool", {}).get("maturin", {})
        for key in sorted(set(settings) - _MATURIN_KEYS):
            problems.append(f"{relative}: [tool.maturin] {key} isn't allowed; the wheel may only hold the compiled module")
        if "module-name" in settings and settings["module-name"] != module:
            problems.append(f"{relative}: [tool.maturin] module-name must be {module}")
    if bindings_dir and (workspace / bindings_dir / "Cargo.toml").is_file() and not (workspace / bindings_dir / "pyproject.toml").is_file():
        problems.append(f"{bindings_dir}/pyproject.toml is missing; restore the scaffold's")
    return problems


def _include_targets(workspace: Path, path: Path, file: RustFile, problems: list[str]) -> list[Path]:
    """Files a #[path] attribute or include! pulls in, which may lie outside any crate."""
    relative = path.relative_to(workspace)
    found = []
    for match in re.finditer(r"#\s*\[\s*path\s*=\s*|\binclude\s*!\s*\(", file.code):
        if match.group().startswith("#"):
            value_text = file.src[match.end():file.code.find("]", match.end())]
        else:
            args, _ = call_args(file.code, match.end() - 1)
            value_text = file.src[args[0][0]:args[0][1]] if len(args) == 1 else ""
        value = literal(value_text)
        line = file.line(match.start())
        if value is None:
            problems.append(f"{relative}:{line}: includes code by a computed path, which can't be checked: {file.snippet(match.start())}")
            continue
        target = (path.parent / value).resolve()
        if not _within(target, workspace):
            problems.append(f"{relative}:{line}: includes {value}, which is outside the port")
        elif _within(target, workspace / "target"):
            problems.append(f"{relative}:{line}: includes {value}, which is build output")
        elif target.is_file():
            found.append(target)
        # Inside an inline module of a non-mod.rs file, #[path] starts from a folder named after the file.
        nested = (path.parent / path.stem / value).resolve()
        if match.group().startswith("#") and nested.is_file() and _within(nested, workspace) and not _within(nested, workspace / "target"):
            found.append(nested)
    return found


def _import_problems(relative: Path, file: RustFile, module: str, strict: bool) -> list[str]:
    problems = []
    for match in _IMPORT_CALL.finditer(file.code):
        if re.search(r"\bfn\s*$", file.code[:match.start()]):
            continue
        args, _ = call_args(file.code, match.end() - 1)
        if not args:
            continue
        start, end = args[0] if len(args) == 1 else args[1]
        expression = file.src[start:end].strip()
        interned = _INTERN.match(expression)
        if interned:
            expression = expression[interned.end():].rstrip(") \n")
        imported = literal(expression, file.consts)
        line = file.line(match.start())
        if strict:
            problems.append(_strict_message(relative, file, match.start(), f"imports {imported or 'a module'}"))
        if imported is None:
            problems.append(f"{relative}:{line}: imports a Python module named by an expression, which can't be checked: {file.snippet(match.start())}")
            continue
        if imported.split(".")[0] == module:
            problems.append(f"{relative}: imports the original library {imported}")
        elif imported.split(".")[0] not in ALLOWED_PYTHON_IMPORTS:
            problems.append(f"{relative}: imports Python module {imported}, which isn't allowed")
    for match in _INTERN.finditer(file.code):
        args, _ = call_args(file.code, file.code.index("(", match.start()))
        value = literal(file.src[args[1][0]:args[1][1]], file.consts) if len(args) == 2 else None
        if value is not None and value.split(".")[0] == module:
            problems.append(f"{relative}: imports the original library {value}")
    return problems


def _strict_message(relative: Path, file: RustFile, offset: int, reason: str) -> str:
    return (
        f"{relative}:{file.line(offset)}: the bindings may only convert values and raise exceptions; "
        f"{reason}, which calls into Python: {file.snippet(offset)}"
    )


def integrity_gate(
    workspace: str | Path,
    module: str,
    *,
    core_crate: str,
    locked: dict[str, dict[str, dict[str, str]]] | None = None,
    approved_crates: set[str] | None = None,
    strict_bindings: bool = False,
    bindings_dir: str | None = None,
) -> GateResult:
    """Fail on any sign the port depends on Python logic instead of implementing it.

    ``locked`` maps a label to ``{"path": ..., "hashes": ...}`` for files that mustn't change.
    With ``strict_bindings``, every crate that can reach PyO3 may only convert values and
    raise exceptions: no Python imports and no calls into Python objects. Ports of
    Python's built-ins need this, or the bindings could hand the work back to CPython.
    ``bindings_dir`` is the PyO3 crate maturin builds; its pyproject.toml is checked.
    """
    workspace = Path(workspace)
    root = workspace.resolve()
    problems: list[str] = []
    allowed_crates = APPROVED_CRATES | (approved_crates or set())

    for path in _files(workspace, ".py"):
        problems.append(f"{path.relative_to(workspace)}: Python source isn't allowed in a port")
    for path in _walk(workspace):
        if path.parent.name == ".cargo" and path.name in ("config", "config.toml"):
            problems.append(f"{path.relative_to(workspace)}: Cargo configuration in the port isn't allowed")
    problems += _pyproject_problems(workspace, module, bindings_dir)

    root_manifest_path = workspace / "Cargo.toml"
    workspace_deps = {}
    if root_manifest_path.is_file():
        try:
            workspace_deps = tomllib.loads(root_manifest_path.read_text(encoding="utf-8")).get("workspace", {}).get("dependencies", {})
        except tomllib.TOMLDecodeError:
            pass
    crates: dict[Path, _Crate] = {}
    core: _Crate | None = None
    for path in _files(workspace, "Cargo.toml"):
        relative = path.relative_to(workspace)
        if _is_fuzz(relative):
            continue
        try:
            manifest = tomllib.loads(path.read_text(encoding="utf-8"))
        except tomllib.TOMLDecodeError as error:
            problems.append(f"{relative}: invalid TOML: {error}")
            continue
        problems += _manifest_problems(workspace, relative, manifest)
        dependencies = _dependencies(manifest)
        for crate in sorted(dependencies - allowed_crates):
            problems.append(f"{relative}: crate {crate} isn't approved")
        crate = _Crate(path, relative, manifest, workspace, workspace_deps)
        crates[crate.directory] = crate
        if crate.build_script:
            problems.append(f"{relative}: build scripts run code at build time that the scan can't see, which isn't allowed")
        for folder in sorted(crate.source_dirs - {crate.directory}):
            if not _within(folder, root) or _within(folder, root / "target"):
                problems.append(f"{relative}: a build target's source is outside the port or in build output")
        if crate.name == core_crate:
            core = crate
            if crate.pyo3_specs:
                problems.append(f"{relative}: the core crate can't depend on PyO3")
            lib = path.parent / "src" / "lib.rs"
            if not lib.exists() or "forbid(unsafe_code)" not in lib.read_text(encoding="utf-8"):
                problems.append(f"{relative}: core must declare #![forbid(unsafe_code)]")
    if core is None:
        problems.append(f"core crate {core_crate} not found")

    # Every crate that can call PyO3: directly, or through a path dependency on one that can.
    python_crates = {directory for directory, crate in crates.items() if crate.pyo3_specs}
    while True:
        more = {d for d, crate in crates.items() if d not in python_crates and crate.path_deps & python_crates}
        if not more:
            break
        python_crates |= more
    if core is not None and core.directory in python_crates and not core.pyo3_specs:
        problems.append(f"{core.relative}: the core crate can't depend on PyO3, even through another crate")
    strict_dirs = {folder for d in python_crates for folder in crates[d].source_dirs} if strict_bindings else set()
    if strict_bindings:
        for directory in sorted(python_crates):
            crate = crates[directory]
            for spec in crate.pyo3_specs:
                for feature in feature_problems(spec, crate.manifest.get("features", {})):
                    problems.append(f"{crate.relative}: PyO3 feature {feature} converts values by running Python code, which strict bindings don't allow")
        for crate in crates.values():
            if crate.manifest.get("lib", {}).get("proc-macro") or crate.manifest.get("lib", {}).get("proc_macro"):
                problems.append(f"{crate.relative}: procedural macros generate code the strict check can't see")
    # Crates the bindings depend on: their macros expand inside the bindings.
    macro_dirs = set()
    for directory in python_crates if strict_bindings else ():
        for dependency in crates[directory].path_deps - python_crates:
            if dependency in crates:
                macro_dirs |= crates[dependency].source_dirs

    # Every Rust file, plus whatever #[path] and include! pull in from wherever they point.
    queue = [
        (path.resolve(), any(_within(path.resolve(), d) for d in strict_dirs))
        for path in _files(workspace, ".rs") if not _is_fuzz(path.relative_to(workspace))
    ]
    files: dict[Path, RustFile] = {}
    strict_files: dict[Path, bool] = {}
    while queue:
        path, strict = queue.pop()
        if path in strict_files and (strict_files[path] or not strict):
            continue
        strict_files[path] = strict
        if path not in files:
            files[path] = read_rust(path.read_text(encoding="utf-8", errors="replace"))
        queue += [(target, strict) for target in _include_targets(root, path, files[path], problems)]
    for path, strict in sorted(strict_files.items()):
        relative = path.relative_to(root)
        file = files[path]
        problems += _import_problems(relative, file, module, strict)
        for label, pattern in _EXEC_PATTERNS.items():
            if pattern.search(file.src):
                problems.append(f"{relative}: {label}")
        if strict:
            for offset, reason in sorted(bindings_problems(file)):
                problems.append(_strict_message(relative, file, offset, reason))
        elif any(_within(path, d) for d in macro_dirs):
            for offset, reason in sorted(macro_problems(file)):
                problems.append(_strict_message(relative, file, offset, f"a macro the bindings use {reason}"))

    for label, lock in (locked or {}).items():
        changed = changed_files(lock["hashes"], hash_tree(lock["path"]))
        if changed:
            problems.append(f"{label} changed: {', '.join(changed[:10])}")

    problems = list(dict.fromkeys(problems))
    summary = f"{len(problems)} integrity problems" if problems else "no integrity problems"
    return GateResult("integrity", not problems, summary, {"problems": problems})


def _is_maturin_reexport(text: str, module: str) -> bool:
    """Whether ``text`` only re-exports the compiled module, as maturin generates it.

    Checked by structure rather than exact text, so a maturin release that words the
    file differently still passes, while any other statement fails.
    """
    import ast

    try:
        tree = ast.parse(text)
    except SyntaxError:
        return False

    def is_attribute(node, attribute):
        return (isinstance(node, ast.Attribute) and node.attr == attribute
                and isinstance(node.value, ast.Name) and node.value.id == module)

    def is_copy(node, attribute):  # __doc__ = module.__doc__, __all__ = module.__all__
        return (isinstance(node, ast.Assign) and len(node.targets) == 1
                and isinstance(node.targets[0], ast.Name) and node.targets[0].id == attribute
                and is_attribute(node.value, attribute))

    for node in tree.body:
        if isinstance(node, ast.Expr) and isinstance(node.value, ast.Constant) and isinstance(node.value.value, str):
            continue  # a docstring
        if (isinstance(node, ast.ImportFrom) and node.level == 1 and node.module == module
                and all(alias.asname is None and alias.name in ("*", module) for alias in node.names)):
            continue
        if is_copy(node, "__doc__") or is_copy(node, "__all__"):
            continue
        if (isinstance(node, ast.If) and not node.orelse
                and isinstance(node.test, ast.Call) and isinstance(node.test.func, ast.Name) and node.test.func.id == "hasattr"
                and len(node.test.args) == 2 and isinstance(node.test.args[0], ast.Name) and node.test.args[0].id == module
                and isinstance(node.test.args[1], ast.Constant) and node.test.args[1].value == "__all__"
                and all(is_copy(inner, "__all__") for inner in node.body)):
            continue
        return False
    return True


def site_problems(site: str | Path, module: str) -> list[str]:
    """Anything in an unpacked wheel besides the compiled module and its .dist-info.

    maturin packs Python files when told to (python-source, include), and Python would
    import them. For a crate with no Python, a wheel holds ``<module>/__init__.py`` with
    maturin's fixed re-export, ``<module>/<module>.<tag>.so``, optionally a stub
    (``__init__.pyi``, ``py.typed``), and ``<dist>.dist-info`` with METADATA, WHEEL,
    RECORD, and sboms/ or licenses/.
    """
    site = Path(site)
    problems = []
    extensions = 0
    for path in sorted(site.rglob("*")):
        if path.is_dir():
            continue
        relative = path.relative_to(site)
        parts = relative.parts
        name = path.name
        allowed = False
        if len(parts) == 2 and parts[0] == module:
            if name == "__init__.py":
                allowed = _is_maturin_reexport(path.read_text(encoding="utf-8", errors="replace"), module)
            elif re.fullmatch(rf"{re.escape(module)}\.(?:cpython-[\w-]+|abi3)\.so", name):
                allowed = True
                extensions += 1
            else:
                allowed = name in ("__init__.pyi", "py.typed")
        elif len(parts) == 1 and re.fullmatch(rf"{re.escape(module)}\.(?:cpython-[\w-]+|abi3)\.so", name):
            allowed = True
            extensions += 1
        elif len(parts) >= 2 and parts[0].endswith(".dist-info") and path.suffix not in (".py", ".pyc", ".pth", ".so"):
            allowed = (len(parts) == 2 and name in ("METADATA", "WHEEL", "RECORD")) or parts[1] in ("sboms", "licenses")
        if not allowed:
            problems.append(f"{relative}: the wheel may only hold the compiled module and its metadata")
    if extensions != 1:
        problems.append(f"the wheel holds {extensions} compiled modules named {module}, not one")
    return problems
