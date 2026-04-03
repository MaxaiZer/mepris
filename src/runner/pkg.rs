use crate::config::Step;
use crate::runner::logger::Logger;
use crate::system::os_info::{DEFAULT_PACKAGE_MANAGER, OS_INFO, Platform};
use crate::system::pkg::PackageManager;
use anyhow::bail;

pub fn resolve_step_package_manager(step: &Step) -> PackageManager {
    if let Some(source) = &step.package_source {
        if let Some(manager) = source
            .get_package_managers()
            .iter()
            .find(|m| m.is_available())
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
    logger: &mut Logger,
) -> anyhow::Result<()> {
    if std::env::var("MEPRIS_INSTALL_COMMAND").is_err() && !manager.is_available() {
        bail!("Package manager {} not found", manager);
    }

    logger.log(&format!(
        "📦 PROGRESS Installing packages: {}",
        packages.join(", ")
    ))?;

    manager.install(packages)
}
