"""Runs Rust tools inside the toolchain image, so the host doesn't need Rust installed.

Inside an agent sandbox the same functions run tools directly, because the sandbox
already is the image. ``PORTER_IN_SANDBOX`` tells them which situation they're in.
"""

from __future__ import annotations

import fcntl
import hashlib
import os
import shutil
import subprocess
import sys
import tempfile
import threading
import uuid
from pathlib import Path
import platform

IMAGE = "portwright-toolchain:dev"
REPO_ROOT = Path(__file__).resolve().parents[1]
# Where the image's cargo (CARGO_HOME=/opt/cargo) keeps downloaded crates. The image
# ships without them, so the host cache below is mounted over these.
CONTAINER_CARGO_CACHE = {"registry": "/opt/cargo/registry", "git": "/opt/cargo/git"}
# Copied into the fetch step's staging folder without these: a workspace's own cargo
# config or toolchain file could run a program the agent wrote, with network.
FETCH_IGNORES = ("target", ".git", ".cargo", "rust-toolchain", "rust-toolchain.toml")
# The toolchain each separate cargo workspace builds with. cargo-fuzz needs nightly.
FETCH_TOOLCHAINS = {"Cargo.toml": "stable", "fuzz/Cargo.toml": "nightly"}


def in_sandbox() -> bool:
    return os.environ.get("PORTER_IN_SANDBOX") == "1"


def image_available(image: str = IMAGE) -> bool:
    if in_sandbox():
        return True
    done = subprocess.run(["docker", "image", "inspect", image], capture_output=True, check=False)
    return done.returncode == 0


def python_mismatch(image: str = IMAGE) -> str | None:
    """Explain any Python version difference between this process and the toolchain.

    Agents preview their work with the toolchain's Python, and the gates run the
    original library with this one. Standard library behavior changes between patch
    releases, so a mismatch makes correct fixes fail the gates.
    """
    done = subprocess.run(
        ["docker", "run", "--rm", image, "python", "-c", "import platform; print(platform.python_version())"],
        capture_output=True, text=True, check=False,
    )
    if done.returncode != 0:
        return f"can't read the toolchain's Python version: {done.stderr[-300:]}"
    theirs, ours = done.stdout.strip(), platform.python_version()
    if theirs == ours:
        return None
    return f"this harness runs Python {ours}, but the toolchain has {theirs}. Run: uv venv --python {theirs} .venv"


def cargo_cache_root() -> Path:
    """The host folder that holds crates the gates' builds use.

    It is separate from the agents' cache, which agents can write to. It must not be in
    the host's temporary directory, which some Docker installs can't see (see
    ``isolation.scratch_root``). ``PORTWRIGHT_CARGO_CACHE`` moves it.
    """
    root = Path(os.environ.get("PORTWRIGHT_CARGO_CACHE", REPO_ROOT / "target" / "cargo-cache")).resolve()
    if root.is_relative_to(tempfile.gettempdir()):
        raise ValueError(f"the cargo cache must not be in a temporary directory: {root}")
    for name in CONTAINER_CARGO_CACHE:
        (root / name).mkdir(parents=True, exist_ok=True)
    return root


def run_in_toolchain(
    command: list[str],
    workspace: str | Path,
    *,
    extra_mounts: dict[str, str] | None = None,
    target_dir: str | Path | None = None,
    env: dict[str, str] | None = None,
    read_only: bool = False,
    network: bool = False,
    timeout: float = 1800.0,
    image: str = IMAGE,
    fetch: bool | tuple[str, ...] = True,
) -> subprocess.CompletedProcess:
    """Run a command with the workspace mounted at /work and return its result.

    Builds run agent code (build scripts, proc macros, tests), so by default the
    container has no network and cargo is offline. The workspace's crates are fetched
    first, by ``fetch_dependencies``, into a cache the build sees read-only. If that
    fetch fails, its result is returned instead. ``fetch`` names the manifests to fetch
    for (``True``: the workspace's own Cargo.toml); ``fetch=False`` skips it.

    ``network=True`` gives the container the host network and a writable crate cache.
    Use it only for commands that run no code from the workspace, like ``cargo fetch``.
    """
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
    if not network and fetch:
        manifests = ("Cargo.toml",) if fetch is True else tuple(fetch)
        fetched = fetch_dependencies(workspace, manifests, timeout=timeout, image=image)
        if fetched.returncode != 0:
            return fetched
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
    cache = cargo_cache_root()
    for name, container_path in CONTAINER_CARGO_CACHE.items():
        # Read-only, so a build script can't change a crate's source for later builds.
        docker += ["-v", f"{cache / name}:{container_path}{'' if network else ':ro'}"]
    if network:
        # This machine's default Docker bridge has no outbound access, so the fetch step
        # uses the host network. Agent sandboxes never do: see portwright/sandbox.py.
        docker += ["--network", "host"]
    else:
        docker += ["--network", "none", "-e", "CARGO_NET_OFFLINE=true"]
    for host_path, container_path in (extra_mounts or {}).items():
        docker += ["-v", f"{Path(host_path).resolve()}:{container_path}"]
    return subprocess.run(
        [*docker, image, *command], capture_output=True, text=True, timeout=timeout, check=False
    )


_fetch_lock = threading.Lock()
_fetched: set[tuple[Path, tuple[str, ...], str]] = set()


def _manifests_key(workspace: Path) -> str:
    """A hash of every manifest and lock file, which decide what a fetch downloads."""
    digest = hashlib.sha256()
    for directory, subdirectories, files in os.walk(workspace):
        subdirectories[:] = sorted(name for name in subdirectories if name not in FETCH_IGNORES)
        for name in sorted(files):
            if name in ("Cargo.toml", "Cargo.lock"):
                path = Path(directory) / name
                digest.update(str(path.relative_to(workspace)).encode() + b"\0" + path.read_bytes() + b"\0")
    return digest.hexdigest()


def fetch_dependencies(
    workspace: str | Path,
    manifests: tuple[str, ...] = ("Cargo.toml",),
    *,
    timeout: float = 1800.0,
    image: str = IMAGE,
) -> subprocess.CompletedProcess:
    """Download the crates the workspace's manifests need into the gates' crate cache.

    This is the only build step with network. It runs ``cargo fetch``, which reads
    manifests but compiles and runs nothing, on a copy of the workspace without its
    cargo config or toolchain file. A Cargo.lock the fetch creates is copied back, so
    builds use the versions it downloaded. Missing manifests are skipped, and manifests
    already fetched by this process aren't fetched again.
    """
    workspace = Path(workspace).resolve()
    done = subprocess.CompletedProcess(["cargo", "fetch"], 0, "", "")
    manifests = tuple(path for path in manifests if (workspace / path).is_file())
    if in_sandbox() or not manifests:
        return done
    cache = cargo_cache_root()
    with _fetch_lock, open(cache / ".fetch-lock", "w") as lock:
        if (cache, manifests, _manifests_key(workspace)) in _fetched and (cache / "registry" / "cache").is_dir():
            return done
        fcntl.flock(lock, fcntl.LOCK_EX)  # other processes may share the cache
        staging = cache / "staging" / uuid.uuid4().hex
        try:
            shutil.copytree(workspace, staging, symlinks=True, ignore=shutil.ignore_patterns(*FETCH_IGNORES))
            for manifest in manifests:
                toolchain = FETCH_TOOLCHAINS.get(manifest, "stable")
                done = run_in_toolchain(
                    ["cargo", f"+{toolchain}", "fetch", "--manifest-path", manifest],
                    staging, network=True, timeout=timeout, image=image, env={"RUSTUP_TOOLCHAIN": toolchain},
                )
                if done.returncode != 0:
                    done.stderr = f"cargo fetch for {manifest} failed:\n{done.stderr}"
                    return done
                lock_file = Path(manifest).with_name("Cargo.lock")
                if (staging / lock_file).is_file() and not (workspace / lock_file).exists():
                    shutil.copyfile(staging / lock_file, workspace / lock_file)
        finally:
            shutil.rmtree(staging, ignore_errors=True)
        _fetched.add((cache, manifests, _manifests_key(workspace)))
        return done


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
