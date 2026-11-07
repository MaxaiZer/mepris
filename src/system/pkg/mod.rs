use crate::logging::EventType;
use crate::system::os_info::{OS_INFO, Platform};
use crate::system::pkg::parsers::parse_packages_list_func;
use anyhow::{Context, bail};
use serde::Deserialize;
use std::cell::RefCell;
use std::cmp::PartialEq;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::process::{Command, Output, Stdio};
use strum_macros::{Display, EnumIter, EnumString};
use tempfile::NamedTempFile;
use tracing::{debug, debug_span};
use which::which;

mod parsers;

thread_local! {
    static PKG_CACHE: RefCell<HashMap<String, HashSet<String>>> = RefCell::new(HashMap::new());
}

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

#[derive(
    Debug, Deserialize, Clone, PartialEq, Eq, EnumIter, Display, Hash, EnumString, Default,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum PackageManager {
    #[default]
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
    Uv,
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
            Self::Uv => which("uv").is_ok(),
        }
    }

    fn requires_cache(&self) -> bool {
        parse_packages_list_func(self).is_ok()
    }

    fn normalized_package_name(&self, pkg: &str) -> String {
        const UV_SPEC_STARTERS: [char; 7] = ['[', '@', '=', '~', '>', '<', '!'];
        const RPM_SPEC_STARTERS: [char; 3] = ['=', '<', '>'];

        match self {
            Self::Uv => strip_version_spec(pkg, &UV_SPEC_STARTERS),
            Self::Npm => strip_npm_version(pkg),
            Self::Cargo | Self::Scoop => strip_at_version(pkg),
            Self::Apt => strip_version_spec(pkg, &['=']),
            Self::Pacman | Self::Yay | Self::Paru | Self::Zypper => {
                strip_version_spec(pkg, &RPM_SPEC_STARTERS)
            }
            _ => pkg.to_string(),
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
                        "--silent",
                        "--accept-source-agreements",
                        "--accept-package-agreements",
                    ]
                    .into_iter()
                    .map(String::from)
                    .collect(),
                })
                .collect(),

            Self::Uv => pkgs
                .iter()
                .map(|pkg| CommandSpec {
                    bin: "uv".into(),
                    args: vec!["tool", "install", pkg]
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

        if self.requires_cache() {
            let cache_id = self.to_string();
            PKG_CACHE.with(|cache| {
                let mut cache = cache.borrow_mut();
                if let Some(entry) = cache.get_mut(&cache_id) {
                    pkgs.iter().for_each(|pkg| {
                        entry.insert(self.normalized_package_name(pkg));
                    });
                }
            });
        }

        Ok(())
    }

    pub fn is_installed(&self, pkg: &str) -> anyhow::Result<bool> {
        if let Ok(res) = std::env::var("MEPRIS_IS_INSTALLED_RESULT") {
            return Ok(res == "0");
        }

        let normalized = self.normalized_package_name(pkg);

        let cmd = match self {
            Self::Pacman | Self::Yay | Self::Paru => CommandSpec {
                bin: "pacman".to_string(),
                args: vec!["-Qq".to_string()],
            },
            Self::Apt => CommandSpec {
                bin: "dpkg".to_string(),
                args: vec!["-l".to_string(), normalized.clone()],
            },
            Self::Dnf | Self::Zypper => CommandSpec {
                bin: "rpm".to_string(),
                args: vec!["-q".to_string(), normalized.clone()],
            },
            Self::Flatpak => CommandSpec {
                bin: "flatpak".to_string(),
                args: vec!["info".to_string(), normalized.clone()],
            },
            Self::Brew => CommandSpec {
                bin: "brew".to_string(),
                args: vec![
                    "list".to_string(),
                    "--versions".to_string(),
                    normalized.clone(),
                ],
            },
            Self::Winget => CommandSpec {
                bin: "winget".to_string(),
                args: vec![
                    "export".to_string(),
                    "--source".to_string(),
                    "winget".to_string(),
                    "-o".to_string(),
                ],
            },
            Self::Scoop => CommandSpec {
                bin: "scoop.cmd".to_string(),
                args: vec!["list".to_string()],
            },
            Self::Choco => CommandSpec {
                bin: "choco".to_string(),
                args: vec![
                    "list".to_string(),
                    "--limit-output".to_string(),
                    "--no-color".to_string(),
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
                    "----parseable".to_string(),
                ],
            },
            Self::Uv => CommandSpec {
                bin: "uv".to_string(),
                args: vec!["tool".to_string(), "list".to_string()],
            },
        };

        if self.requires_cache() {
            return run_cacheable_is_installed(self, &cmd, parse_packages_list_func(self)?, pkg);
        }

        let output = run_command(&cmd)?;
        let out = String::from_utf8_lossy(&output.stdout);

        match self {
            PackageManager::Dnf | PackageManager::Zypper | PackageManager::Flatpak => {
                Ok(output.status.success())
            }

            PackageManager::Apt => {
                Ok(output.status.success() && out.lines().any(|line| line.starts_with("ii")))
            }

            PackageManager::Pacman | PackageManager::Yay | PackageManager::Paru => {
                Ok(out.lines().any(|line| line == normalized))
            }

            PackageManager::Brew => Ok(out.contains(&normalized)),

            PackageManager::Cargo => Ok(out.lines().any(|line| {
                line.strip_prefix(&normalized)
                    .is_some_and(|rest| rest.starts_with(" v"))
            })),

            PackageManager::Npm
            | PackageManager::Uv
            | PackageManager::Scoop
            | PackageManager::Choco
            | PackageManager::Winget => {
                bail!("Unreachable code")
            }
        }
    }
}

fn strip_version_spec(pkg: &str, starters: &[char]) -> String {
    let end = pkg.find(|c| starters.contains(&c)).unwrap_or(pkg.len());
    pkg[..end].trim().to_string()
}

fn strip_at_version(pkg: &str) -> String {
    pkg.split_once('@')
        .map_or(pkg, |(name, _)| name)
        .trim()
        .to_string()
}

fn strip_npm_version(pkg: &str) -> String {
    let name_start = if pkg.starts_with('@') {
        pkg.find('/').map_or(1, |slash| slash + 1)
    } else {
        0
    };

    pkg[name_start..]
        .find('@')
        .map_or(pkg, |offset| &pkg[..name_start + offset])
        .trim()
        .to_string()
}

fn run_cacheable_is_installed(
    manager: &PackageManager,
    cmd: &CommandSpec,
    parse: fn(output: String) -> anyhow::Result<HashSet<String>>,
    pkg: &str,
) -> anyhow::Result<bool> {
    let res = PKG_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let cache_id = manager.to_string();

        let packages = cache.entry(cache_id).or_insert_with(|| {
            debug!("Package cache is empty for {}, populating...", manager);
            let _span = debug_span!("cache_population").entered();

            let output = if manager == &PackageManager::Winget {
                run_win_command_with_file_output(cmd).unwrap()
            } else {
                let res = run_command(cmd).unwrap();
                String::from_utf8_lossy(&res.stdout).to_string()
            };

            debug!(event_type=%EventType::CachePopulationCompleted);
            parse(output).unwrap()
        });

        packages.contains(&manager.normalized_package_name(pkg))
    });

    Ok(res)
}

fn run_command(cmd: &CommandSpec) -> anyhow::Result<Output> {
    Command::new(&cmd.bin)
        .args(&cmd.args)
        .output()
        .context(format!("Failed to run {} {}", &cmd.bin, cmd.args.join(" ")))
}

fn run_win_command_with_file_output(cmd: &CommandSpec) -> anyhow::Result<String> {
    let temp = NamedTempFile::new().context("failed to create temp file")?;
    let path = temp.path();

    let cmd_str =
        cmd.bin.clone() + " " + &cmd.args.join(" ") + " " + path.to_string_lossy().as_ref();

    let script = format!(
        r#"
    $tmp = New-TemporaryFile
    {}
    Get-Content $tmp
    Remove-Item $tmp
    "#,
        cmd_str
    );

    let _ = Command::new("powershell")
        .arg("-Command")
        .arg(script)
        .output()
        .context("failed to run powershell")?;

    let file = fs::read_to_string(path).context("failed to read temp file")?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uv_normalized_package_name_strips_specs() {
        assert_eq!(
            PackageManager::Uv.normalized_package_name("hound-mcp[all]"),
            "hound-mcp"
        );
        assert_eq!(
            PackageManager::Uv.normalized_package_name("foo @ https://example.com/foo.whl"),
            "foo"
        );
        assert_eq!(
            PackageManager::Uv.normalized_package_name("bar==1.0"),
            "bar"
        );
        assert_eq!(PackageManager::Uv.normalized_package_name("baz~=2"), "baz");
        assert_eq!(PackageManager::Uv.normalized_package_name("qux!=1"), "qux");
        assert_eq!(PackageManager::Uv.normalized_package_name("ruff"), "ruff");
    }

    #[test]
    fn test_npm_normalized_package_name_strips_version() {
        assert_eq!(
            PackageManager::Npm.normalized_package_name("typescript@5"),
            "typescript"
        );
        assert_eq!(
            PackageManager::Npm.normalized_package_name("typescript@5.4.2"),
            "typescript"
        );
        assert_eq!(
            PackageManager::Npm.normalized_package_name("@angular/cli@18"),
            "@angular/cli"
        );
        assert_eq!(
            PackageManager::Npm.normalized_package_name("@angular/cli"),
            "@angular/cli"
        );
    }

    #[test]
    fn test_cargo_and_scoop_normalized_package_name_strips_version() {
        assert_eq!(
            PackageManager::Cargo.normalized_package_name("ripgrep@14"),
            "ripgrep"
        );
        assert_eq!(
            PackageManager::Scoop.normalized_package_name("7zip@25.01"),
            "7zip"
        );
    }

    #[test]
    fn test_system_package_managers_normalize_version_specs() {
        assert_eq!(
            PackageManager::Apt.normalized_package_name("bash=5.2"),
            "bash"
        );
        assert_eq!(
            PackageManager::Pacman.normalized_package_name("bash>=5.2"),
            "bash"
        );
        assert_eq!(
            PackageManager::Zypper.normalized_package_name("bash<6"),
            "bash"
        );
    }

    #[test]
    fn test_normalized_package_name_identity_for_other_managers() {
        assert_eq!(
            PackageManager::Brew.normalized_package_name("python@3.12"),
            "python@3.12"
        );
        assert_eq!(
            PackageManager::Dnf.normalized_package_name("gcc-c++"),
            "gcc-c++"
        );
        assert_eq!(
            PackageManager::Cargo.normalized_package_name("ripgrep"),
            "ripgrep"
        );
    }
}
