"""Writes the conformance report. It claims only what was tested, and shows the misses."""

from __future__ import annotations

import json
from pathlib import Path

from porter.state import atomic_write

REQUIRED_GATES = ["main_integrity", "main_rust_quality", "main_license", "main_build", "main_differential", "main_upstream_tests", "holdout_tests", "fuzz"]


def verdict(state: dict) -> tuple[str, list[str]]:
    """Return the overall result and any reasons it isn't a full pass."""
    reasons = []
    if state.get("stopped"):
        reasons.append(f"The run stopped early: {state['stopped']}")
    gates = state.get("gates", {})
    for name in REQUIRED_GATES:
        gate = gates.get(name)
        if gate is None:
            reasons.append(f"`{name}` never ran.")
        elif not gate["passed"]:
            reasons.append(f"`{name}` failed: {gate['summary'][:300]}")
    return ("matched on what was tested" if not reasons else "not conforming"), reasons


def _table(rows: list[list[str]], header: list[str]) -> str:
    def clean(cell) -> str:
        return str(cell).replace("|", "\\|").replace("\n", " ")
    lines = ["| " + " | ".join(header) + " |", "|" + " --- |" * len(header)]
    lines += ["| " + " | ".join(clean(cell) for cell in row) + " |" for row in rows]
    return "\n".join(lines)


def _list_or_none(items: list[str]) -> str:
    return "\n".join(f"- {item}" for item in items) if items else "None."


def build_markdown(paths, config, state: dict, tasks: dict, speed: dict, native: dict) -> str:
    result, reasons = verdict(state)
    gates = state.get("gates", {})
    parts = [
        f"# Conformance report: {config.lib}",
        "**Experimental, unofficial Rust port. Not affiliated with or endorsed by the original authors, and not actively maintained.**",
        f"- **Original:** {config.url} at commit `{state.get('commit', 'unknown')}`",
        f"- **Result:** {result}",
        "\nThis report shows the port matched the original on the inputs tested. It doesn't prove the two are equivalent.",
    ]
    if reasons:
        parts.append("## Why this isn't a full pass\n" + _list_or_none(reasons))

    parts.append("## Gates\n" + _table(
        [[name, "pass" if gate["passed"] else "FAIL", gate["summary"]] for name, gate in sorted(gates.items())],
        ["Gate", "Result", "Detail"],
    ))

    canaries = gates.get("canaries", {}).get("details", {})
    mutation = gates.get("rust_mutation", {}).get("details", {})
    parts.append("## Bugs the checks didn't catch\nPlanted bugs that went unnoticed are blind spots in this report.")
    parts.append("**In copies of the Python:**\n" + _list_or_none(
        [f"`{item['file']}:{item['line']}`: {item['bug']}" for item in canaries.get("missed", [])]))
    parts.append("**In the Rust:**\n" + _list_or_none(
        [f"`{item['file']}:{item['line']}` in `{item['function']}`: {item['change']}" for item in mutation.get("missed", [])]))

    upstream = gates.get("main_upstream_tests", {}).get("details", {})
    holdout = gates.get("holdout_tests", {}).get("details", {})
    private = sorted(set(upstream.get("private_failed", [])) | set(holdout.get("private_failed", [])))
    parts.append(
        "## Upstream tests\n"
        f"{state.get('holdout_count', 0)} test functions were held back from agents and run only by the harness.\n\n"
        "**Failing tests that use private internals or mocks** (reported, not blocking):\n" + _list_or_none(private)
    )

    approved = state.get("approved_differences", [])
    parts.append("## Approved differences\n" + _list_or_none(approved))
    proposals = sorted((paths.control / "differences").glob("*.json")) if (paths.control / "differences").is_dir() else []
    entries = []
    for item in proposals:
        try:
            entries.append(json.dumps(json.loads(item.read_text())))
        except ValueError:
            entries.append(f"{item.name}: unreadable")
    parts.append("## Differences agents proposed (not approved unless listed above)\n" + _list_or_none(entries))
    bugs = paths.public / "upstream_bugs.md"
    parts.append("## Likely bugs in the original\nThe port reproduces these on purpose.\n\n" + (bugs.read_text(encoding="utf-8") if bugs.exists() else "None reported."))

    if speed.get("speedups"):
        rows = []
        for name, ratio in sorted(speed["speedups"].items()):
            reference = speed["reference"].get(name, {}).get("median_ns")
            port = speed["port"].get(name, {}).get("median_ns")
            rows.append([name, f"{reference:,.0f}" if reference else "n/a", f"{port:,.0f}" if port else "n/a", f"{ratio:.2f}x" if ratio else "n/a"])
        parts.append("## Speed through Python\nMedian nanoseconds per call on one machine. Below 1.00x means the port is slower.\n\n" + _table(rows, ["Benchmark", "Original", "Port", "Speedup"]))
    if native.get("median_ns"):
        parts.append("## Native Rust speed\n" + _table(
            [[name, f"{value:,.0f}"] for name, value in sorted(native["median_ns"].items())], ["Benchmark", "Median ns"]))

    review = paths.public / "review.md"
    if review.exists():
        parts.append("## Final design review (advisory, by a reviewer agent)\n" + review.read_text(encoding="utf-8"))

    rejected = state.get("rejected", [])
    if rejected:
        parts.append("## Optimization changes rejected by the harness\n" + _list_or_none([f"{item['task']}: {item['reason'][:300]}" for item in rejected]))

    totals: dict = {}
    rows = []
    for role, usage in sorted(state.get("usage", {}).items()):
        rows.append([role, usage.get("sessions", 0), f"{usage.get('output_tokens', 0):,}", f"${usage.get('estimated_api_cost_usd', 0):.2f}"])
        totals["cost"] = totals.get("cost", 0) + usage.get("estimated_api_cost_usd", 0)
    parts.append(
        "## Usage\nCost is Claude Code's client-side estimate of the API-equivalent price, not a bill.\n\n"
        + _table(rows, ["Role", "Sessions", "Output tokens", "Estimated cost"])
        + f"\n\n**Total estimate:** ${totals.get('cost', 0):.2f}"
    )
    blocked = [f"{task_id}: {task['status']}" for task_id, task in sorted(tasks.items()) if task["status"] != "merged"]
    if blocked:
        parts.append("## Unfinished tasks\n" + _list_or_none(blocked))
    return "\n\n".join(parts) + "\n"


def write_report(paths, config, state: dict, tasks: dict, speed: dict, native: dict) -> Path:
    text = build_markdown(paths, config, state, tasks, speed, native)
    result, reasons = verdict(state)
    (paths.port / "CONFORMANCE.md").write_text(text, encoding="utf-8")
    atomic_write(
        paths.control / "report.json",
        json.dumps({"result": result, "reasons": reasons, "gates": state.get("gates", {}), "usage": state.get("usage", {}), "speed": speed, "native": native}, indent=1, default=str),
    )
    return paths.port / "CONFORMANCE.md"
