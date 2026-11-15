"""Creates the empty Rust workspace agents start from: a core crate and a PyO3 crate."""

from __future__ import annotations

import subprocess
from pathlib import Path

from portwright.gates.license import write_attribution


# Written into every port's bindings crate. Python finds `pkg.sub` through
# sys.modules, so `import isodate.duration` and pickling a class whose module is
# "isodate.duration" need this. Reaching `sys` would let bindings load the original
# library, so the integrity gate accepts this one use only while the file is
# exactly this text (#64).
SUBMODULES_FILE = "portwright_submodules.rs"
SUBMODULES_RS = """//! Written by Portwright. Don't edit: the integrity gate accepts this file only unchanged.
//! Registers the module's submodules in `sys.modules`, so `import pkg.sub` and
//! `from pkg.sub import name` work as they do for the original, and so does pickling
//! a class whose module is `pkg.sub`. Call it at the end of the `#[pymodule]` function.

use pyo3::prelude::*;
use pyo3::types::PyModule;

/// Add each submodule attribute of `root` to `sys.modules` as `<package>.<attribute>`.
/// maturin loads the extension as `<package>.<package>`, so the package is the first
/// part of the name, and every entry stays inside the port's own package.
pub fn register_submodules(root: &Bound<'_, PyModule>) -> PyResult<()> {
    let modules = root.py().import("sys")?.getattr("modules")?;
    let name = root.name()?.to_cow()?.into_owned();
    let package = name.split('.').next().unwrap_or(&name);
    for (key, value) in root.dict().iter() {
        if value.is_instance_of::<PyModule>() {
            let key: String = key.extract()?;
            modules.set_item(format!("{package}.{key}"), value)?;
        }
    }
    Ok(())
}
"""


def crate_names(lib: str) -> tuple[str, str]:
    """Port crates use their own names, so they can't be mistaken for official ones."""
    safe = lib.replace("_", "-")
    return f"{safe}-port-core", f"{safe}-port-py"


def write_scaffold(port: str | Path, lib: str, module: str, license_text: str, url: str, commit: str, *, description: str = "") -> None:
    port = Path(port)
    core, py = crate_names(lib)
    core_lib = core.replace("-", "_")
    files = {
        ".gitignore": "target/\n__pycache__/\n*.so\n",
        "Cargo.toml": f"""[workspace]
resolver = "2"
members = ["crates/{core}", "crates/{py}"]

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "MIT"
publish = false

[profile.release]
# Python integers never overflow, so an unexpected overflow must never wrap silently.
overflow-checks = true
""",
        f"crates/{core}/Cargo.toml": f"""[package]
name = "{core}"
version.workspace = true
edition.workspace = true
license.workspace = true
publish.workspace = true
description = "Experimental, unofficial Rust port of {lib}."

[dependencies]
""",
        f"crates/{core}/src/lib.rs": f"//! Experimental, unofficial Rust port of {lib}. Contains no Python.\n#![forbid(unsafe_code)]\n",
        f"crates/{py}/Cargo.toml": f"""[package]
name = "{py}"
version.workspace = true
edition.workspace = true
license.workspace = true
publish.workspace = true
description = "Python bindings for the experimental, unofficial Rust port of {lib}."

[lib]
name = "{module}"
crate-type = ["cdylib"]

[dependencies]
{core} = {{ path = "../{core}" }}
pyo3 = {{ version = "0.29", features = ["extension-module"] }}
""",
        f"crates/{py}/pyproject.toml": f"""[build-system]
requires = ["maturin>=1.7,<2"]
build-backend = "maturin"

[project]
name = "{lib.replace('_', '-')}-rust-port"
version = "0.1.0"
requires-python = ">=3.12"
description = "Experimental, unofficial Rust port of {lib}."

[tool.maturin]
module-name = "{module}"
""",
        f"crates/{py}/src/lib.rs": f"""//! Thin Python layer over {core}. Python-specific behavior belongs here.
#![forbid(unsafe_code)]

use pyo3::prelude::*;

#[pymodule(name = "{module}")]
fn module_init(_module: &Bound<'_, PyModule>) -> PyResult<()> {{
    let _ = std::any::type_name::<{core_lib}::Placeholder>();
    Ok(())
}}
""",
    }
    files[f"crates/{core}/src/lib.rs"] += "\n/// Replaced when the port begins.\npub struct Placeholder;\n"
    files[f"crates/{py}/src/{SUBMODULES_FILE}"] = SUBMODULES_RS
    for relative, content in files.items():
        path = port / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")
    write_attribution(port, license_text, lib, url, commit)


def init_repo(port: str | Path) -> None:
    port = Path(port)
    identity = ["-c", "user.name=portwright", "-c", "user.email=portwright@users.noreply.github.com", "-c", "commit.gpgsign=false"]
    subprocess.run(["git", "init", "-q", "-b", "main", str(port)], check=True, capture_output=True)
    subprocess.run(["git", *identity, "add", "-A"], cwd=port, check=True, capture_output=True)
    subprocess.run(["git", *identity, "commit", "-qm", "Start port from scaffold"], cwd=port, check=True, capture_output=True)
