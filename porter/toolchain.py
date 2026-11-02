"""Runs Rust tools inside the toolchain image, so the host doesn't need Rust installed.

Inside an agent sandbox the same functions run tools directly, because the sandbox
already is the image. ``PORTER_IN_SANDBOX`` tells them which situation they're in.
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

IMAGE = "rust-porter-toolchain:dev"


def in_sandbox() -> bool:
    return os.environ.get("PORTER_IN_SANDBOX") == "1"


def image_available(image: str = IMAGE) -> bool:
    if in_sandbox():
        return True
    done = subprocess.run(["docker", "image", "inspect", image], capture_output=True, check=False)
    return done.returncode == 0


def run_in_toolchain(
    command: list[str],
    workspace: str | Path,
    *,
    extra_mounts: dict[str, str] | None = None,
    target_dir: str | Path | None = None,
    env: dict[str, str] | None = None,
    read_only: bool = False,
    network: bool = True,
    timeout: float = 1800.0,
    image: str = IMAGE,
) -> subprocess.CompletedProcess:
    """Run a command with the workspace mounted at /work and return its result."""
    workspace = Path(workspace).resolve()
    if in_sandbox():
        return subprocess.run(
            command, cwd=workspace, capture_output=True, text=True,
            timeout=timeout, check=False, env={**os.environ, **(env or {})},
        )
    if workspace.is_relative_to(tempfile.gettempdir()):
        # Some Docker installs can't see host temporary directories, and mount an empty
        # folder without any error. Refusing here is clearer than "file not found" later.
        raise ValueError(f"workspace must not be in a temporary directory: {workspace}")
    docker = [
        "docker", "run", "--rm",
        "--user", f"{os.getuid()}:{os.getgid()}",
        "-e", "HOME=/tmp",
        "-e", "CARGO_TARGET_DIR=/work/target",
        "-v", f"{workspace}:/work{':ro' if read_only else ''}",
    ]
    if target_dir is not None:
        Path(target_dir).mkdir(parents=True, exist_ok=True)
        # Docker would otherwise create this mount point as root.
        (workspace / "target").mkdir(exist_ok=True)
        docker +=["-v", f"{Path(target_dir).resolve()}:/work/target"]
    for key, value in (env or {}).items():
        docker += ["-e", f"{key}={value}"]
    # This machine's default Docker bridge has no outbound access, so trusted build
    # steps use the host network. Agent sandboxes never do: see porter/sandbox.py.
    docker += ["--network", "host" if network else "none"]
    for host_path, container_path in (extra_mounts or {}).items():
        docker += ["-v", f"{Path(host_path).resolve()}:{container_path}"]
    return subprocess.run(
        [*docker, image, *command], capture_output=True, text=True, timeout=timeout, check=False
    )


def host_target_dir(workspace: str | Path, target_dir: str | Path | None) -> Path:
    if target_dir is not None:
        return Path(target_dir).resolve()
    if in_sandbox():
        return Path(os.environ.get("CARGO_TARGET_DIR", Path(workspace) / "target")).resolve()
    return Path(workspace).resolve() / "target"


def build_python_package(
    workspace: str | Path,
    crate: str,
    install_dir: str | Path,
    *,
    target_dir: str | Path | None = None,
    timeout: float = 1800.0,
) -> subprocess.CompletedProcess:
    """Build a crate's Python wheel, then unpack it into ``install_dir``."""
    install_dir = Path(install_dir).resolve()
    wheels = host_target_dir(workspace, target_dir) / "porter-wheels"
    wheels.mkdir(parents=True, exist_ok=True)
    for old in wheels.glob("*.whl"):
        old.unlink()
    out = str(wheels) if in_sandbox() else "/work/target/porter-wheels"
    built = run_in_toolchain(
        [
            "maturin", "build", "--release", "--compatibility", "linux",
            "-i", "python3.12", "-m", f"{crate}/Cargo.toml", "-o", out,
        ],
        workspace,
        target_dir=target_dir,
        timeout=timeout,
    )
    if built.returncode != 0:
        return built
    wheel = max(wheels.glob("*.whl"), key=lambda path: path.stat().st_mtime)
    if install_dir.exists():
        shutil.rmtree(install_dir)
    install_dir.mkdir(parents=True)
    import zipfile

    with zipfile.ZipFile(wheel) as archive:
        archive.extractall(install_dir)
    return built
