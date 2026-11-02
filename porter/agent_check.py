"""The command agents run inside a sandbox to check their work.

It's a preview. Its answer doesn't count: harness code reruns the gates outside the
sandbox, with inputs the agent has never seen, before anything merges.

    python -m porter.agent_check checks        # are the check files sound?
    python -m porter.agent_check port [API...] # does the port match the original?
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

from porter.evaluation import Project, verify
from porter.gates.coverage import coverage_gate
from porter.gates.differential import self_comparison_gate

CONFIG = Path.home() / "porter.json"


def main() -> int:
    config = json.loads(CONFIG.read_text())
    mode = sys.argv[1] if len(sys.argv) > 1 else "port"
    reference = config["reference"] | {"python": sys.executable}
    if mode == "checks":
        results = [
            self_comparison_gate("/work/checks", reference, api=config["api"], max_examples=100),
            coverage_gate("/work/checks", reference, minimum=config.get("coverage_minimum", 80), max_examples=100),
        ]
        for gate in results:
            print(f"[{'PASS' if gate.passed else 'FAIL'}] {gate.name}: {gate.summary}")
            if not gate.passed:
                print(json.dumps({k: v for k, v in gate.details.items() if k in ("reach", "divergences", "failures", "uncovered", "output")}, indent=1)[:6000])
        return 0 if all(gate.passed for gate in results) else 1
    project = Project(
        module=config["module"], lib=config["lib"], core_crate=config["core_crate"], py_crate=config["py_crate"],
        reference=reference, checks="/work/checks", tests_visible="/work/upstream/tests",
        approved_differences=config.get("approved_differences", []), approved_crates=config.get("approved_crates", []),
        shared=config.get("shared", {}),
    )
    apis = sys.argv[2:] or config.get("apis")
    result = verify(
        project, "/work/port", "/tmp/porter-site", apis=apis, upstream=not apis, check_license=False, max_examples=60,
    )
    print(result.for_agent())
    print("\nThis is a preview. The harness reruns all gates with new inputs before merging.")
    return 0 if result.passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
