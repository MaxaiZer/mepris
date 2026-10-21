use crate::config::expr;
use crate::config::expr::Expr;
use crate::system::pkg::{PackageManager, PackageSource, Repository};
use crate::system::shell::Shell;
use serde::de::IntoDeserializer;
use serde::de::value::StringDeserializer;
use serde::{Deserialize, Deserializer, de};
use std::collections::BTreeMap;
use strum::IntoEnumIterator;
use strum_macros::Display;

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FullScript {
    shell: Shell,
    #[serde(rename = "run")]
    code: String,
}

impl<'de> Deserialize<'de> for Script {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        /*        let value = ScriptDef::deserialize(deserializer)?;
        Ok(match value {
            ScriptDef::Short(code) => Script { shell: None, code },
            ScriptDef::Full( FullScript { shell, code }) => Script {
                shell: Some(shell),
                code,
            },
        })*/

        let value = serde_yaml::Value::deserialize(deserializer)?;

        match value {
            serde_yaml::Value::String(code) => Ok(Script { shell: None, code }),

            serde_yaml::Value::Mapping(_) => {
                let full: FullScript = FullScript::deserialize(value).map_err(de::Error::custom)?;

                Ok(Script {
                    shell: Some(full.shell),
                    code: full.code,
                })
            }

            _ => Err(de::Error::custom(
                "script must be either a string or an object with fields 'shell' and 'run'",
            )),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Require {
    pub id: String,
    pub os: Option<Expr>,
    pub when_script: Option<Script>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FullRequire {
    pub id: String,
    #[serde(default, deserialize_with = "expr::os::parse_os_expr")]
    pub os: Option<Expr>,
    #[serde(rename = "when", default)]
    pub when_script: Option<Script>,
}

impl<'de> Deserialize<'de> for Require {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_yaml::Value::deserialize(deserializer)?;
        match value {
            serde_yaml::Value::String(id) => Ok(Require {
                id,
                os: None,
                when_script: None,
            }),

            serde_yaml::Value::Mapping(_) => {
                let full: FullRequire =
                    FullRequire::deserialize(value).map_err(de::Error::custom)?;

                Ok(Require {
                    id: full.id,
                    os: full.os,
                    when_script: full.when_script,
                })
            }

            _ => Err(de::Error::custom(
                "require must be either a string (id) or an object with fields 'id', optional 'os' and optional 'when'",
            )),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Display, Default)]
pub enum StepSelectionReason {
    #[default]
    MatchedFilter,
    Dependency,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub id: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, deserialize_with = "expr::os::parse_os_expr")]
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

    pub extends: Option<String>,
    pub with: Option<BTreeMap<String, String>>,

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
#[serde(deny_unknown_fields)]
pub struct Config {
    pub includes: Option<Vec<String>>,
    pub defaults: Option<Defaults>,
    pub steps: Option<Vec<Step>>,
}
