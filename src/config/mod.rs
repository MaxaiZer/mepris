use std::collections::HashSet;

use pest::{Parser, iterators::Pair};
use pest_derive::Parser;
use serde::{Deserialize, Deserializer};
use strum_macros::EnumIter;

use crate::os_info::OsInfo;

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
pub struct Defaults {
    pub windows_package_manager: Option<PackageManager>,
}

#[derive(Debug, Clone)]
pub struct CommandSpec {
    pub bin: String,
    pub args: Vec<String>,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PackageManager {
    Apt,
    Dnf,
    Pacman,
    Zypper,
    Yay,
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
            Self::Pacman => vec![build("sudo", &["pacman", "-S", "--noconfirm"], pkgs)],
            Self::Zypper => vec![build("sudo", &["zypper", "install", "-y"], pkgs)],
            Self::Yay => vec![build("yay", &["-S", "--noconfirm"], pkgs)],
            Self::Brew => vec![build("brew", &["install"], pkgs)],
            Self::Scoop => vec![build("scoop", &["install"], pkgs)],
            Self::Choco => vec![build("choco", &["install", "-y"], pkgs)],
        }
    }
}

#[derive(Parser)]
#[grammar = "config/os_expr.pest"]
pub struct OsExprParser;

#[derive(Debug, Clone)]
pub enum OsExpr {
    Var(String),
    Not(Box<OsExpr>),
    And(Box<OsExpr>, Box<OsExpr>),
    Or(Box<OsExpr>, Box<OsExpr>),
}

impl OsExpr {}

fn parse_term(term: &str) -> OsCond {
    let norm = term.to_ascii_lowercase();
    if let Some(rest) = norm.strip_prefix('%') {
        OsCond::IdLike(rest.to_string())
    } else {
        OsCond::Os(norm.to_string())
    }
}

pub fn parse_os_expr<'de, D>(deserializer: D) -> Result<Option<OsExpr>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: Option<&str> = Option::deserialize(deserializer)?;
    match s {
        Some(inner) => {
            let parsed = parse(inner).map_err(serde::de::Error::custom)?;
            Ok(Some(parsed))
        }
        None => Ok(None),
    }
}

pub fn parse(input: &str) -> Result<OsExpr, String> {
    let mut pairs =
        OsExprParser::parse(Rule::expr, input).map_err(|e| format!("Parse error: {e}"))?;

    build_expr(pairs.next().unwrap())
}

pub fn eval_expr(expr: &OsExpr, os_info: &OsInfo) -> bool {
    match expr {
        OsExpr::Var(s) => match parse_term(s) {
            OsCond::Os(id) => id == os_info.platform.as_str() || Some(id) == os_info.id,
            OsCond::IdLike(id) => os_info.id_like.contains(&id),
        },
        OsExpr::Not(e) => !eval_expr(e, os_info),
        OsExpr::And(a, b) => eval_expr(a, os_info) && eval_expr(b, os_info),
        OsExpr::Or(a, b) => eval_expr(a, os_info) || eval_expr(b, os_info),
    }
}

fn build_expr(pair: Pair<Rule>) -> Result<OsExpr, String> {
    match pair.as_rule() {
        Rule::expr => build_expr(pair.into_inner().next().unwrap()),

        Rule::or_expr => {
            let mut inner = pair.into_inner();
            let first = build_expr(inner.next().unwrap())?;
            inner.try_fold(first, |left, right_pair| {
                let right = build_expr(right_pair)?;
                Ok(OsExpr::Or(Box::new(left), Box::new(right)))
            })
        }

        Rule::and_expr => {
            let mut inner = pair.into_inner();
            let first = build_expr(inner.next().unwrap())?;
            inner.try_fold(first, |left, right_pair| {
                let right = build_expr(right_pair)?;
                Ok(OsExpr::And(Box::new(left), Box::new(right)))
            })
        }

        Rule::not_expr => {
            let inner = pair.into_inner();
            let mut not_count = 0;
            let mut last = None;

            for p in inner {
                if p.as_rule() == Rule::atom {
                    last = Some(build_expr(p)?);
                    break;
                }
                not_count += 1;
            }

            let mut expr = last.ok_or("Missing atom after !")?;
            for _ in 0..not_count {
                expr = OsExpr::Not(Box::new(expr));
            }

            Ok(expr)
        }

        Rule::atom => build_expr(pair.into_inner().next().unwrap()),

        Rule::ident | Rule::idlike | Rule::os => Ok(OsExpr::Var(pair.as_str().to_string())),

        _ => Err(format!("Unexpected rule: {:?}", pair.as_rule())),
    }
}

#[derive(Debug)]
pub enum OsCond {
    Os(String),
    IdLike(String),
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
    #[serde(default, deserialize_with = "parse_os_expr")]
    pub os: Option<OsExpr>,
    #[serde(rename = "when")]
    pub when_script: Option<Script>,
    pub package_manager: Option<PackageManager>,
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

#[test]
fn test_os_expr() {
    let inputs = vec![
        ("ubuntu", true),
        ("Ubuntu", true),
        ("debian", false),
        ("%debian", true),
        ("!%debian", false),
        ("!windows", true),
        ("windows", false),
        ("ubuntu || debian", true),
        ("!ubuntu || debian", false),
        ("!(arch || fedora)", true),
        ("linux && !arch && !fedora", true),
        ("linux && !arch && !fedora && !ubuntu", false),
    ];
    let os_info = OsInfo {
        platform: crate::os_info::Platform::Linux,
        id: Some("ubuntu".to_string()),
        id_like: vec!["debian".to_string()],
    };

    for (str, expected) in &inputs {
        let parsed = parse(str).unwrap();
        assert_eq!(eval_expr(&parsed, &os_info), expected.clone());
    }
}
