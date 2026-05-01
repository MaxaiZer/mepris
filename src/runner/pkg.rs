use crate::config::Step;
use crate::logging::EventType;
use crate::system::os_info::{DEFAULT_PACKAGE_MANAGER, OS_INFO, Platform};
use crate::system::pkg::PackageManager;
use anyhow::bail;
use tracing::info;

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

pub fn install_packages(packages: &[String], manager: &PackageManager) -> anyhow::Result<()> {
    if std::env::var("MEPRIS_INSTALL_COMMAND").is_err() && !manager.is_available() {
        bail!("Package manager {} not found", manager);
    }

    info!(event_type=%EventType::PackagesInstallStarted, packages = packages.join(", "));
    manager.install(packages)
}
