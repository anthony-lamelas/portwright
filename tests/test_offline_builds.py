"""Gate builds run agent code (build scripts, proc macros, tests) with no network.

Crates are fetched first by ``cargo fetch``, which runs nothing from the port, into a
cache the builds see read-only.
"""

import shutil
import socket
import textwrap
import threading
from pathlib import Path

import pytest

from portwright.gates.rust_quality import rust_quality_gate
from portwright.toolchain import build_python_package, cargo_cache_root, fetch_dependencies, image_available, run_in_toolchain

FIXTURES = Path(__file__).parent / "fixtures"
REPO_TARGET = Path(__file__).resolve().parents[1] / "target" / "offline-test"

pytestmark = pytest.mark.skipif(not image_available(), reason="needs the toolchain image")

# Formatted and lint-clean, so the quality gate gets as far as running it.
BUILD_SCRIPT = """\
//! Tries to reach the network while the crate builds.
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

fn main() {
    let mut addresses = vec![SocketAddr::from(([127, 0, 0, 1], PORT))];
    match ("index.crates.io", 443).to_socket_addrs() {
        Ok(found) => addresses.extend(found),
        Err(error) => println!("cargo:warning=lookup failed: {error}"),
    }
    for address in &addresses {
        if TcpStream::connect_timeout(address, Duration::from_secs(3)).is_ok() {
            println!("cargo:warning=reached the network at {address}");
            std::process::exit(1);
        }
    }
    println!("cargo:rerun-if-changed=build.rs");
}
"""


class Listener:
    """Counts connections to a port on the host's loopback, which host networking reaches."""

    def __init__(self):
        self.server = socket.create_server(("127.0.0.1", 0))
        self.server.settimeout(0.2)
        self.port = self.server.getsockname()[1]
        self.connections = 0
        self.stopped = threading.Event()
        self.thread = threading.Thread(target=self._accept, daemon=True)
        self.thread.start()

    def _accept(self):
        while not self.stopped.is_set():
            try:
                connection, _ = self.server.accept()
            except OSError:
                continue
            self.connections += 1
            connection.close()

    def close(self):
        self.stopped.set()
        self.thread.join()
        self.server.close()


@pytest.fixture
def listener():
    server = Listener()
    yield server
    server.close()


def _fresh(name: str) -> Path:
    path = REPO_TARGET / name
    if path.exists():
        shutil.rmtree(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    return path


def test_a_build_script_cannot_reach_the_network_during_gate_builds(listener):
    port = _fresh("probe-port")
    shutil.copytree(FIXTURES / "textkit_rust", port, ignore=shutil.ignore_patterns("target"))
    (port / "crates/textkit-core/build.rs").write_text(BUILD_SCRIPT.replace("PORT", str(listener.port)))

    # The probe works: given the network, as every build had before, it gets through.
    control = run_in_toolchain(
        ["cargo", "build", "-p", "textkit-core"], port, network=True, target_dir=REPO_TARGET / "probe-control-target",
    )
    assert control.returncode != 0 and "reached the network" in control.stderr, control.stderr[-2000:]
    assert listener.connections > 0
    listener.connections = 0

    quality = rust_quality_gate(port, py_crate_name="textkit-py")
    assert quality.passed, quality.details
    built = build_python_package(port, "crates/textkit-py", REPO_TARGET / "probe-site")
    assert built.returncode == 0, built.stderr[-2000:]
    assert "lookup failed" in built.stderr
    assert listener.connections == 0


def test_a_port_with_a_crates_io_dependency_builds_offline_after_fetch(monkeypatch):
    monkeypatch.setenv("PORTWRIGHT_CARGO_CACHE", str(_fresh("cache-memchr")))
    port = _fresh("memchr-port")
    (port / "src").mkdir(parents=True)
    (port / "Cargo.toml").write_text(textwrap.dedent("""
        [package]
        name = "uses-memchr"
        version = "0.1.0"
        edition = "2021"
        [dependencies]
        memchr = "2"
    """))
    (port / "src/lib.rs").write_text("pub fn find(text: &[u8]) -> Option<usize> {\n    memchr::memchr(b'x', text)\n}\n")

    # Builds are offline: with nothing fetched, the dependency can't be downloaded.
    unfetched = run_in_toolchain(["cargo", "build"], port, fetch=False)
    assert unfetched.returncode != 0
    assert "offline" in unfetched.stderr, unfetched.stderr[-2000:]

    built = run_in_toolchain(["cargo", "build"], port)
    assert built.returncode == 0, built.stderr[-2000:]
    assert list((cargo_cache_root() / "registry" / "src").glob("*/memchr-*"))
    assert (port / "Cargo.lock").is_file()  # the version the fetch chose

    # Builds can't change the crates later builds compile.
    write = run_in_toolchain(["bash", "-c", "touch /opt/cargo/registry/planted"], port)
    assert write.returncode != 0 and "Read-only" in write.stderr, write.stderr
    assert not (cargo_cache_root() / "registry" / "planted").exists()


def test_the_fetch_step_ignores_the_ports_toolchain_and_cargo_config(monkeypatch):
    monkeypatch.setenv("PORTWRIGHT_CARGO_CACHE", str(_fresh("cache-toolchain")))
    port = _fresh("toolchain-port")
    (port / "src").mkdir(parents=True)
    (port / "Cargo.toml").write_text('[package]\nname = "plain"\nversion = "0.1.0"\nedition = "2021"\n')
    (port / "src/lib.rs").write_text("")
    # Either file could make the fetch step, which has network, run the port's own program.
    fake = port / "fake-toolchain/bin"
    fake.mkdir(parents=True)
    for tool in ("cargo", "rustc"):
        (fake / tool).write_text("#!/bin/sh\ntouch /opt/cargo/registry/planted\n")
        (fake / tool).chmod(0o755)
    (port / "rust-toolchain.toml").write_text('[toolchain]\npath = "/work/fake-toolchain"\n')
    (port / ".cargo").mkdir()
    (port / ".cargo/config.toml").write_text('[build]\nrustc = "/work/fake-toolchain/bin/rustc"\n')

    fetched = fetch_dependencies(port)
    assert fetched.returncode == 0, fetched.stderr[-2000:]
    assert not (cargo_cache_root() / "registry" / "planted").exists()
