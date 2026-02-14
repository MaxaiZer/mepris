use expr::Expr;
use serde::{
    de::{self, value::StringDeserializer, IntoDeserializer}, Deserialize,
    Deserializer,
};
use strum::IntoEnumIterator;
use strum_macros::{Display, EnumIter, EnumString};
use crate::system::shell::Shell;

pub mod alias;
pub mod expr;
pub mod parser;

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
pub struct Defaults {
    pub windows_package_manager: Option<PackageManager>,
    pub windows_shell: Option<Shell>,
    pub linux_shell: Option<Shell>,
    pub macos_shell: Option<Shell>,
}

impl Defaults {
    pub fn merge(inherited: &Option<Defaults>, overrides: &Option<Defaults>) -> Self {
        
        let inherited = inherited.as_ref();
        let overrides = overrides.as_ref();
        
        Defaults {
            windows_package_manager: overrides
                .and_then(|overrides| overrides.windows_package_manager.clone())
                .or(inherited.and_then(|d| d.windows_package_manager.clone())),
            windows_shell: overrides
                .and_then(|overrides| overrides.windows_shell.clone())
                .or(inherited.and_then(|d| d.windows_shell.clone())),
            linux_shell: overrides
                .and_then(|overrides| overrides.linux_shell.clone())
                .or(inherited.and_then(|d| d.linux_shell.clone())),
            macos_shell: overrides
                .and_then(|overrides| overrides.macos_shell.clone())
                .or(inherited.and_then(|d| d.macos_shell.clone())),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CommandSpec {
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

impl<'de> Deserialize<'de> for PackageSource {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: String = String::deserialize(deserializer)?;
        let s_lower = s.to_lowercase();

        let parse_err = || {
            let mut expected: Vec<String> = PackageManager::iter()
                .filter(|pm| !Repository::Aur.get_package_managers().contains(pm))
                .map(|pm| pm.to_string().to_lowercase())
                .collect();
            expected.extend(Repository::iter().map(|repo| repo.to_string().to_lowercase()));
            de::Error::custom(format!(
                "unknown package_source '{}', expected one of [{}]",
                s_lower,
                expected.join(", ")
            ))
        };

        if let Ok(repo) = Repository::deserialize::<StringDeserializer<D::Error>>(
            s_lower.clone().into_deserializer(),
        ) {
            return Ok(PackageSource::Repository(repo));
        }

        if let Ok(pm) = PackageManager::deserialize::<StringDeserializer<D::Error>>(
            s_lower.clone().into_deserializer(),
        ) {
            if Repository::Aur.get_package_managers().contains(&pm) {
                return Err(parse_err());
            }
            return Ok(PackageSource::Manager(pm));
        }

        Err(parse_err())
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
    pub const fn command(&self) -> &'static str {
        match self {
            Self::Pacman => "pacman",
            Self::Apt => "apt-get",
            Self::Dnf => "dnf",
            Self::Zypper => "zypper",
            Self::Brew => "brew",
            Self::Winget => "winget",
            Self::Yay => "yay",
            Self::Paru => "paru",
            Self::Flatpak => "flatpak",
            Self::Scoop => "scoop",
            Self::Choco => "choco",
            Self::Cargo => "cargo",
            Self::Npm => "npm"
        }
    }
    pub fn commands_to_install(&self, pkgs: &[String]) -> Vec<CommandSpec> {
        fn build(cmd: &str, args: &[&str], pkgs: &[String]) -> CommandSpec {
            CommandSpec {
                bin: cmd.into(),
                args: args
                    .iter()
                    .map(ToString::to_string)
                    .chain(pkgs.iter().cloned())
                    .collect(),
            }
        }

        match self {
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
                    args: vec!["install", "-e", "--id", pkg]
                        .into_iter()
                        .map(String::from)
                        .collect(),
                })
                .collect(),

            Self::Apt => vec![build("sudo", &["apt-get", "install", "-y"], pkgs)],
            Self::Dnf => vec![build("sudo", &["dnf", "install", "-y"], pkgs)],
            Self::Pacman => vec![build(
                "sudo",
                &["pacman", "-S", "--noconfirm", "--needed"],
                pkgs,
            )],
            Self::Yay => vec![build("yay", &["-S", "--noconfirm", "--needed"], pkgs)],
            Self::Paru => vec![build("paru", &["-S", "--noconfirm", "--needed"], pkgs)],
            Self::Zypper => vec![build("sudo", &["zypper", "install", "-y"], pkgs)],
            Self::Brew => vec![build("brew", &["install"], pkgs)],
            Self::Scoop => vec![build("scoop.cmd", &["install"], pkgs)],
            Self::Choco => vec![build("choco", &["install", "-y"], pkgs)],
            Self::Cargo => vec![build("cargo", &["install"], pkgs)],
            Self::Npm => vec![build("npm", &["i", "-g"], pkgs)]
        }
    }
    pub fn command_check_if_installed(&self, pkg: &str) -> CommandSpec {
        match self {
            Self::Pacman | Self::Yay | Self::Paru => CommandSpec {
                bin: "pacman".to_string(),
                args: vec!["-Q".to_string(), pkg.to_string()],
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
                args: vec!["list".to_string(), "--versions".to_string(), pkg.to_string()],
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
                args: vec!["list".to_string(), "--local-only".to_string(), pkg.to_string()],
            },
            Self::Cargo => CommandSpec {
                bin: "cargo".to_string(),
                args: vec!["install".to_string(), "--list".to_string()],
            },
            Self::Npm => CommandSpec {
                bin: "npm".to_string(),
                args: vec!["list".to_string(), "--depth=0".to_string(), "-g".to_string(), pkg.to_string()],
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct Script {
    pub shell: Option<Shell>,
    pub code: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ScriptDef {
    Short(String),
    Full {
        shell: Shell,
        #[serde(rename = "run")]
        code: String,
    },
}

impl<'de> Deserialize<'de> for Script {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let helper = ScriptDef::deserialize(deserializer)?;
        Ok(match helper {
            ScriptDef::Short(code) => Script {
                shell: None,
                code,
            },
            ScriptDef::Full { shell, code } => Script { shell: Some(shell), code },
        })
    }
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct Step {
    pub id: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, deserialize_with = "expr::parse_os_expr")]
    pub os: Option<Expr>,
    #[serde(default)]
    pub env: Vec<String>,
    #[serde(rename = "when")]
    pub when_script: Option<Script>,
    pub package_source: Option<PackageSource>,
    #[serde(default)]
    pub packages: Vec<String>,
    pub pre_script: Option<Script>,
    pub script: Option<Script>,

    #[serde(skip_deserializing)]
    pub source_file: String,
    #[serde(skip_deserializing)]
    pub defaults: Option<Defaults>,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub includes: Option<Vec<String>>,
    pub defaults: Option<Defaults>,
    pub steps: Option<Vec<Step>>,
}
