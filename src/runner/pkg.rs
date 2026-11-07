use crate::config::Step;
use crate::logging::EventType;
use crate::system::os_info::{OS_INFO, Platform, default_package_manager};
use crate::system::pkg::PackageManager;
use anyhow::bail;
use tracing::info;

pub fn resolve_step_package_manager(step: &Step) -> anyhow::Result<Option<PackageManager>> {
    if let Some(source) = &step.package_source {
        let managers = source.get_package_managers();
        return Ok(managers
            .iter()
            .find(|m| m.is_available())
            .or_else(|| managers.first())
            .cloned());
    }

    if let Some(win_pm) = step
        .defaults
        .as_ref()
        .and_then(|d| d.windows_package_manager.clone())
        && OS_INFO.platform == Platform::Windows
    {
        return Ok(Some(win_pm));
    }

    default_package_manager()
}

pub fn install_packages(packages: &[String], manager: &PackageManager) -> anyhow::Result<()> {
    if std::env::var("MEPRIS_INSTALL_COMMAND").is_err() && !manager.is_available() {
        bail!("Package manager {} not found", manager);
    }

    info!(event_type=%EventType::PackagesInstallStarted, packages = packages.join(", "));
    manager.install(packages)
}
