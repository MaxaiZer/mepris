use std::collections::HashSet;

use expr::Expr;
use serde::{
    Deserialize, Deserializer,
    de::{self, IntoDeserializer, value::StringDeserializer},
};
use strum::IntoEnumIterator;
use strum_macros::{Display, EnumIter};

pub mod expr;

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
pub struct Defaults {
    pub windows_package_manager: Option<PackageManager>,
}

#[derive(Debug, Clone)]
pub struct CommandSpec {
    pub bin: String,
    pub args: Vec<String>,
}

impl<'de> Deserialize<'de> for PackageSource {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: String = String::deserialize(deserializer)?;
        let s_lower = s.to_lowercase();

        if let Ok(repo) = Repository::deserialize::<StringDeserializer<D::Error>>(
            s_lower.clone().into_deserializer(),
        ) {
            return Ok(PackageSource::Repository(repo));
        }

        if let Ok(pm) = PackageManager::deserialize::<StringDeserializer<D::Error>>(
            s_lower.clone().into_deserializer(),
        ) {
            return Ok(PackageSource::Manager(pm));
        }

        let mut expected: Vec<String> = PackageManager::iter()
            .map(|pm| pm.to_string().to_lowercase())
            .collect();
        expected.extend(Repository::iter().map(|repo| repo.to_string().to_lowercase()));

        Err(de::Error::custom(format!(
            "unknown package_source '{}', expected one of [{}]",
            s_lower,
            expected.join(", ")
        )))
    }
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

#[derive(Debug)]
pub enum PackageSource {
    Repository(Repository),
    Manager(PackageManager),
}

#[derive(Debug, Deserialize, EnumIter, Display)]
#[serde(rename_all = "lowercase")]
pub enum Repository {
    Aur,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq, EnumIter, Display)]
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
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Hash, EnumIter)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum Shell {
    #[default]
    Bash,
    #[serde(rename = "pwsh")]
    PowerShellCore,
}

impl Shell {
    pub fn get_command(&self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::PowerShellCore => "pwsh",
        }
    }
}

#[derive(Debug)]
pub struct Script {
    pub shell: Shell,
    pub code: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ScriptDef {
    Short(String),
    Full {
        #[serde(default)]
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
                shell: Shell::Bash,
                code,
            },
            ScriptDef::Full { shell, code } => Script { shell, code },
        })
    }
}

#[derive(Debug, Deserialize, Default)]
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

impl Step {
    pub fn all_used_shells(&self) -> HashSet<Shell> {
        [
            self.when_script.as_ref(),
            self.pre_script.as_ref(),
            self.script.as_ref(),
        ]
        .iter()
        .filter_map(|script_opt| script_opt.map(|s| s.shell.clone()))
        .collect()
    }
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub includes: Option<Vec<String>>,
    pub defaults: Option<Defaults>,
    pub steps: Option<Vec<Step>>,
}
