"""Gates run agent code in the toolchain image when it's built, and on the host when it isn't.

With the image, the suite exercises the real, contained path. Its temporary files then
live under target/, because some Docker installs can't see the host's /tmp.

Rust builds the suite makes under target/ add up to about 9 GB a run, and they used to
pile up until the disk was full (open item T). After a passing run they are deleted;
after a failing one they stay for inspection. ``PORTWRIGHT_KEEP_TEST_SCRATCH=1`` keeps
them either way. The crate download cache (target/cargo-cache) always stays.
"""

import os
import shutil
from pathlib import Path

from portwright.auth import FALLBACK_OAUTH_TOKEN, OAUTH_TOKEN
from portwright.toolchain import image_available

REPO = Path(__file__).resolve().parents[1]
# Every folder the tests build in. Keep in step with the REPO_TARGET/RUNS constants.
SCRATCH = (
    "orchestrator-tests", "gate-tests", "sandbox-tests", "offline-test", "containment-test", "integrity-wheel",
    "rust-bugs", "pytest-tmp", "test-budget",
)


# Real credentials. No test needs them, and a test that forgot to clear one would read
# it and could print it in an assertion message. Tests set fake values when they need one.
CREDENTIAL_VARIABLES = (OAUTH_TOKEN, FALLBACK_OAUTH_TOKEN, "ANTHROPIC_API_KEY")


def pytest_configure(config):
    for name in CREDENTIAL_VARIABLES:
        os.environ.pop(name, None)
    if not image_available():
        os.environ["PORTWRIGHT_GATES_ON_HOST"] = "1"
    elif not config.option.basetemp:
        (REPO / "target").mkdir(exist_ok=True)
        config.option.basetemp = str(REPO / "target" / "pytest-tmp")
    # A machine-wide ledger belongs to real runs; tests keep their own.
    os.environ.setdefault("PORTWRIGHT_BUDGET_DIR", str(REPO / "target" / "test-budget"))
    # Real runs refuse to start below 10 GB free; the suite's builds need less.
    os.environ.setdefault("PORTWRIGHT_MIN_FREE_GB", "2")


def pytest_sessionfinish(session, exitstatus):
    if exitstatus != 0 or os.environ.get("PORTWRIGHT_KEEP_TEST_SCRATCH") == "1":
        return
    if hasattr(session.config, "workerinput"):  # an xdist worker; the controller cleans up
        return
    if session.config.option.collectonly or not session.testscollected:
        # A `--collect-only` run built nothing, and deleting here would pull the folders
        # out from under a suite running alongside it.
        return
    for name in SCRATCH:
        path = REPO / "target" / name
        if path == Path(session.config.option.basetemp or "/nonexistent"):
            continue  # pytest is still using it; it clears it at the next run's start
        shutil.rmtree(path, ignore_errors=True)
