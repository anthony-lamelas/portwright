"""Just enough Rust lexing for the integrity gate's static checks.

The checks look for API names, so comments must not hide code (``"//"`` inside a string
used to cut a line short) and strings must not look like code. Each file is read as two
texts with the same offsets: ``src`` keeps string literals and drops comments, and
``code`` also blanks the inside of every string and char literal.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field

_RAW_STRING = re.compile(r'(?:b|c)?r(#*)"')
_CHAR = re.compile(r"'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F_]*\}|.)|[^\\'\n])'")
_IDENT_CHAR = re.compile(r"\w")
_STRING_LITERAL = re.compile(r'''^(b|c)?(?:r(#*)"(.*)"\2|"((?:[^"\\]|\\.)*)")$''', re.S)
_SIMPLE_ESCAPES = {"n": "\n", "t": "\t", "r": "\r", "0": "\0", "\\": "\\", '"': '"', "'": "'"}


@dataclass
class RustFile:
    text: str
    src: str
    code: str
    consts: dict[str, str] = field(default_factory=dict)

    def line(self, offset: int) -> int:
        return self.code.count("\n", 0, offset) + 1

    def snippet(self, offset: int) -> str:
        lines = self.text.splitlines()
        number = self.line(offset)
        return lines[number - 1].strip()[:120] if number <= len(lines) else ""


def read_rust(text: str) -> RustFile:
    src, code = [], []
    i, n = 0, len(text)

    def blank(chunk: str) -> str:
        return re.sub(r"[^\n]", " ", chunk)

    while i < n:
        if text.startswith("//", i):
            end = text.find("\n", i)
            end = n if end < 0 else end
            src.append(blank(text[i:end]))
            code.append(blank(text[i:end]))
            i = end
            continue
        if text.startswith("/*", i):
            depth, j = 0, i
            while j < n:
                if text.startswith("/*", j):
                    depth, j = depth + 1, j + 2
                elif text.startswith("*/", j):
                    depth, j = depth - 1, j + 2
                    if depth == 0:
                        break
                else:
                    j += 1
            src.append(blank(text[i:j]))
            code.append(blank(text[i:j]))
            i = j
            continue
        raw = _RAW_STRING.match(text, i)
        if raw and (i == 0 or not _IDENT_CHAR.match(text[i - 1])):
            close = text.find('"' + raw.group(1), raw.end())
            end = n if close < 0 else close + 1 + len(raw.group(1))
            src.append(text[i:end])
            code.append(text[i:raw.end()] + blank(text[raw.end():end - 1 - len(raw.group(1))]) + text[end - 1 - len(raw.group(1)):end])
            i = end
            continue
        if text[i] == '"':
            j = i + 1
            while j < n and text[j] != '"':
                j += 2 if text[j] == "\\" else 1
            end = min(j + 1, n)
            src.append(text[i:end])
            code.append('"' + blank(text[i + 1:end - 1]) + ('"' if end - i > 1 else ""))
            i = end
            continue
        if text[i] == "'":
            char = _CHAR.match(text, i)
            if char:
                src.append(char.group())
                code.append("'" + blank(char.group()[1:-1]) + "'")
                i = char.end()
                continue
        src.append(text[i])
        code.append(text[i])
        i += 1
    result = RustFile(text, "".join(src), "".join(code))
    for match in re.finditer(r"\b(?:const|static)\s+(\w+)\s*:\s*&\s*(?:'static\s+)?(?:str|CStr)\s*=\s*([^;]+);", result.code):
        value = literal(result.src[match.start(2):match.end(2)])
        if value is not None:
            result.consts[match.group(1)] = value
    return result


def literal(expression: str, consts: dict[str, str] | None = None) -> str | None:
    """The value of a string literal (plain, raw, byte or C), or of a const naming one."""
    expression = expression.strip()
    if consts and expression in consts:
        return consts[expression]
    match = _STRING_LITERAL.match(expression)
    if not match:
        return None
    if match.group(3) is not None:
        return match.group(3)
    return re.sub(r"\\(u\{[0-9a-fA-F]+\}|x[0-9a-fA-F]{2}|.)", _unescape, match.group(4), flags=re.S)


def _unescape(match: re.Match) -> str:
    escape = match.group(1)
    if escape.startswith("u{"):
        return chr(int(escape[2:-1], 16))
    if escape.startswith("x"):
        return chr(int(escape[1:], 16))
    return _SIMPLE_ESCAPES.get(escape, "")


def call_args(code: str, open_index: int) -> tuple[list[tuple[int, int]], int]:
    """Spans of the top-level arguments of the call whose ``(`` is at ``open_index``."""
    depth, spans, start = 0, [], open_index + 1
    for i in range(open_index, len(code)):
        char = code[i]
        if char in "([{":
            depth += 1
        elif char in ")]}":
            depth -= 1
            if depth == 0:
                if code[start:i].strip():
                    spans.append((start, i))
                return spans, i
        elif char == "," and depth == 1:
            spans.append((start, i))
            start = i + 1
    return spans, len(code)


def _back_over_brackets(code: str, index: int) -> int:
    """Index of the bracket that opens the one closing at ``index``."""
    pairs = {")": "(", "]": "[", "}": "{", ">": "<"}
    closing = code[index]
    depth = 0
    for i in range(index, -1, -1):
        if code[i] == closing:
            depth += 1
        elif code[i] == pairs[closing]:
            depth -= 1
            if depth == 0:
                return i
    return 0


def receiver(code: str, dot: int) -> str:
    """The expression a method call at ``dot`` (the index of its ``.``) is made on."""
    i = dot - 1
    while i >= 0:
        while i >= 0 and (code[i].isspace() or code[i] == "?"):
            i -= 1
        if i >= 0 and code[i] in ")]":
            i = _back_over_brackets(code, i) - 1
            while i >= 0 and code[i].isspace():
                i -= 1
            if i >= 0 and code[i] == ">":  # a turbofish: name::<T>(...)
                i = _back_over_brackets(code, i) - 1
                while i >= 0 and code[i] in ": \n\t":
                    i -= 1
        start = i
        while i >= 0 and (code[i].isalnum() or code[i] == "_"):
            i -= 1
        if i == start:
            break
        j = i
        while j >= 0 and code[j].isspace():
            j -= 1
        if j >= 0 and code[j] == "." and (j == 0 or code[j - 1] != "."):
            i = j - 1
            continue
        if j >= 1 and code[j - 1:j + 1] == "::":
            i = j - 2
            continue
        break
    return code[i + 1:dot].strip()


def chain(expression: str) -> list[str]:
    """Split ``a.b(x).c::<T>()?`` into its top-level ``.`` segments."""
    segments, depth, start = [], 0, 0
    for i, char in enumerate(expression):
        if char in "([{<":
            depth += 1
        elif char in ")]}>":
            depth -= 1
        elif char == "." and depth == 0 and expression[i - 1:i + 1] != ".." and expression[i:i + 2] != "..":
            segments.append(expression[start:i])
            start = i + 1
    segments.append(expression[start:])
    return [segment.strip().rstrip("?").strip() for segment in segments]


def type_text(code: str, start: int) -> str:
    """A type written at ``start``, up to the ``,``, ``)``, ``=``, ``{``, ``;`` or ``|`` ending it."""
    depth = 0
    for i in range(start, len(code)):
        char = code[i]
        if char in "<([":
            depth += 1
        elif char in ">)]" and not (char == ">" and code[i - 1] == "-"):
            if depth == 0:
                return code[start:i]
            depth -= 1
        elif char in ",={;|" and depth == 0:
            return code[start:i]
    return code[start:]


def expression_text(code: str, start: int, stops: str = ";") -> str:
    """An expression at ``start``, up to a top-level character in ``stops``."""
    depth = 0
    for i in range(start, len(code)):
        char = code[i]
        if char in "([":
            depth += 1
        elif char in ")]":
            if depth == 0:
                return code[start:i]
            depth -= 1
        elif char == "{" and "{" not in stops:
            depth += 1
        elif char == "}" and "{" not in stops:
            if depth == 0:
                return code[start:i]
            depth -= 1
        elif char in stops and depth == 0:
            return code[start:i]
    return code[start:]


def macro_bodies(code: str) -> list[tuple[int, int]]:
    """Spans of every ``macro_rules!`` body."""
    spans = []
    for match in re.finditer(r"\bmacro_rules\s*!\s*\w+\s*([({\[])", code):
        _, end = call_args(code, match.start(1))
        spans.append((match.start(1), end))
    return spans
