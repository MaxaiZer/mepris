use crate::system::pkg::{PackageManager, PackageSource, Repository};
use crate::system::shell::Shell;
use expr::Expr;
use serde::{
    Deserialize, Deserializer,
    de::{self, IntoDeserializer, value::StringDeserializer},
};
use strum::IntoEnumIterator;
use strum_macros::Display;

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
#[derive(Debug, Clone)]
pub struct Script {
    pub shell: Option<Shell>,
    pub code: String,
}

impl<'de> Deserialize<'de> for Script {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
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

        let value = ScriptDef::deserialize(deserializer)?;
        Ok(match value {
            ScriptDef::Short(code) => Script { shell: None, code },
            ScriptDef::Full { shell, code } => Script {
                shell: Some(shell),
                code,
            },
        })
    }
}

#[derive(Debug, Clone)]
pub struct Require {
    pub id: String,
    pub os: Option<Expr>,
    pub when_script: Option<Script>,
}

impl<'de> Deserialize<'de> for Require {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum RequireDef {
            Short(String),
            Full {
                id: String,
                #[serde(default, deserialize_with = "expr::parse_os_expr")]
                os: Option<Expr>,
                #[serde(rename = "when")]
                when_script: Option<Script>,
            },
        }

        let value = RequireDef::deserialize(deserializer).map_err(|err| {
            de::Error::custom(format!(
                "Invalid require format: {}\n\
                 Expected either a short string (id) or a full object with fields 'id', optional 'os' and optional 'when'.",
                err
            ))
        })?;
        Ok(match value {
            RequireDef::Short(id) => Require {
                id,
                os: None,
                when_script: None,
            },
            RequireDef::Full {
                id,
                os,
                when_script,
            } => Require {
                id,
                os,
                when_script,
            },
        })
    }
}

#[derive(Clone, Debug, PartialEq, Display, Default)]
pub enum StepSelectionReason {
    #[default]
    MatchedFilter,
    Dependency,
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
    #[serde(rename = "check")]
    pub check_script: Option<Script>,
    #[serde(default)]
    pub requires: Vec<Require>,
    #[serde(default)]
    pub provides: Vec<String>,

    #[serde(skip_deserializing)]
    pub source_file: String,
    #[serde(skip_deserializing)]
    pub defaults: Option<Defaults>,
    #[serde(skip_deserializing)]
    pub selection_reason: Option<StepSelectionReason>,
    #[serde(skip_deserializing)]
    pub dependencies: Vec<String>,
    #[serde(skip_deserializing)]
    pub dependency_of: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub includes: Option<Vec<String>>,
    pub defaults: Option<Defaults>,
    pub steps: Option<Vec<Step>>,
}
