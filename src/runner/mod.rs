use std::cmp::PartialEq;
use std::collections::{HashMap, HashSet};
use std::{
    io::Write,
    path::{Path, PathBuf},
};

pub mod dry;
pub mod interactive;
pub mod logger;
mod pkg;
pub mod script;
pub mod script_checker;
pub mod state;

pub use interactive::{CliInteractor, Decision, Interactor};

use crate::{config, config::aliases::load_aliases};

use crate::config::StepSelectionReason;
use crate::config::aliases::PackageAliases;
use crate::runner::pkg::{install_packages, resolve_step_package_manager};
pub(crate) use crate::runner::script::{
    Script, ScriptResult, run_noninteractive_script, run_script,
};
use crate::system::pkg::PackageManager;
use crate::system::shell::Shell;
use anyhow::{Context, Result, bail};
use colored::Colorize;
use logger::Logger;
use script_checker::ScriptChecker;

pub struct RunParameters {
    pub source_file_path: PathBuf,
    pub dry_run: bool,
}

pub struct RunState {
    pub last_step_id: Option<String>,
    pub interactive: bool,
}

pub trait StateSaver {
    fn save(&self, info: &RunState) -> Result<()>;
}

pub struct Package {
    pub name: String,
    pub used_alias: bool,
}

#[derive(Debug, PartialEq, Eq, Default, Clone)]
pub enum StepCompletedResult {
    #[default]
    Completed,
    NotInstalledPackageManager,
    NotInstalledPackages(Vec<String>),
    FailedCheckScript,
    HasScriptWithoutCheck,
}

#[derive(PartialEq, Debug)]
enum ExecutionResult {
    Completed,
    Skipped,
    CompletedWithMissingDeps,
}

#[derive(Default)]
pub struct Step {
    pub id: String,
    pub package_manager: PackageManager,
    pub packages: Vec<Package>,
    pub pre_script: Option<Script>,
    pub script: Option<Script>,
    pub check_script: Option<Script>,
    pub source_file: String,
    pub selection_reason: StepSelectionReason,
    pub dependencies: Vec<String>,
    pub dependency_of: Vec<String>,
}

impl Step {
    fn from(config_step: &config::Step, aliases: &PackageAliases) -> Self {
        let resolve_script = |script: &Option<config::Script>| -> Option<Script> {
            if script.is_none() {
                return None;
            }
            Some(Script::from(
                script.as_ref().unwrap(),
                &config_step.defaults,
            ))
        };

        let pkg_manager = resolve_step_package_manager(config_step);

        let mut packages: Vec<Package> = Vec::new();
        for cfg_pkg in &config_step.packages {
            let mut resolved_pkg = Package {
                name: aliases.resolve_name(cfg_pkg, &pkg_manager),
                used_alias: false,
            };
            resolved_pkg.used_alias = cfg_pkg != &resolved_pkg.name;
            packages.push(resolved_pkg);
        }

        Step {
            id: config_step.id.clone(),
            package_manager: pkg_manager,
            packages,
            pre_script: resolve_script(&config_step.pre_script),
            script: resolve_script(&config_step.script),
            check_script: resolve_script(&config_step.check_script),
            source_file: config_step.source_file.clone(),
            selection_reason: config_step
                .selection_reason
                .clone()
                .context(format!(
                    "selection_reason must be set for config step '{}'",
                    config_step.id
                ))
                .unwrap(),
            dependencies: config_step.dependencies.clone(),
            dependency_of: config_step.dependency_of.clone(),
        }
    }

    pub fn all_used_shells(&self) -> HashSet<Shell> {
        [self.pre_script.as_ref(), self.script.as_ref()]
            .iter()
            .filter_map(|script_opt| script_opt.map(|s| s.shell.clone()))
            .collect()
    }

    pub fn directory(&self) -> &Path {
        Path::new(&self.source_file).parent().unwrap()
    }

    pub fn is_completed(
        &self,
        script_checker: Option<&mut dyn ScriptChecker>,
    ) -> Result<StepCompletedResult> {
        if std::env::var("MEPRIS_INSTALL_COMMAND").is_err() && !self.package_manager.is_available()
        {
            return Ok(StepCompletedResult::NotInstalledPackageManager);
        }

        let mut not_installed_pkgs = Vec::new();
        for pkg in self.packages.iter() {
            if !self.package_manager.is_installed(&pkg.name)? {
                not_installed_pkgs.push(pkg.name.clone());
            }
        }

        if !not_installed_pkgs.is_empty() {
            return Ok(StepCompletedResult::NotInstalledPackages(
                not_installed_pkgs,
            ));
        }

        if let Some(check_script) = self.check_script.as_ref() {
            let res = run_noninteractive_script(check_script, self.directory(), script_checker)
                .context(format!("failed to run check-script for step '{}'", self.id))?;
            match res {
                ScriptResult::NotZeroExitStatus(_) => {
                    return Ok(StepCompletedResult::FailedCheckScript);
                }
                ScriptResult::Success => {}
            }
        } else if self.script.is_some() {
            return Ok(StepCompletedResult::HasScriptWithoutCheck);
        }

        Ok(StepCompletedResult::Completed)
    }
}

pub fn run(
    steps: &[config::Step],
    params: &RunParameters,
    state_saver: &dyn StateSaver,
    script_checker: &mut dyn ScriptChecker,
    mut interactor: Option<&mut dyn Interactor>,
    out: &mut impl Write,
) -> Result<Option<dry::RunPlan>> {
    let aliases = load_aliases(params.source_file_path.parent().unwrap())?;
    let mut steps: Vec<Step> = steps.iter().map(|s| Step::from(s, &aliases)).collect();
    check_scripts_before_run(&steps, script_checker)?;

    if params.dry_run {
        return dry::run(&steps).map(Some);
    }

    let mut logger = Logger::new(steps.len(), out);
    let mut interactive = interactor.is_some();
    let mut execution_results: HashMap<String, ExecutionResult> = HashMap::new();

    for (i, step) in steps.iter_mut().enumerate() {
        logger.current_step = i + 1;

        let has_broken_deps = step.dependencies.iter().any(|dep| {
            execution_results
                .get(dep)
                .is_some_and(|res| res != &ExecutionResult::Completed)
        });

        if state_saver
            .save(&RunState {
                last_step_id: Some(step.id.clone()),
                interactive,
            })
            .is_err()
        {
            logger.log(&format!("{} Failed to save run state", "Warning:".yellow()))?;
        }

        let completion = step.is_completed(Some(script_checker))?;
        if interactive && let Some(interactor) = interactor.as_mut() {
            match interactor.ask_confirmation(step, has_broken_deps, &completion, &mut logger)? {
                Decision::Run => {}
                Decision::Skip => {
                    if completion == StepCompletedResult::Completed {
                        execution_results.insert(step.id.clone(), ExecutionResult::Completed);
                    } else {
                        execution_results.insert(step.id.clone(), ExecutionResult::Skipped);
                    }
                    continue;
                }
                Decision::Abort => return Ok(None),
                Decision::LeaveInteractiveMode => interactive = false,
            }
        } else if completion == StepCompletedResult::Completed {
            logger.log_with_progress(|p| {
                format!("✅ {p} Step '{}' already completed, skipping", step.id)
            })?;
            continue;
        }

        if !interactive && has_broken_deps {
            bail!("cannot run step with broken dependencies without interactive mode")
        }

        logger.log_with_progress(|p| format!("🚀 {p} Running step '{}'...", step.id))?;
        run_step(step, script_checker, &mut logger)?;
        logger.log_with_progress(|p| format!("✅ {p} Step '{}' completed", step.id))?;

        if has_broken_deps {
            execution_results.insert(step.id.clone(), ExecutionResult::CompletedWithMissingDeps);
        } else {
            execution_results.insert(step.id.clone(), ExecutionResult::Completed);
        }
    }

    if state_saver
        .save(&RunState {
            last_step_id: None,
            interactive,
        })
        .is_err()
    {
        logger.log(&format!("{} Failed to save run state", "Warning:".yellow()))?;
    }

    logger.log("✅ Run completed")?;

    Ok(None)
}

fn check_scripts_before_run(steps: &[Step], script_checker: &mut dyn ScriptChecker) -> Result<()> {
    let skip_if_shell_unavailable = true;
    let check_step_script = |step: &Step,
                             script_name: &str,
                             script: &Option<Script>,
                             script_checker: &mut dyn ScriptChecker|
     -> Result<()> {
        if let Some(script) = script {
            script_checker
                .check_script(script, skip_if_shell_unavailable)
                .context(format!(
                    "Failed to check {script_name} in {}, step '{}'",
                    step.source_file, step.id
                ))?;
        }
        Ok(())
    };

    for step in steps.iter() {
        check_step_script(step, "pre-script", &step.pre_script, script_checker)?;
        check_step_script(step, "script", &step.script, script_checker)?;
        check_step_script(step, "check-script", &step.check_script, script_checker)?;
    }
    Ok(())
}

fn run_step(
    step: &Step,
    script_checker: &mut dyn ScriptChecker,
    logger: &mut Logger,
) -> Result<()> {
    let step_dir = step.directory();

    let mut run_step_script =
        |name: &str, script: &Option<Script>, logger: &mut Logger| -> Result<()> {
            if let Some(script) = script {
                logger.log_with_progress(|p| format!("⚙️ {p} Running {name}..."))?;
                match run_script(script, step_dir, Some(script_checker), logger.out) {
                    Ok(ScriptResult::Success) => return Ok(()),
                    Ok(ScriptResult::NotZeroExitStatus(code)) => {
                        bail!("failed to run {name}: status code {code}")
                    }
                    Err(e) => bail!("failed to run {name}: {e}"),
                }
            }
            Ok(())
        };

    run_step_script("pre-script", &step.pre_script, logger)?;

    if !step.packages.is_empty() {
        install_packages(
            &step
                .packages
                .iter()
                .map(|p| p.name.clone())
                .collect::<Vec<String>>(),
            &step.package_manager,
            logger,
        )?;
    }

    run_step_script("script", &step.script, logger)?;
    run_step_script("check-script", &step.check_script, logger)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::system::shell::mock_available_shells;

    use super::*;
    use crate::EnvGuard;
    use crate::config::StepSelectionReason::MatchedFilter;
    use crate::runner::script_checker::DefaultScriptChecker;
    use crate::system::pkg::PackageSource;
    use rstest::rstest;
    use serial_test::serial;
    use std::{collections::HashSet, env, fs, io};
    use tempfile::tempdir;

    struct FakeStateSaver;
    impl StateSaver for FakeStateSaver {
        fn save(&self, _: &RunState) -> Result<()> {
            Ok(())
        }
    }

    struct MockScriptChecker {
        pub check_value: Result<(), String>,
        pub check_value_calls: u32,
        pub is_checked_value: bool,
    }
    impl ScriptChecker for MockScriptChecker {
        fn check_script(&mut self, _: &Script, _: bool) -> Result<()> {
            self.check_value_calls += 1;
            self.check_value.clone().map_err(anyhow::Error::msg)
        }
        fn is_checked(&self, _: &Script) -> bool {
            self.is_checked_value
        }
    }

    #[test]
    #[cfg(unix)]
    fn test_run_script_from_file_dir() -> Result<()> {
        use std::{collections::HashSet, io};

        use anyhow::Ok;

        mock_available_shells(HashSet::from_iter([Shell::Bash]));
        let dir = tempdir().expect("Failed to create temp dir");
        let step_path = dir.path().join("file.yaml").to_str().unwrap().to_string();

        let steps = vec![config::Step {
            id: "parent".to_string(),
            script: Some(config::Script {
                shell: Some(Shell::Bash),
                code: "cat file.txt".to_string(),
            }),
            source_file: step_path.clone(),
            selection_reason: Some(MatchedFilter),
            ..Default::default()
        }];

        fs::write(dir.path().join("file.txt").to_str().unwrap(), "temp file")
            .expect("Failed to write temp file");

        let _ = run(
            &steps,
            &RunParameters {
                dry_run: false,
                source_file_path: Path::new("/file.yaml").to_path_buf(),
            },
            &FakeStateSaver,
            &mut DefaultScriptChecker::new(),
            None,
            &mut io::sink(),
        )?;
        Ok(())
    }

    #[test]
    fn test_run_dry_warns_about_unavailable_shell() -> Result<()> {
        let mut output = Vec::new();
        mock_available_shells(HashSet::from_iter([Shell::Bash]));

        let steps = vec![config::Step {
            id: "step".to_string(),
            script: Some(config::Script {
                shell: Some(Shell::PowerShellCore),
                code: "cat file.txt".to_string(),
            }),
            selection_reason: Some(MatchedFilter),
            ..Default::default()
        }];

        let plan = run(
            &steps,
            &RunParameters {
                dry_run: true,
                source_file_path: Path::new("/file.yaml").to_path_buf(),
            },
            &FakeStateSaver,
            &mut DefaultScriptChecker::new(),
            None,
            &mut output,
        )?
        .unwrap();

        assert_eq!(plan.steps_to_run.len(), 1);
        assert!(!plan.steps_to_run[0].missing_shells.is_empty());
        assert!(
            plan.steps_to_run[0].missing_shells.contains(
                &steps[0]
                    .script
                    .as_ref()
                    .unwrap()
                    .shell
                    .as_ref()
                    .unwrap()
                    .get_command()
                    .to_string()
            )
        );
        Ok(())
    }

    #[test]
    #[serial]
    fn test_run_dry_warns_about_unavailable_package_manager() -> Result<()> {
        let mut output = Vec::new();
        mock_available_shells(HashSet::from_iter([Shell::Bash]));

        let steps = vec![config::Step {
            id: "step".to_string(),
            packages: vec!["git".to_string()],
            package_source: Some(PackageSource::Manager(PackageManager::Choco)),
            selection_reason: Some(MatchedFilter),
            ..Default::default()
        }];

        let plan = run(
            &steps,
            &RunParameters {
                dry_run: true,
                source_file_path: Path::new("/file.yaml").to_path_buf(),
            },
            &FakeStateSaver,
            &mut DefaultScriptChecker::new(),
            None,
            &mut output,
        )?
        .unwrap();

        assert_eq!(plan.steps_to_run.len(), 1);
        assert!(
            !plan.steps_to_run[0]
                .package_manager
                .as_ref()
                .unwrap()
                .installed
        );
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn test_run_failed_check_after_step() -> Result<()> {
        let mut output = Vec::new();

        let steps = vec![config::Step {
            id: "step".to_string(),
            script: Some(config::Script {
                shell: Some(Shell::Bash),
                code: "exit 0".to_string(),
            }),
            check_script: Some(config::Script {
                shell: Some(Shell::Bash),
                code: "exit 1".to_string(),
            }),
            source_file: "/file.yaml".to_string(),
            selection_reason: Some(MatchedFilter),
            ..Default::default()
        }];

        let res = run(
            &steps,
            &RunParameters {
                dry_run: false,
                source_file_path: Path::new("/file.yaml").to_path_buf(),
            },
            &FakeStateSaver,
            &mut DefaultScriptChecker::new(),
            None,
            &mut output,
        );

        assert!(res.is_err());
        let err_msg = res.unwrap_err().to_string();
        assert!(
            err_msg.contains("failed to run check-script: status code 1"),
            "{}",
            err_msg
        );

        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn test_run_already_completed_dependencies() -> Result<()> {
        let mut output = Vec::new();

        let steps = vec![
            config::Step {
                id: "step".to_string(),
                script: Some(config::Script {
                    shell: Some(Shell::Bash),
                    code: "exit 0".to_string(),
                }),
                check_script: Some(config::Script {
                    shell: Some(Shell::Bash),
                    code: "exit 0".to_string(),
                }),
                source_file: "/file.yaml".to_string(),
                selection_reason: Some(MatchedFilter),
                dependency_of: vec!["step2".to_string()],
                ..Default::default()
            },
            config::Step {
                id: "step2".to_string(),
                script: Some(config::Script {
                    shell: Some(Shell::Bash),
                    code: "exit 0".to_string(),
                }),
                source_file: "/file.yaml".to_string(),
                selection_reason: Some(MatchedFilter),
                dependencies: vec!["step".to_string()],
                ..Default::default()
            },
        ];

        let res = run(
            &steps,
            &RunParameters {
                dry_run: false,
                source_file_path: Path::new("/file.yaml").to_path_buf(),
            },
            &FakeStateSaver,
            &mut DefaultScriptChecker::new(),
            None,
            &mut output,
        )?;

        let output_str = String::from_utf8(output).unwrap();
        assert!(
            output_str.contains("Step 'step' already completed"),
            "unexpected output: {}",
            output_str
        );
        assert!(
            output_str.contains("Step 'step2' completed"),
            "unexpected output: {}",
            output_str
        );

        Ok(())
    }

    #[test]
    fn test_run_script_doesnt_check_script_again() -> Result<()> {
        mock_available_shells(HashSet::from_iter([Shell::Bash]));
        let mut mock_checker = MockScriptChecker {
            check_value: Ok(()),
            is_checked_value: true,
            check_value_calls: 0,
        };

        let script = Script {
            shell: Shell::Bash,
            code: "echo \"what\"".to_string(),
        };

        run_script(
            &script,
            Path::new("/"),
            Some(&mut mock_checker),
            &mut io::sink(),
        )?;

        assert_eq!(mock_checker.check_value_calls, 0);

        mock_checker.check_value_calls = 0;
        mock_checker.is_checked_value = false;
        run_script(
            &script,
            Path::new("/"),
            Some(&mut mock_checker),
            &mut io::sink(),
        )?;

        assert_eq!(mock_checker.check_value_calls, 1);
        Ok(())
    }

    #[rstest]
    #[case(1)]
    #[case(2)]
    #[serial]
    fn test_is_completed_check_script_nonzero_exit_returns_failed(
        #[case] exit_code: i32,
    ) -> Result<()> {
        let _guard = EnvGuard::new("MEPRIS_INSTALL_COMMAND", "exit 0;");
        let step = Step {
            id: "test".to_string(),
            check_script: Some(Script {
                shell: Shell::Bash,
                code: format!("exit {exit_code}"),
            }),
            source_file: "/test.yaml".to_string(),
            package_manager: PackageManager::Apt,
            ..Default::default()
        };

        let result = step.is_completed(None)?;

        assert_eq!(result, StepCompletedResult::FailedCheckScript);
        Ok(())
    }

    #[test]
    #[serial]
    fn test_is_completed_pre_script_doesnt_require_check_script() -> Result<()> {
        let _guard = EnvGuard::new("MEPRIS_IS_INSTALLED_RESULT", "0");
        let _guard2 = EnvGuard::new("MEPRIS_INSTALL_COMMAND", "exit 0;");

        let step = Step {
            id: "test".to_string(),
            pre_script: Some(Script {
                shell: Shell::Bash,
                code: "exit 0".to_string(),
            }),
            source_file: "/test.yaml".to_string(),
            package_manager: PackageManager::Apt,
            packages: vec![Package {
                name: "pkg".to_string(),
                used_alias: false,
            }],
            ..Default::default()
        };

        let result = step.is_completed(None)?;

        assert_eq!(result, StepCompletedResult::Completed);
        Ok(())
    }

    #[test]
    #[serial]
    fn test_is_completed_script_requires_check_script() -> Result<()> {
        let _guard = EnvGuard::new("MEPRIS_INSTALL_COMMAND", "exit 0;");
        let step = Step {
            id: "test".to_string(),
            source_file: "/test.yaml".to_string(),
            package_manager: PackageManager::Apt,
            script: Some(Script {
                shell: Shell::Bash,
                code: "exit 0".to_string(),
            }),
            ..Default::default()
        };

        let result = step.is_completed(None)?;

        assert_eq!(result, StepCompletedResult::HasScriptWithoutCheck);
        Ok(())
    }

    #[test]
    #[serial]
    fn test_is_completed_script_package_manager_not_installed() -> Result<()> {
        let step = Step {
            id: "test".to_string(),
            source_file: "/test.yaml".to_string(),
            package_manager: PackageManager::Choco,
            packages: vec![Package {
                name: "pkg".to_string(),
                used_alias: false,
            }],
            script: Some(Script {
                shell: Shell::Bash,
                code: "exit 0".to_string(),
            }),
            ..Default::default()
        };

        let result = step.is_completed(None)?;

        assert_eq!(result, StepCompletedResult::NotInstalledPackageManager);
        Ok(())
    }
}
