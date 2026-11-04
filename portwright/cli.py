"""Command line for running ports and recording the decisions only a person can make."""

from __future__ import annotations

import argparse
import dataclasses
import json
import signal
import sys
import time
from pathlib import Path

from portwright.agents import DockerClaude, LimitReached
from portwright.auth import SUBSCRIPTION, Auth, AuthError
from portwright.orchestrator import Config, Porter
from portwright.sandbox import AllowlistProxy
from portwright.state import JsonStore
from portwright.toolchain import image_available, python_mismatch

LOGIN_RETRY_SECONDS = 30
LIMIT_WAIT_SECONDS = 15 * 60


def _config_path(run_dir: Path) -> Path:
    return run_dir / "control" / "config.json"


def _load_or_create_config(options) -> Config:
    run_dir = Path(options.run_dir).resolve()
    path = _config_path(run_dir)
    if path.exists():
        data = json.loads(path.read_text())
        data["source"], data["run_dir"] = Path(data["source"]), Path(data["run_dir"])
        config = Config(**data)
    else:
        for name in ("lib", "module", "source", "package", "tests", "url"):
            if getattr(options, name, None) is None:
                sys.exit(f"--{name} is required for a new run")
        config = Config(
            lib=options.lib, module=options.module, source=Path(options.source).resolve(),
            package_path=options.package, tests_path=options.tests, url=options.url, run_dir=run_dir,
            canaries_path=options.canaries, reference_note=options.reference_note or "",
            vendor=dict(item.split("=", 1) for item in options.vendor or []),
            strict_bindings=options.strict_bindings,
            seed_crates={name: str(Path(path).resolve()) for name, path in (item.split("=", 1) for item in options.seed or [])},
        )
        config.vendor = {name: str(Path(path).resolve()) for name, path in config.vendor.items()}
    if options.max_agents:
        config.max_agents = options.max_agents
    if options.strict_bindings:  # can be turned on for a run already started, never off
        config.strict_bindings = True
    if options.model:
        config.model = options.model
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(dataclasses.asdict(config), indent=1, default=str))
    return config


def command_run(options) -> int:
    auth = Auth(options.auth)
    try:
        auth.check_environment()
    except AuthError as error:
        sys.exit(str(error))
    if not image_available():
        sys.exit("The toolchain image isn't built. Run: docker build --network host -t portwright-toolchain:dev -f docker/toolchain.Dockerfile docker")
    mismatch = python_mismatch()
    if mismatch:
        sys.exit(mismatch)
    config = _load_or_create_config(options)
    socket_path = Path(config.run_dir) / "build" / "proxy" / "p.sock"
    proxy = AllowlistProxy(socket_path)
    proxy.start()
    backend = DockerClaude(
        socket_path, auth, cargo_cache=Path(config.run_dir) / "build" / "cargo-registry",
        run_label=str(Path(config.run_dir).resolve()), max_agents=config.max_agents,
    )
    sessions = Path(config.run_dir) / "sessions"
    leftover = backend.stop_leftovers(sessions)
    if leftover:
        print(f"Stopped {leftover} agent containers left by an earlier run.", flush=True)
    # Turn SIGTERM into an exit so the cleanup below runs. SIGKILL can't be caught;
    # the containers' own time limit and the startup cleanup cover that case.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(143))
    deadline = time.time() + options.max_wait_hours * 3600
    if options.redo:
        Porter(config, backend).redo_from(options.redo)
    try:
        while True:
            try:
                state = Porter(config, backend).run()
                break
            except LimitReached as limit:
                if not options.wait or time.time() > deadline:
                    print(f"Paused at a usage limit: {limit}\nRun the same command again to resume.")
                    return 3
                # A revoked login usually means the host's own Claude Code just rotated its
                # token, so a fresh one is already on disk. A real usage limit needs a long wait.
                wait = LOGIN_RETRY_SECONDS if "login needs renewing" in str(limit) else LIMIT_WAIT_SECONDS
                print(f"Paused ({str(limit)[:200]}). Retrying in {wait} seconds.", flush=True)
                time.sleep(wait)
    finally:
        backend.stop_leftovers(sessions)
        proxy.stop()
    report = json.loads((Path(config.run_dir) / "control" / "report.json").read_text())
    print(f"Result: {report['result']}")
    for reason in report["reasons"]:
        print(f"  - {reason}")
    print(f"Report: {Path(config.run_dir) / 'port' / 'CONFORMANCE.md'}")
    return 0 if not state["stopped"] else 2


def command_approve(options) -> int:
    """Only this command can accept a behavior difference or a new crate."""
    store = JsonStore(Path(options.run_dir).resolve() / "control" / "run.json")
    key = "approved_crates" if options.crate else "approved_differences"
    store.update(lambda data: data.setdefault(key, []).append(options.value))
    print(f"Approved {options.value}")
    return 0


def command_decide(options) -> int:
    """Record a person's ruling. Every later agent session gets it in its prompt."""
    path = Path(options.run_dir).resolve() / "control" / "human_decisions.md"
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a", encoding="utf-8") as handle:
        handle.write(f"- {options.text.strip()}\n")
    print(f"Recorded in {path}")
    return 0


def command_grant(options) -> int:
    """Let agents change frozen shared files, as a person's answer to a shared_change request.

    The files stop being frozen for the rest of the run. Every change to them still has
    to pass every gate, for every API.
    """
    run_dir = Path(options.run_dir).resolve()
    store = JsonStore(run_dir / "control" / "run.json")
    granted = []

    def unfreeze(data):
        for key in list(data.get("shared", {})):
            if key in options.paths:
                del data["shared"][key]
                granted.append(key)
    store.update(unfreeze)
    missing = sorted(set(options.paths) - set(granted))
    if missing:
        print(f"Not frozen, nothing to do: {', '.join(missing)}")
    if granted:
        # Requests now answered no longer count toward blocking a task (#19), and tasks
        # blocked while waiting for this answer go back in the queue.
        requests = run_dir / "control" / "requests"
        for path in requests.glob("done-*.json") if requests.is_dir() else ():
            path.rename(requests / ("granted-" + path.name[len("done-"):]))
        board = JsonStore(run_dir / "control" / "board.json")

        def reopen(data):
            for task in data.get("tasks", {}).values():
                if task["status"] == "blocked" and "frozen shared code" in task.get("failure", ""):
                    task["status"] = "pending"
                    task["claim"] = None
        board.update(reopen)
        decisions = run_dir / "control" / "human_decisions.md"
        with decisions.open("a", encoding="utf-8") as handle:
            handle.write(f"- Granted: you may now change {', '.join(granted)}. {options.reason.strip()}\n")
        print(f"Unfroze {', '.join(granted)}")
    return 0


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(prog="portwright", description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    run = commands.add_parser("run", help="start or resume a port")
    run.add_argument("--run-dir", required=True)
    run.add_argument("--lib")
    run.add_argument("--module")
    run.add_argument("--source", help="a local clone of the library")
    run.add_argument("--package", help="package path inside the clone")
    run.add_argument("--tests", help="tests path inside the clone")
    run.add_argument("--url")
    run.add_argument("--auth", default=SUBSCRIPTION, choices=["subscription", "api_key"])
    run.add_argument("--redo", metavar="PHASE", help="run PHASE and every later phase again, e.g. after a harness fix")
    run.add_argument("--canaries", help="hand-written planted bugs inside the clone, kept from agents")
    run.add_argument("--reference-note", help="what the original really is, for the report")
    run.add_argument("--vendor", action="append", metavar="NAME=CRATE_DIR",
                     help="a finished crate the port may use, copied in and frozen (repeatable)")
    run.add_argument("--seed", action="append", metavar="NAME=DIR",
                     help="earlier code agents may start from, copied to seed/NAME, not frozen (repeatable)")
    run.add_argument("--strict-bindings", action="store_true",
                     help="for ports of Python's built-ins: the PyO3 layer can't call into Python")
    run.add_argument("--max-agents", type=int)
    run.add_argument("--model")
    run.add_argument("--wait", action="store_true", help="wait out usage limits instead of pausing")
    run.add_argument("--max-wait-hours", type=float, default=72)
    run.set_defaults(handler=command_run)
    approve = commands.add_parser("approve", help="approve a difference signature or a crate")
    approve.add_argument("--run-dir", required=True)
    approve.add_argument("--crate", action="store_true")
    approve.add_argument("value")
    approve.set_defaults(handler=command_approve)
    decide = commands.add_parser("decide", help="record a ruling every later agent session must follow")
    decide.add_argument("--run-dir", required=True)
    decide.add_argument("text")
    decide.set_defaults(handler=command_decide)
    grant = commands.add_parser("grant", help="unfreeze shared files in answer to a shared_change request")
    grant.add_argument("--run-dir", required=True)
    grant.add_argument("--reason", required=True, help="what the change is for; agents see it")
    grant.add_argument("paths", nargs="+", help="frozen paths, as listed in control/run.json")
    grant.set_defaults(handler=command_grant)
    options = parser.parse_args(argv)
    return options.handler(options)


if __name__ == "__main__":
    raise SystemExit(main())
