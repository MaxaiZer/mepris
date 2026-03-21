use crate::system::os_info::{OS_INFO, Platform};
use anyhow::{Context, bail};
use serde::Deserialize;
use std::process::{Command, Stdio};
use strum_macros::{Display, EnumIter, EnumString};
use which::which;

#[derive(Debug, Clone)]
struct CommandSpec {
    pub bin: String,
    pub args: Vec<String>,
}

#[derive(Debug, Eq, PartialEq, Hash, Clone)]
pub enum PackageSource {
    Repository(Repository),
    Manager(PackageManager),
}

impl PackageSource {
    pub fn get_package_managers(&self) -> Vec<PackageManager> {
        match self {
            PackageSource::Repository(Repository::Aur) => {
                vec![PackageManager::Yay, PackageManager::Paru]
            }
            PackageSource::Manager(pm) => vec![pm.clone()],
        }
    }
}

#[derive(Debug, Deserialize, EnumIter, Display, PartialEq, Eq, Hash, Clone)]
#[serde(rename_all = "lowercase")]
pub enum Repository {
    Aur,
}

impl Repository {
    pub fn get_package_managers(&self) -> Vec<PackageManager> {
        match self {
            Repository::Aur => {
                vec![PackageManager::Yay, PackageManager::Paru]
            }
        }
    }
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq, EnumIter, Display, Hash, EnumString)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum PackageManager {
    Apt,
    Dnf,
    Pacman,
    Zypper,
    Yay,
    Paru,
    Flatpak,
    Brew,
    Scoop,
    Choco,
    Winget,
    Cargo,
    Npm,
}

impl PackageManager {
    pub fn is_available(&self) -> bool {
        match self {
            Self::Pacman => which("pacman").is_ok(),
            Self::Apt => which("apt-get").is_ok(),
            Self::Dnf => which("dnf").is_ok(),
            Self::Zypper => which("zypper").is_ok(),
            Self::Brew => which("brew").is_ok(),
            Self::Winget => which("winget").is_ok(),
            Self::Yay => which("yay").is_ok(),
            Self::Paru => which("paru").is_ok(),
            Self::Flatpak => which("flatpak").is_ok(),
            Self::Scoop => which("scoop").is_ok(),
            Self::Choco => which("choco").is_ok(),
            Self::Cargo => which("cargo").is_ok(),
            Self::Npm => which("npm").is_ok(),
        }
    }
    pub fn install(&self, pkgs: &[String]) -> anyhow::Result<()> {
        if let Ok(cmd) = std::env::var("MEPRIS_INSTALL_COMMAND") {
            let parts = shell_words::split(&cmd)?;
            let (program, args) = parts.split_first().unwrap();

            let success = Command::new(program)
                .args(args)
                .args(pkgs)
                .output()?
                .status
                .success();

            if success {
                return Ok(());
            } else {
                bail!("Failed to install {}", pkgs.join(", "));
            }
        }

        fn build_cmd(cmd: &str, args: &[&str], pkgs: &[String]) -> CommandSpec {
            CommandSpec {
                bin: cmd.into(),
                args: args
                    .iter()
                    .map(ToString::to_string)
                    .chain(pkgs.iter().cloned())
                    .collect(),
            }
        }

        let commands = match self {
            Self::Flatpak => pkgs
                .iter()
                .map(|pkg| CommandSpec {
                    bin: "flatpak".into(),
                    args: vec!["install", "-y", "flathub", pkg]
                        .into_iter()
                        .map(String::from)
                        .collect(),
                })
                .collect(),

            Self::Winget => pkgs
                .iter()
                .map(|pkg| CommandSpec {
                    bin: "winget".into(),
                    args: vec![
                        "install",
                        "--exact",
                        "--id",
                        pkg,
                        "--source",
                        "winget",
                        "--accept-source-agreements",
                        "--accept-package-agreements",
                    ]
                    .into_iter()
                    .map(String::from)
                    .collect(),
                })
                .collect(),

            Self::Apt => vec![build_cmd("sudo", &["apt-get", "install", "-y"], pkgs)],
            Self::Dnf => vec![build_cmd("sudo", &["dnf", "install", "-y"], pkgs)],
            Self::Pacman => vec![build_cmd(
                "sudo",
                &["pacman", "-S", "--noconfirm", "--needed"],
                pkgs,
            )],
            Self::Yay => vec![build_cmd("yay", &["-S", "--noconfirm", "--needed"], pkgs)],
            Self::Paru => vec![build_cmd("paru", &["-S", "--noconfirm", "--needed"], pkgs)],
            Self::Zypper => vec![build_cmd("sudo", &["zypper", "install", "-y"], pkgs)],
            Self::Brew => vec![build_cmd("brew", &["install"], pkgs)],
            Self::Scoop => vec![build_cmd("scoop.cmd", &["install"], pkgs)],
            Self::Choco => vec![build_cmd("choco", &["install", "-y"], pkgs)],
            Self::Cargo => vec![build_cmd("cargo", &["install"], pkgs)],
            Self::Npm => vec![build_cmd(
                if OS_INFO.platform == Platform::Windows {
                    "npm.cmd"
                } else {
                    "npm"
                },
                &["i", "-g"],
                pkgs,
            )],
        };

        for cmd in &commands {
            let status = Command::new(&cmd.bin)
                .args(&cmd.args)
                .stdin(Stdio::null())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .status()
                .context(format!("Failed to install {}", pkgs.join(", ")))?;

            if !status.success() {
                bail!("Failed to install {}", pkgs.join(", "));
            }
        }

        Ok(())
    }
    pub fn is_installed(&self, pkg: &str) -> anyhow::Result<bool> {
        if let Ok(res) = std::env::var("MEPRIS_IS_INSTALLED_RESULT") {
            return Ok(res == "0");
        }

        let cmd = match self {
            Self::Pacman | Self::Yay | Self::Paru => CommandSpec {
                bin: "pacman".to_string(),
                args: vec!["-Qq".to_string()],
            },
            Self::Apt => CommandSpec {
                bin: "dpkg".to_string(),
                args: vec!["-l".to_string(), pkg.to_string()],
            },
            Self::Dnf | Self::Zypper => CommandSpec {
                bin: "rpm".to_string(),
                args: vec!["-q".to_string(), pkg.to_string()],
            },
            Self::Flatpak => CommandSpec {
                bin: "flatpak".to_string(),
                args: vec!["info".to_string(), pkg.to_string()],
            },
            Self::Brew => CommandSpec {
                bin: "brew".to_string(),
                args: vec![
                    "list".to_string(),
                    "--versions".to_string(),
                    pkg.to_string(),
                ],
            },
            Self::Winget => CommandSpec {
                bin: "winget".to_string(),
                args: vec!["list".to_string(), "--id".to_string(), pkg.to_string()],
            },
            Self::Scoop => CommandSpec {
                bin: "scoop.cmd".to_string(),
                args: vec!["list".to_string(), pkg.to_string()],
            },
            Self::Choco => CommandSpec {
                bin: "choco".to_string(),
                args: vec![
                    "list".to_string(),
                    "--local-only".to_string(),
                    pkg.to_string(),
                ],
            },
            Self::Cargo => CommandSpec {
                bin: "cargo".to_string(),
                args: vec!["install".to_string(), "--list".to_string()],
            },
            Self::Npm => CommandSpec {
                bin: if OS_INFO.platform == Platform::Windows {
                    "npm.cmd".to_string()
                } else {
                    "npm".to_string()
                },
                args: vec![
                    "list".to_string(),
                    "--depth=0".to_string(),
                    "-g".to_string(),
                    pkg.to_string(),
                ],
            },
        };

        let output = Command::new(&cmd.bin)
            .args(&cmd.args)
            .output()
            .context(format!("Failed to run {} {}", &cmd.bin, cmd.args.join(" ")))?;

        let out = String::from_utf8_lossy(&output.stdout);

        match self {
            PackageManager::Dnf
            | PackageManager::Zypper
            | PackageManager::Flatpak
            | PackageManager::Npm => Ok(output.status.success()),

            PackageManager::Apt => {
                Ok(output.status.success() && out.lines().any(|line| line.starts_with("ii")))
            }

            PackageManager::Scoop => {
                //first line is "installed apps matching <pkg_name>:" + scoop uses SUBSTRING MATCH. package name is in first column
                Ok(out
                    .lines()
                    .skip(1)
                    .filter_map(|line| line.split_whitespace().next())
                    .any(|name| name == pkg))
            }

            PackageManager::Pacman | PackageManager::Yay | PackageManager::Paru => {
                Ok(out.lines().any(|line| line == pkg))
            }

            PackageManager::Winget | PackageManager::Choco | PackageManager::Brew => {
                Ok(out.contains(pkg))
            }

            PackageManager::Cargo => Ok(out
                .lines()
                .any(|line| line.starts_with(pkg) && line.contains(" v"))),
        }
    }
}
