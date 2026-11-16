//! Written by Portwright. Don't edit: the integrity gate accepts this file only unchanged.
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
