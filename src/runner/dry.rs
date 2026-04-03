use crate::config::StepSelectionReason;
use crate::runner::{Step, StepCompletedResult};
use crate::system::shell::is_shell_available;
use std::collections::HashSet;
use std::fmt;

#[derive(Debug, Default)]
pub struct StepRun {
    pub id: String,
    pub source_file: String,
    pub step_completed_result: StepCompletedResult,
    pub missing_shells: Vec<String>,
    pub package_manager: Option<PackageManagerInfo>,
    pub packages_to_install: Vec<PackageInfo>,
    pub selection_reason: StepSelectionReason,
    pub dependencies: Vec<String>,
    pub dependency_of: Vec<String>,
}

#[derive(Debug)]
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

#[derive(Debug)]
pub struct PackageManagerInfo {
    pub name: String,
    pub installed: bool,
}

#[derive(Debug)]
pub struct RunPlan {
    pub steps_to_run: Vec<StepRun>,
}

pub fn run(steps: &[Step]) -> anyhow::Result<RunPlan> {
    let mut res = RunPlan {
        steps_to_run: vec![],
    };

    for step in steps {
        let step_completed_res = step.is_completed(None)?;
        let mut step_dry_run = StepRun {
            id: step.id.clone(),
            source_file: step.source_file.clone(),
            step_completed_result: step_completed_res.clone(),
            selection_reason: step.selection_reason.clone(),
            dependencies: step.dependencies.clone(),
            dependency_of: step.dependency_of.clone(),
            ..Default::default()
        };

        if !step.packages.is_empty() {
            let package_manager = step.package_manager.clone();

            step_dry_run.package_manager = Some(PackageManagerInfo {
                name: package_manager.to_string(),
                installed: package_manager.is_available(),
            });

            let not_installed_pkgs: HashSet<String> = match &step_completed_res {
                StepCompletedResult::NotInstalledPackages(pkgs) => {
                    HashSet::from_iter(pkgs.iter().cloned())
                }
                _ => HashSet::new(),
            };

            step_dry_run.packages_to_install = step
                .packages
                .iter()
                .map(|p| PackageInfo {
                    name: p.name.clone(),
                    use_alias: p.used_alias,
                    installed: step_completed_res
                        != StepCompletedResult::NotInstalledPackageManager
                        && !not_installed_pkgs.contains(&p.name),
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
