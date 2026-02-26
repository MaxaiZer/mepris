use crate::runner::script_checker::ScriptChecker;
use crate::config::alias::PackageAliases;
use crate::runner::{run_script, Step};
use crate::system::shell::is_shell_available;
use std::path::Path;
use std::{fmt, io};
use which::which;

#[derive(Default)]
pub struct StepRun {
    pub id: String,
    pub missing_shells: Vec<String>,
    pub package_manager: Option<PackageManagerInfo>,
    pub packages_to_install: Vec<PackageInfo>,
}

pub struct PackageInfo {
    pub name: String,
    pub use_alias: bool,
    pub installed: bool,
}

impl fmt::Display for PackageInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.use_alias {
            write!(f, "{} (using alias)", self.name)
        } else {
            write!(f, "{}", self.name)
        }
    }
}

pub struct PackageManagerInfo {
    pub name: String,
    pub installed: bool,
}

pub struct RunPlan {
    pub steps_to_run: Vec<StepRun>,
    pub steps_skipped_by_when: Vec<String>,
}

pub fn run(
    steps: &[Step],
    aliases: &PackageAliases,
    script_checker: &mut dyn ScriptChecker,
) -> anyhow::Result<RunPlan> {
    let mut res = RunPlan {
        steps_to_run: vec![],
        steps_skipped_by_when: vec![],
    };

    for step in steps {
        if let Some(when_script) = &step.when_script {
            match run_script(
                when_script,
                Path::new(&step.source_file).parent().unwrap(),
                script_checker,
                &mut io::sink(),
            ) {
                Ok(()) => (),
                Err(_) => {
                    res.steps_skipped_by_when.push(step.id.clone());
                    continue;
                }
            }
        }

        let mut step_dry_run = StepRun {
            id: step.id.clone(),
            ..Default::default()
        };

        if !step.packages.is_empty() {
            let package_manager = step.package_manager.clone();

            step_dry_run.package_manager = Some(PackageManagerInfo {
                name: package_manager.command().to_string(),
                installed: which(package_manager.command()).is_ok(),
            });

            step_dry_run.packages_to_install = step
                .packages
                .iter()
                .map(|p| {
                    let new_name = aliases.resolve_name(p, &package_manager);
                    PackageInfo {
                        name: new_name.clone(),
                        use_alias: *p != new_name,
                        installed: package_manager.is_installed(&new_name).unwrap_or(false)
                    }
                })
                .collect();
        }

        let not_available_shells = step
            .all_used_shells()
            .into_iter()
            .filter(|s| !is_shell_available(s))
            .map(|s| s.get_command())
            .collect::<Vec<&str>>();
        if !not_available_shells.is_empty() {
            step_dry_run.missing_shells =
                not_available_shells.iter().map(|s| s.to_string()).collect();
        }

        res.steps_to_run.push(step_dry_run);
    }

    Ok(res)
}