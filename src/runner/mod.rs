use std::cmp::PartialEq;
use std::collections::{HashMap, HashSet};
use std::{
    io::Write,
    path::{Path, PathBuf},
};

pub mod dry;
pub mod interactive;
mod pkg;
pub mod script;
pub mod script_checker;
pub mod state;

pub use interactive::{CliInteractor, Decision, Interactor};

use crate::{config, config::aliases::load_aliases};

use crate::config::StepSelectionReason;
use crate::config::aliases::PackageAliases;
use crate::logging::{EventType, SpanType};
use crate::runner::pkg::{install_packages, resolve_step_package_manager};
use crate::runner::script::ScriptStatus;
use crate::runner::script::ScriptStatus::Failed;
pub(crate) use crate::runner::script::{
    Script, ScriptResult, run_noninteractive_script, run_script,
};
use crate::system::pkg::PackageManager;
use crate::system::shell::Shell;
use anyhow::{Context, Result, bail};
use script_checker::ScriptChecker;
use tracing::{debug, debug_span, info, info_span, warn};

pub struct RunParameters {
    pub source_file_path: PathBuf,
    pub dry_run: bool,
}

pub struct RunState {
    pub completed: bool,
    pub handled_steps: Vec<String>,
    pub interactive: bool,
}

pub trait StateSaver {
    fn save(&self, info: &RunState) -> Result<()>;
}

pub struct Package {
    pub name: String,
    pub used_alias: bool,
}

pub struct StepPackages {
    pub manager: PackageManager,
    pub packages: Vec<Package>,
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
    pub package_setup: Option<StepPackages>,
    pub pre_script: Option<Script>,
    pub script: Option<Script>,
    pub check_script: Option<Script>,
    pub source_file: String,
    pub selection_reason: StepSelectionReason,
    pub dependencies: Vec<String>,
    pub dependency_of: Vec<String>,
}

impl Step {
    pub fn from(config_step: &config::Step, aliases: &PackageAliases) -> Result<Self> {
        let resolve_script = |script: &Option<config::Script>| -> Result<Option<Script>> {
            if let Some(s) = script {
                Ok(Some(Script::from_step(
                    s,
                    &config_step.defaults,
                    &config_step.id,
                    &config_step.with,
                )?))
            } else {
                Ok(None)
            }
        };

        let package_setup = if config_step.packages.is_empty() {
            None
        } else {
            let manager = resolve_step_package_manager(config_step)?.context(format!(
                "No available package manager for step '{}'",
                config_step.id
            ))?;

            let mut packages: Vec<Package> = Vec::new();
            for cfg_pkg in &config_step.packages {
                let mut resolved_pkg = Package {
                    name: aliases.resolve_name(cfg_pkg, &manager),
                    used_alias: false,
                };
                resolved_pkg.used_alias = cfg_pkg != &resolved_pkg.name;
                packages.push(resolved_pkg);
            }

            Some(StepPackages { manager, packages })
        };

        Ok(Step {
            id: config_step.id.clone(),
            package_setup,
            pre_script: resolve_script(&config_step.pre_script)?,
            script: resolve_script(&config_step.script)?,
            check_script: resolve_script(&config_step.check_script)?,
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
        })
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
        if self.package_setup.is_none() && self.check_script.is_none() {
            return match self.script {
                Some(_) => Ok(StepCompletedResult::HasScriptWithoutCheck),
                None => Ok(StepCompletedResult::Completed),
            };
        }

        let _check_span = debug_span!(SpanType::StepCheck.as_str(), step_id = self.id).entered();
        debug!(event_type = %EventType::StepCheckStarted.as_str());

        let exit = |res: StepCompletedResult| {
            // "finished" event instead of span's on_close() to avoid logging on errors (`?`)
            debug!(event_type = %EventType::StepCheckFinished.as_str());
            Ok(res)
        };

        if let Some(setup) = &self.package_setup {
            if std::env::var("MEPRIS_INSTALL_COMMAND").is_err() && !setup.manager.is_available() {
                return exit(StepCompletedResult::NotInstalledPackageManager);
            }

            let mut not_installed_pkgs = Vec::new();

            let _packages_span = debug_span!("check_packages").entered();

            for pkg in &setup.packages {
                let _package_span = debug_span!(SpanType::PackageCheck.as_str()).entered();
                if !setup.manager.is_installed(&pkg.name)? {
                    not_installed_pkgs.push(pkg.name.clone());
                }
            }

            debug!(event_type = %EventType::PackagesCheckCompleted.as_str());

            if !not_installed_pkgs.is_empty() {
                return exit(StepCompletedResult::NotInstalledPackages(
                    not_installed_pkgs,
                ));
            }
        }

        if let Some(check_script) = self.check_script.as_ref() {
            let res = run_noninteractive_script(check_script, self.directory(), script_checker)
                .context(format!("failed to run check-script for step '{}'", self.id))?;

            debug!(
                event_type = %EventType::ScriptCompleted.as_str(),
                code = res.status.code(),
                elapsed_secs = res.time.as_secs_f64(),
                kind = "check-script",
            );

            match res.status {
                Failed(_) => {
                    return exit(StepCompletedResult::FailedCheckScript);
                }
                ScriptStatus::Success => {}
            }
        } else if self.script.is_some() {
            return exit(StepCompletedResult::HasScriptWithoutCheck);
        }

        exit(StepCompletedResult::Completed)
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
    let mut steps: Vec<Step> = steps
        .iter()
        .map(|s| Step::from(s, &aliases))
        .collect::<Result<Vec<_>>>()?;

    let _span = info_span!("run").entered();
    if params.dry_run {
        return dry::run(&steps).map(Some);
    }

    let total_steps = steps.len();
    let mut interactive = interactor.is_some();
    let mut execution_results: HashMap<String, ExecutionResult> = HashMap::new();
    let mut handled_steps = Vec::<String>::new();

    for (i, step) in steps.iter_mut().enumerate() {
        let _span = info_span!(
            "step",
            step_id = step.id,
            number = i + 1,
            total = total_steps
        )
        .entered();
        let has_broken_deps = step.dependencies.iter().any(|dep| {
            execution_results
                .get(dep)
                .is_some_and(|res| res != &ExecutionResult::Completed)
        });

        if state_saver
            .save(&RunState {
                completed: false,
                handled_steps: handled_steps.clone(),
                interactive,
            })
            .is_err()
        {
            warn!("failed to save run state");
        }

        let completion = step.is_completed(Some(script_checker))?;
        if interactive && let Some(interactor) = interactor.as_mut() {
            match interactor.ask_confirmation(step, has_broken_deps, &completion, out)? {
                Decision::Run => {}
                Decision::Skip => {
                    if completion == StepCompletedResult::Completed {
                        execution_results.insert(step.id.clone(), ExecutionResult::Completed);
                    } else {
                        execution_results.insert(step.id.clone(), ExecutionResult::Skipped);
                    }
                    handled_steps.push(step.id.clone());
                    continue;
                }
                Decision::Abort => return Ok(None),
                Decision::LeaveInteractiveMode => interactive = false,
            }
        } else if completion == StepCompletedResult::Completed {
            info!(event_type = %EventType::CompletedStepSkipped.as_str());
            handled_steps.push(step.id.clone());
            continue;
        }

        if !interactive && has_broken_deps {
            bail!("cannot run step with broken dependencies without interactive mode")
        }

        run_step(step, &completion, script_checker, out)
            .context(format!("failed to run step '{}'", step.id))?;

        if has_broken_deps {
            execution_results.insert(step.id.clone(), ExecutionResult::CompletedWithMissingDeps);
        } else {
            execution_results.insert(step.id.clone(), ExecutionResult::Completed);
        }
        handled_steps.push(step.id.clone());
    }

    if state_saver
        .save(&RunState {
            completed: true,
            handled_steps: handled_steps.clone(),
            interactive,
        })
        .is_err()
    {
        warn!("failed to save run state");
    }

    info!(event_type = %EventType::RunCompleted.as_str(), interactive = interactive);
    Ok(None)
}

fn run_step(
    step: &Step,
    completion: &StepCompletedResult,
    script_checker: &mut dyn ScriptChecker,
    out: &mut impl Write,
) -> Result<()> {
    let _span = info_span!("step_run").entered();
    info!(event_type = %EventType::StepRunStarted.as_str());

    let step_dir = step.directory();

    let mut run_step_script =
        |name: &str, script: &Option<Script>, out: &mut dyn Write| -> Result<()> {
            if let Some(script) = script {
                info!(event_type = %EventType::ScriptStarted.as_str(), kind=name);
                let result = run_script(script, step_dir, Some(script_checker), out);
                if let Ok(res) = result.as_ref() {
                    debug!(
                        event_type = %EventType::ScriptCompleted.as_str(),
                        code = res.status.code(),
                        elapsed_secs = res.time.as_secs_f64(),
                        kind = name,
                    );
                }

                match result {
                    Ok(ScriptResult {
                        status: ScriptStatus::Success,
                        ..
                    }) => return Ok(()),
                    Ok(ScriptResult {
                        status: Failed(code),
                        ..
                    }) => {
                        bail!("failed to run {name}: status code {code}")
                    }
                    Err(e) => bail!("failed to run {name}: {e}"),
                }
            }
            Ok(())
        };

    run_step_script("pre-script", &step.pre_script, out)?;

    if let Some(setup) = &step.package_setup {
        let packages_to_install = match completion {
            StepCompletedResult::NotInstalledPackages(pkgs) => pkgs.clone(),
            StepCompletedResult::NotInstalledPackageManager => {
                setup.packages.iter().map(|p| p.name.clone()).collect()
            }
            _ => Vec::new(),
        };

        if !packages_to_install.is_empty() {
            install_packages(&packages_to_install, &setup.manager)?;
        }
    }

    run_step_script("script", &step.script, out)?;
    run_step_script("check-script", &step.check_script, out)?;

    info!(event_type = %EventType::StepRunFinished.as_str());
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::system::shell::mock_available_shells;

    use super::*;
    use crate::EnvGuard;
    use crate::config::StepSelectionReason::MatchedFilter;
    use crate::logging::test::run_with_tracing;
    use crate::runner::dry::RunPlan;
    use crate::runner::script_checker::DefaultScriptChecker;
    use crate::system::pkg::PackageSource;
    use rstest::rstest;
    use serial_test::serial;
    use std::io::sink;
    use std::{collections::HashSet, fs, io};
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

        let err = res.err().unwrap();
        let messages: Vec<String> = err.chain().map(|e| e.to_string()).collect();

        assert!(
            messages.contains(&"failed to run check-script: status code 1".to_string()),
            "unexpected output: {}",
            messages.join("\n")
        );

        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn test_run_already_completed_dependencies() -> Result<()> {
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

        let mut res: Option<RunPlan> = None;
        let trace_output = run_with_tracing(false, || {
            res = run(
                &steps,
                &RunParameters {
                    dry_run: false,
                    source_file_path: Path::new("/file.yaml").to_path_buf(),
                },
                &FakeStateSaver,
                &mut DefaultScriptChecker::new(),
                None,
                &mut sink(),
            )
            .unwrap();
        });

        assert!(
            trace_output
                .as_string()
                .contains("Step 'step' already completed"),
            "unexpected output: {}",
            trace_output.as_string()
        );
        assert!(
            trace_output.as_string().contains("Step 'step2' completed"),
            "unexpected output: {}",
            trace_output.as_string()
        );

        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn test_run_bash_syntax_check() -> Result<()> {
        mock_available_shells(HashSet::from_iter([Shell::Bash]));
        let steps = vec![config::Step {
            id: "step".to_string(),
            script: Some(config::Script {
                shell: Some(Shell::Bash),
                code: r#"echo "what"
                    exit '0"#
                    .to_string(),
            }),
            source_file: "/file.yaml".to_string(),
            selection_reason: Some(MatchedFilter),
            dependency_of: vec!["step2".to_string()],
            ..Default::default()
        }];

        let mut res: Result<Option<RunPlan>> = Ok(None);
        let trace_output = run_with_tracing(false, || {
            res = run(
                &steps,
                &RunParameters {
                    dry_run: false,
                    source_file_path: Path::new("/file.yaml").to_path_buf(),
                },
                &FakeStateSaver,
                &mut DefaultScriptChecker::new(),
                None,
                &mut sink(),
            )
        });

        let err = res.err().unwrap();
        let messages: Vec<String> = err.chain().map(|e| e.to_string()).collect();

        assert!(
            messages.contains(&"failed to run script: validation failed".to_string()),
            "unexpected output: {}",
            messages.join("\n")
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
            env_values: std::collections::BTreeMap::new(),
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
                env_values: std::collections::BTreeMap::new(),
            }),
            source_file: "/test.yaml".to_string(),
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
                env_values: std::collections::BTreeMap::new(),
            }),
            source_file: "/test.yaml".to_string(),
            package_setup: Some(StepPackages {
                manager: PackageManager::Apt,
                packages: vec![Package {
                    name: "pkg".to_string(),
                    used_alias: false,
                }],
            }),
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
            script: Some(Script {
                shell: Shell::Bash,
                code: "exit 0".to_string(),
                env_values: std::collections::BTreeMap::new(),
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
            package_setup: Some(StepPackages {
                manager: PackageManager::Choco,
                packages: vec![Package {
                    name: "pkg".to_string(),
                    used_alias: false,
                }],
            }),
            script: Some(Script {
                shell: Shell::Bash,
                code: "exit 0".to_string(),
                env_values: std::collections::BTreeMap::new(),
            }),
            ..Default::default()
        };

        let result = step.is_completed(None)?;

        assert_eq!(result, StepCompletedResult::NotInstalledPackageManager);
        Ok(())
    }
}
