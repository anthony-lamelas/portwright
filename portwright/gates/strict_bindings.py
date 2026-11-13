"""What ``--strict-bindings`` lets a PyO3 crate do: convert values and raise exceptions.

A port of Python's built-ins must not hand the work back to CPython. Anything that
runs CPython's own behavior on a Python object counts: ``str()`` and ``repr()``
(including PyO3's ``Display`` and ``Debug``, and so ``to_string()`` and ``format!``),
attribute lookup, calls, operators and comparisons, item access through
``__getitem__``, iteration through ``__iter__``, imports, and the C API behind them.

The checks are static and can't see types, so they work from names:

- ``ffi::`` symbols, PyO3 features and codecs are allowlists, derived from the finished
  strict ports and each checked to only copy data across the boundary.
- PyO3 methods that dispatch are denied by name. Where a Rust or a concrete PyO3 type
  has a method of the same name that doesn't dispatch (``len``, ``get_item``), the
  receiver's declared type decides; ``get_item``/``set_item`` pass only on a receiver
  known to be a tuple, list, dict or other concrete container.

A Python object reaching ``Display`` through generic code in another crate, or through
a variable whose type isn't written down nearby, isn't caught.
"""

from __future__ import annotations

import re

from portwright.gates.rust_source import (
    RustFile, call_args, chain, expression_text, literal, macro_bodies, receiver, type_text,
)

# Each entry only copies data across the boundary. Taken from pc_format's bindings.
ALLOWED_FFI = {
    "Py_ssize_t": "C's ssize_t, a type",
    "PyUnicode_4BYTE_KIND": "the constant naming UCS-4 string storage",
    "PyUnicode_AsUCS4Copy": "copies a str's code points, lone surrogates included, into a new buffer",
    "PyUnicode_FromKindAndData": "builds a str from a buffer of code points",
    "PyMem_Free": "frees the buffer PyUnicode_AsUCS4Copy returned",
    "PyLong_AsLongLongAndOverflow": "reads an int's value, flagging overflow (callers pass exact ints: on other objects it calls __index__)",
}
# PyO3's own conversions for these features are plain data copies. Others run Python:
# rust_decimal and bigdecimal call str() and decimal.Decimal(), chrono calls tzinfo
# methods, num-rational reads Fraction attributes, uuid and chrono-tz import modules.
ALLOWED_PYO3_FEATURES = {"extension-module", "num-bigint", "macros"}
# Codecs implemented in C that only transcode, and error handlers that keep lone
# surrogates. A name such as "unicode_escape" would run CPython's escaping, and "idna" Python code.
ALLOWED_CODECS = {"utf-8", "utf-32", "utf-32-le", "latin-1", "strict", "surrogatepass"}
_CODEC_METHODS = {"encode", "decode"}

# PyAnyMethods that always run Python-level behavior, and whose names Rust code doesn't use.
ALWAYS_DISPATCH = {
    "repr", "str", "call", "call0", "call1", "call_method", "call_method0", "call_method1",
    "getattr", "getattr_opt", "setattr", "hasattr", "delattr", "compare", "rich_compare",
    "lt", "le", "gt", "ge", "floor_div", "div", "rem", "divmod", "matmul", "lshift", "rshift",
    "bitand", "bitor", "bitxor", "bitnot", "is_truthy", "dir", "py_super", "try_iter", "is_iterable",
}
# PyAnyMethods named like Rust or concrete PyO3 methods that don't dispatch: flagged on a
# receiver declared as a generic Python object.
DISPATCH_ON_ANY = {
    "len", "is_empty", "contains", "hash", "eq", "ne", "add", "sub", "mul", "pow", "neg", "pos", "abs",
    "is_instance",
}
# Item access reads a concrete container's storage, but calls __getitem__ and friends on
# anything else, so it passes only on a receiver known to be a concrete container.
ITEM_ACCESS = {"get_item", "set_item", "del_item"}
_CONCRETE = r"\b(?:PyTuple|PyList|PyDict|PySet|PyFrozenSet|PyString|PyBytes|PyByteArray|PyModule)\b"
_CASTS = {"cast", "cast_exact", "cast_into", "cast_into_exact", "downcast", "downcast_exact", "downcast_into", "downcast_into_exact"}
_RETURNS_PYTHON = {"into_any", "as_any", "get_type", "into_pyobject", "into_bound_py_any", "into_py_any", "get_item"}
# Moving a Python object between Py, Bound and Borrowed keeps its type.
_KEEPS_TYPE = {"bind", "into_bound", "unbind", "clone_ref", "as_borrowed", "as_unbound", "clone", "to_owned"}
_RETURNS_RUST = {"to_str", "to_string_lossy", "to_cow", "extract", "len", "as_bytes", "to_vec", "into_owned", "to_string", "is_none", "is_empty", "type_name"}
_FORMAT_MACROS = (
    "format", "format_args", "write", "writeln", "print", "println", "eprint", "eprintln", "panic",
    "unreachable", "todo", "unimplemented", "assert", "assert_eq", "assert_ne",
)
_METHOD = r"\.\s*({names})\s*(?:::\s*<[^()]*?>)?\s*\("
_RAW_ACCESS = [
    (re.compile(r"\bPyAnyMethods\b"), "names PyAnyMethods, whose methods dispatch to Python"),
    (re.compile(r"\b(?:PyIterator|PyMapping|PySequence)\b"), "uses Python's iteration, mapping or sequence protocol"),
    (re.compile(r'\bextern\s*(?:"[^"]*"\s*)?\{'), "declares C functions directly, bypassing the ffi:: allowlist"),
    (re.compile(r"#\s*\[\s*(?:link|link_name)\b"), "links C symbols directly, bypassing the ffi:: allowlist"),
    (re.compile(r"\b(?:dlsym|dlopen)\b"), "looks up C symbols at run time"),
    (re.compile(r"\.\s*(?:tp_\w+|ob_type)\b(?!\s*\()|\bas_type_ptr\b"), "reads a type object's slots, which point at CPython's code"),
    (re.compile(r"\bdbg\s*!"), "formats with Debug, which for a Python object calls repr()"),
    (re.compile(r"\b(?:Debug|Display|ToString)\s*::\s*(?:fmt|to_string)\b|\.\s*fmt\s*\("), "calls a formatting trait directly, which for a Python object calls str() or repr()"),
]
_DEBUG_SPEC = re.compile(r"\{[^{}]*:[^{}]*\?\}")


def _kind(file: RustFile, expression: str, at: int, depth: int = 0) -> str | None:
    """Whether an expression is a concrete PyO3 container ("concrete"), some other Python
    object ("python"), a Rust value ("rust"), or unknown (None)."""
    expression = re.sub(r"^(?:[&*]\s*(?:mut\s+)?)+", "", expression.strip()).strip()
    if depth > 4 or not expression:
        return None
    if re.fullmatch(r"\w+", expression):
        return _identifier_kind(file, expression, at, depth)
    segments = chain(expression)
    last = segments[-1]
    if len(segments) == 1:
        path = re.match(r"(?:\w+\s*::\s*)*(\w+)\s*(?:::\s*<.*>)?\s*::\s*\w+\s*\(", last, re.S)
        if path:
            return _type_kind(path.group(1))
        return None
    if len(segments) == 2 and segments[0] == "self" and re.fullmatch(r"\w+", last):
        field = re.search(rf"[{{,]\s*(?:pub(?:\s*\([^)]*\))?\s+)?{re.escape(last)}\s*:(?!:)", file.code)
        return _type_kind(type_text(file.code, field.end())) if field else None
    method = re.match(r"(\w+)\s*(?:::\s*<(.*)>)?\s*\(", last, re.S)
    if not method:
        return None
    name, generic = method.group(1), method.group(2) or ""
    if name in _CASTS:
        return "concrete" if re.search(_CONCRETE, generic) else "python"
    if name in _KEEPS_TYPE:
        return _kind(file, ".".join(segments[:-1]), at, depth + 1)
    if name in {"values", "keys", "items"}:
        return "concrete"
    if name in _RETURNS_PYTHON or (name in {"name", "qualname", "fully_qualified_name", "module"} and "get_type" in expression):
        return "python"
    if name in _RETURNS_RUST:
        return "rust"
    return None


def _generic_args(text: str) -> list[str]:
    args, depth, start = [], 0, 0
    for i, char in enumerate(text):
        if char in "<([":
            depth += 1
        elif char in ">)]":
            depth -= 1
        elif char == "," and depth == 0:
            args.append(text[start:i].strip())
            start = i + 1
    args.append(text[start:].strip())
    return [arg for arg in args if arg]


def _type_kind(text: str) -> str:
    """The kind of a value of a written type. Option and Result count as what they hold,
    since a binding such as ``Some(x)`` takes it out."""
    text = re.sub(r"^(?:&\s*(?:'\w+\s+)?(?:mut\s+)?)+", "", text.strip()).strip()
    outer = re.fullmatch(r"(?:\w+\s*::\s*)*(\w+)\s*(?:<(.*)>)?", text, re.S)
    if not outer:
        return "rust"
    name, inner = outer.group(1), _generic_args(outer.group(2) or "")
    if name in ("Option", "PyResult", "Result"):
        return _type_kind(inner[0]) if inner else "rust"
    if name in ("Bound", "Borrowed", "Py", "PyRef", "PyRefMut"):
        return "concrete" if inner and re.fullmatch(_CONCRETE, inner[-1]) else "python"
    if re.fullmatch(_CONCRETE, name):
        return "concrete"
    if name.startswith("Py"):
        return "python"
    return "rust"


def _identifier_kind(file: RustFile, name: str, at: int, depth: int) -> str | None:
    """The kind of the latest declaration of ``name`` before ``at``."""
    code = file.code[:at]
    name = re.escape(name)
    found: list[tuple[int, str, int]] = []
    for pattern, how in (
        (rf"\blet\s+(?:mut\s+)?{name}\s*(:(?!:))?", "let"),
        (rf"[(,|]\s*(?:mut\s+)?{name}\s*:(?!:)", "type"),
        (rf"\b(?:Some|Ok|Err)\s*\(\s*(?:mut\s+)?{name}\s*\)\s*=(?![=>])", "iflet"),
        (rf"\b\w+\s*\([^()]*\b{name}\b[^()]*\)\s*(?:if\b[^=]*)?=>", "unknown"),
        (rf"\b{name}\s*(?:if\b[^=]*)?=>", "unknown"),
        (rf"\bfor\s+[^;{{}}]*?\b{name}\b[^;{{}}]*?\bin\b", "unknown"),
        (rf"\|[^|]*\b{name}\b[^|]*\|", "unknown"),
        (rf"\blet\s*(?:mut\s+)?[(\[][^;=]*\b{name}\b[^;=]*[)\]]\s*=", "unknown"),
    ):
        for match in re.finditer(pattern, code):
            found.append((match.start(), how, match.end()))
    if not found:
        return None
    start, how, end = max(found)
    if how == "type":
        return _type_kind(type_text(file.code, end))
    if how == "let":
        if file.code[end - 1] == ":":
            return _type_kind(type_text(file.code, end))
        equals = file.code.find("=", end)
        return _kind(file, expression_text(file.code, equals + 1), start, depth + 1) if equals >= 0 else None
    if how == "iflet":
        return _kind(file, expression_text(file.code, end, ";{"), start, depth + 1)
    return None


def _codec_arguments_allowed(file: RustFile, spans: list[tuple[int, int]]) -> bool:
    """Each argument is a codec or error handler on the list, possibly in Some(...) or a
    tuple, or None."""
    for start, end in spans:
        text = file.src[start:end].strip()
        inner = re.fullmatch(r"(?:Some\s*)?\((.*)\)", text, re.S)
        parts = [part.strip() for part in inner.group(1).split(",") if part.strip()] if inner else [text]
        for part in parts:
            if part == "None":
                continue
            if literal(part, file.consts) not in ALLOWED_CODECS:
                return False
    return True


def _dispatch_problems(file: RustFile, spans: list[tuple[int, int]] | None = None) -> list[tuple[int, str]]:
    """Calls to methods that always dispatch, ffi:: symbols off the list, and raw access."""
    code = file.code
    found = []

    def inside(offset: int) -> bool:
        return spans is None or any(start <= offset < end for start, end in spans)

    names = "|".join(sorted(ALWAYS_DISPATCH, key=len, reverse=True))
    for match in re.finditer(_METHOD.format(names=names), code):
        if not inside(match.start()):
            continue
        name = match.group(1)
        if name.startswith("call_method"):
            args, _ = call_args(code, match.end() - 1)
            if args and literal(file.src[args[0][0]:args[0][1]], file.consts) in _CODEC_METHODS:
                if _codec_arguments_allowed(file, args[1:]):
                    continue
                found.append((match.start(), f"calls {name} with a codec that isn't on the list ({', '.join(sorted(ALLOWED_CODECS))})"))
                continue
        found.append((match.start(), f"calls .{name}(), which runs Python behavior"))
    for match in re.finditer(r"(?:\b(?:Bound|Borrowed|Py)\b(?:\s*::\s*<[^()]*?>)?|>)\s*::\s*(" + names + r")\b", code):
        if inside(match.start()):
            found.append((match.start(), f"names {match.group(1)} by path, which runs Python behavior"))
    for match in re.finditer(r"(?:\.|::)\s*(from_encoded_object|from_object)\s*\(", code):
        if not inside(match.start()):
            continue
        args, _ = call_args(code, match.end() - 1)
        if not _codec_arguments_allowed(file, args[1:]):
            found.append((match.start(), f"decodes with a codec that isn't on the list ({', '.join(sorted(ALLOWED_CODECS))})"))
    for match in re.finditer(r"\bffi\b", code):
        if not inside(match.start()):
            continue
        rest = code[match.end():]
        separator = re.match(r"\s*::\s*", rest)
        if not separator:
            if re.match(r"\s+as\b", rest):
                found.append((match.start(), "renames pyo3::ffi, which hides which C functions it calls"))
            continue
        rest = rest[separator.end():]
        if rest.startswith("*"):
            found.append((match.start(), "imports all of pyo3::ffi"))
        elif rest.startswith("{"):
            group = rest[1:rest.find("}")] if "}" in rest else rest[1:]
            for item in (part.strip() for part in group.split(",")):
                symbol = re.sub(r"^r#", "", item.split(" as ")[0].strip())
                if symbol and symbol != "self" and symbol not in ALLOWED_FFI:
                    found.append((match.start(), f"uses ffi::{symbol}, which isn't on the conversion list"))
                elif symbol == "self" and " as " in item or symbol == "*":
                    found.append((match.start(), "renames or globs pyo3::ffi"))
        else:
            symbol = re.match(r"(?:r#)?(\w+)", rest)
            if symbol and symbol.group(1) not in ALLOWED_FFI:
                found.append((match.start(), f"uses ffi::{symbol.group(1)}, which isn't on the conversion list"))
    for pattern, reason in _RAW_ACCESS:
        for match in pattern.finditer(code):
            if inside(match.start()):
                found.append((match.start(), reason))
    return found


def bindings_problems(file: RustFile) -> list[tuple[int, str]]:
    """Every place a strict PyO3 crate's file runs CPython behavior: (offset, reason)."""
    code = file.code
    found = _dispatch_problems(file)
    names = "|".join(sorted(DISPATCH_ON_ANY | ITEM_ACCESS | {"to_string"}, key=len, reverse=True))
    for match in re.finditer(_METHOD.format(names=names), code):
        name = match.group(1)
        kind = _kind(file, receiver(code, match.start()), match.start())
        if name in ITEM_ACCESS and kind != "concrete":
            found.append((match.start(), f"calls .{name}() on something not known to be a tuple, list or dict, which calls __{name.split('_')[0]}item__"))
        elif name in DISPATCH_ON_ANY and kind == "python":
            found.append((match.start(), f"calls .{name}() on a Python object, which runs Python behavior"))
        elif name == "to_string" and kind in ("python", "concrete"):
            found.append((match.start(), "calls .to_string() on a Python object, which calls str()"))
    for match in re.finditer(r"\b(" + "|".join(_FORMAT_MACROS) + r")\s*!\s*([(\[{])", code):
        found.extend(_format_problems(file, match))
    for start, value in _string_literals(file):
        if _DEBUG_SPEC.search(value.replace("{{", "").replace("}}", "")):
            found.append((start, "formats with Debug, which for a Python object calls repr()"))
    return found


def _string_literals(file: RustFile):
    """(offset, value) of every string literal in the file."""
    opening = re.compile(r'(?<!\w)[bc]?r(#*)"|"')
    position = 0
    while True:
        match = opening.search(file.code, position)
        if not match:
            return
        hashes = match.group(1) or ""
        close = file.code.find('"' + hashes, match.end())
        if close < 0:
            return
        yield match.start(), file.src[match.end():close]
        position = close + 1 + len(hashes)


def _format_problems(file: RustFile, match: re.Match) -> list[tuple[int, str]]:
    code = file.code
    args, _ = call_args(code, match.start(2))
    for index, (start, end) in enumerate(args):
        template = literal(file.src[start:end])
        if template is not None:
            break
    else:
        return []
    problems = []
    clean = template.replace("{{", "").replace("}}", "")
    for capture in re.finditer(r"\{(\w+)[^}]*\}", clean):
        if not capture.group(1).isdigit() and _kind(file, capture.group(1), match.start()) in ("python", "concrete"):
            problems.append((match.start(), f"formats {capture.group(1)}, a Python object, which calls str() or repr()"))
    for start, end in args[index + 1:]:
        expression = code[start:end]
        named = re.match(r"\s*\w+\s*=(?!=)", expression)
        if named:
            expression = expression[named.end():]
        if _kind(file, expression, start) in ("python", "concrete"):
            problems.append((start, f"formats {expression.strip()[:40]}, a Python object, which calls str() or repr()"))
    return problems


def macro_problems(file: RustFile) -> list[tuple[int, str]]:
    """Dispatch inside ``macro_rules!`` bodies of a crate the bindings depend on, which
    the bindings could expand on Python objects."""
    spans = macro_bodies(file.code)
    return _dispatch_problems(file, spans) if spans else []


def feature_problems(spec, features_table: dict) -> list[str]:
    """PyO3 features off the list, from a dependency spec and a crate's [features] table."""
    features = set(spec.get("features", [])) if isinstance(spec, dict) else set()
    for enabled in features_table.values():
        for item in enabled if isinstance(enabled, list) else []:
            if isinstance(item, str) and item.startswith("pyo3/"):
                features.add(item.split("/", 1)[1])
    return sorted(features - ALLOWED_PYO3_FEATURES)
