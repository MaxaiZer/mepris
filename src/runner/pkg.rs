use std::io::Write;
use anyhow::{bail};
use which::which;
use crate::config::{Step};
use crate::system::os_info::{Platform, DEFAULT_PACKAGE_MANAGER, OS_INFO};
use crate::runner::logger::Logger;
use crate::system::pkg::PackageManager;

pub fn resolve_step_package_manager(step: &Step) -> PackageManager {
    if let Some(source) = &step.package_source {
        if let Some(manager) = source
            .get_package_managers()
            .iter()
            .find(|m| which(m.command()).is_ok())
        {
            return manager.clone();
        } else {
            return source.get_package_managers()[0].clone();
        }
    }

    if let Some(win_pm) = step
        .defaults
        .as_ref()
        .and_then(|d| d.windows_package_manager.clone())
        && OS_INFO.platform == Platform::Windows
    {
        return win_pm;
    }

    DEFAULT_PACKAGE_MANAGER.clone()
}

pub fn install_packages(
    packages: &[String],
    manager: &PackageManager,
    logger: &mut Logger<impl Write>,
) -> anyhow::Result<()> {
    if which(manager.command()).is_err() {
        bail!("Package manager {} not found", manager.command());
    }

    logger.log(&format!(
        "📦 PROGRESS Installing packages: {}",
        packages.join(", ")
    ))?;

    manager.install(packages)
}