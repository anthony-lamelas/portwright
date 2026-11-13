"""Keeps the original's license and credit, and checks every dependency's license."""

from __future__ import annotations

import re
from pathlib import Path

from portwright.gates import GateResult
from portwright.toolchain import host_target_dir, in_sandbox, run_in_toolchain

ALLOWED = ["MIT", "Apache-2.0", "Apache-2.0 WITH LLVM-exception","BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib", "Unicode-3.0", "MIT-0", "BSL-1.0", "Unlicense", "CC0-1.0"]
# Wording of the licenses cargo-deny allows above. Several reuse MIT's grant sentence
# (MIT-0, BSL-1.0, Unicode-3.0), and ISC's grant also covers 0BSD.
PERMISSIVE_MARKERS = (
    "Permission is hereby granted, free of charge",  # MIT, MIT-0, BSL-1.0, Unicode-3.0
    "Redistribution and use in source and binary forms",  # BSD
    "Apache License",
    "with or without fee is hereby granted",  # ISC, old and new wording
    "free and unencumbered software released into the public domain",  # Unlicense
    "Permission is granted to anyone to use this software for any purpose",  # Zlib
    "CC0 1.0 Universal",
)
# Copyleft wording refuses a license even beside permissive wording, as in "MIT or GPL"
# or a LICENSE that bundles LGPL parts: a person decides those, not a string match.
COPYLEFT_MARKERS = (
    "GENERAL PUBLIC LICENSE",  # GPL, LGPL, AGPL ("GNU Lesser General Public License" too)
    "GNU LIBRARY GENERAL PUBLIC",
    "Mozilla Public License",
    "Eclipse Public License",
    "EUROPEAN UNION PUBLIC LICENCE",
    "COMMON DEVELOPMENT AND DISTRIBUTION LICENSE",
    "Open Software License",
    "Creative Commons Attribution-ShareAlike",
    "copyleft",
)
# Top-level files that hold a license: LICENSE, LICENCE, COPYING or UNLICENSE, in any
# case, with an optional suffix such as .txt, .rst, -MIT or .LESSER.
LICENSE_FILE = re.compile(r"(?:LICEN[CS]E|COPYING|UNLICENSE)(?:[-_.][\w.-]+)?", re.IGNORECASE)

DENY_TEMPLATE = """[licenses]
allow = {allow}
confidence-threshold = 0.8
[graph]
all-features = false
"""


def is_permissive(license_text: str) -> bool:
    text = " ".join(license_text.split()).casefold()  # wording wraps differently file to file
    if any(marker.casefold() in text for marker in COPYLEFT_MARKERS):
        return False
    return any(marker.casefold() in text for marker in PERMISSIVE_MARKERS)


def license_files(source) -> list[Path]:
    """The license files at the top of a source tree, in name order."""
    return sorted(
        (
            path for path in Path(source).iterdir()
            if path.is_file() and LICENSE_FILE.fullmatch(path.name) and path.suffix != ".py"
        ),
        key=lambda path: path.name.casefold(),
    )


def write_attribution(workspace, upstream_license_text: str, name: str, url: str, commit: str) -> None:
    """Add the original's license and a notice that this is an unofficial port."""
    workspace = Path(workspace)
    (workspace / "LICENSE.upstream").write_text(upstream_license_text, encoding="utf-8")
    (workspace / "NOTICE").write_text(
        f"This is an experimental, unofficial Rust port of {name} ({url}), at commit {commit}.\n"
        "It isn't affiliated with or endorsed by the original authors.\n"
        "The original's license is in LICENSE.upstream.\n",
        encoding="utf-8",
    )


def license_gate(workspace, upstream_license_text: str, name: str, *, target_dir=None) -> GateResult:
    """Fail if credit is missing or any dependency has a license we didn't allow."""
    workspace = Path(workspace)
    problems = []
    copied = workspace / "LICENSE.upstream"
    if not copied.exists() or copied.read_text(encoding="utf-8") != upstream_license_text:
        problems.append("LICENSE.upstream is missing or changed")
    notice = workspace / "NOTICE"
    if not notice.exists() or name not in notice.read_text(encoding="utf-8"):
        problems.append("NOTICE is missing or doesn't credit the original")
    if problems:
        return GateResult("license", False, "; ".join(problems), {"problems": problems})
    config_dir = host_target_dir(workspace, target_dir)
    config_dir.mkdir(parents=True, exist_ok=True)
    allow = "[" + ", ".join(f'"{item}"' for item in ALLOWED) + "]"
    (config_dir / "porter-deny.toml").write_text(DENY_TEMPLATE.format(allow=allow))
    config_path = str(config_dir / "porter-deny.toml") if in_sandbox() else "/work/target/porter-deny.toml"
    done = run_in_toolchain(
        ["cargo", "deny", "--log-level", "error", "--config", config_path, "check", "licenses"],
        workspace, target_dir=target_dir,
    )
    if done.returncode != 0:
        return GateResult("license", False, "a dependency's license isn't allowed", {"output": done.stderr[-3000:]})
    return GateResult("license", True, "credit is present and all dependency licenses are allowed", {})
