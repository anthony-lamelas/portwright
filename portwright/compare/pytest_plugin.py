"""Connects check files to a comparison session and writes results for the gates."""

from __future__ import annotations

import json
import os
from pathlib import Path

from hypothesis import HealthCheck, settings

from portwright.compare import session as session_module

FAILURE_TEXT_LIMIT = 4000


def _config() -> dict:
    return json.loads(Path(os.environ["PORTER_CONFIG"]).read_text())


def pytest_configure(config) -> None:
    options = _config()
    settings.register_profile(
        "porter",
        max_examples=options.get("max_examples", 200),
        deadline=None,
        database=None,
        print_blob=False,
        suppress_health_check=[HealthCheck.too_slow, HealthCheck.data_too_large],
    )
    settings.load_profile("porter")
    active = session_module.Session(
        session_module.Side(**options["reference"]),
        session_module.Side(**options["candidate"]),
        timeout=options.get("timeout", session_module.DEFAULT_TIMEOUT),
        approved=options.get("approved", ()),
    )
    active.start()
    session_module.set_current(active)
    config.porter_failures = []


def pytest_runtest_logreport(report) -> None:
    if report.failed:
        _failures.append({"test": report.nodeid, "text": str(report.longrepr)[-FAILURE_TEXT_LIMIT:]})


_failures: list[dict] = []


def pytest_sessionfinish(session, exitstatus) -> None:
    active = session_module.current()
    active.close()
    Path(_config()["results"]).write_text(
        json.dumps(
            {
                "exit_status": int(exitstatus),
                "stats": active.stats,
                "divergences": active.divergences,
                "failures": _failures,
            },
            indent=1,
        )
    )
