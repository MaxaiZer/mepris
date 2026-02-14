use std::io::Write;
use std::process::{Command, Stdio};
use anyhow::{bail, Context};
use which::which;
use crate::config::{PackageManager, Step};
use crate::system::os_info::{Platform, DEFAULT_PACKAGE_MANAGER, OS_INFO};
use crate::runner::logger::Logger;

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

pub fn check_pkg_installed(manager: &PackageManager, pkg: &str) -> anyhow::Result<bool> {
    let cmd = manager.command_check_if_installed(pkg);
    let output = Command::new(&cmd.bin)
        .args(&cmd.args)
        .output()
        .context(format!("Failed to run {} {}", &cmd.bin, cmd.args.join(" ")))?;

    let out = String::from_utf8_lossy(&output.stdout);

    match manager {
        PackageManager::Pacman | PackageManager::Yay | PackageManager::Paru
        | PackageManager::Dnf | PackageManager::Zypper
        | PackageManager::Flatpak | PackageManager::Npm => Ok(output.status.success()),

        PackageManager::Apt => {
            Ok(output.status.success() && out.lines().any(|line| line.starts_with("ii")))
        }

        PackageManager::Scoop => { //first line is "installed apps matching <pkg_name>:" + scoop uses SUBSTRING MATCH. package name is in first column
            Ok(out.lines()
                .skip(1)
                .filter_map(|line| line.split_whitespace().next())
                .any(|name| name == pkg))
        }

        PackageManager::Winget | PackageManager::Choco
        | PackageManager::Brew | PackageManager::Cargo
        => Ok(out.contains(pkg))
    }
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

    let commands = manager.commands_to_install(packages);
    for cmd in commands {
        let status = Command::new(cmd.bin)
            .args(cmd.args)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .context(format!("Failed to install {}", packages.join(", ")))?;

        if !status.success() {
            bail!("Failed to install {}", packages.join(", "));
        }
    }
    Ok(())
}