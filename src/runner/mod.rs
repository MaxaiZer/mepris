use std::cmp::PartialEq;
use std::collections::HashSet;
use std::{
    io::Write,
    path::{Path, PathBuf},
};

pub(crate) mod dry;
mod interactive;
mod logger;
mod pkg;
mod script;
pub mod script_checker;
pub mod state;

use crate::{config, config::alias::load_aliases};

use crate::config::alias::PackageAliases;
use crate::runner::pkg::{install_packages, resolve_step_package_manager};
use crate::runner::script::{ScriptResult, run_noninteractive_script, run_script};
use crate::system::os_info::{OS_INFO, Platform};
use crate::system::pkg::PackageManager;
use crate::system::shell::Shell;
use anyhow::{Context, Result, bail};
use colored::Colorize;
use interactive::ask_confirmation;
use logger::Logger;
use script_checker::ScriptChecker;

pub struct RunParameters {
    pub source_file_path: PathBuf,
    pub interactive: bool,
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

pub struct Script {
    shell: Shell,
    code: String,
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

pub struct Step {
    pub id: String,
    pub when_script: Option<Script>,
    pub package_manager: PackageManager,
    pub packages: Vec<Package>,
    pub pre_script: Option<Script>,
    pub script: Option<Script>,
    pub check_script: Option<Script>,
    pub source_file: String,
}

impl Step {
    fn from(config_step: &config::Step, aliases: &PackageAliases) -> Self {
        let resolve_shell = |script: &Option<config::Script>| -> Option<Script> {
            if script.is_none() {
                return None;
            }

            let script = script.as_ref().unwrap();

            let res_shell: Shell = if script.shell.is_some() {
                script.shell.as_ref().unwrap().clone()
            } else {
                let default_shell = |get_shell: fn(&config::Defaults) -> Option<Shell>| {
                    config_step
                        .defaults
                        .as_ref()
                        .and_then(get_shell)
                        .unwrap_or_else(Shell::default_for_current_os)
                };

                match OS_INFO.platform {
                    Platform::Linux => default_shell(|d| d.linux_shell.clone()),
                    Platform::MacOS => default_shell(|d| d.macos_shell.clone()),
                    Platform::Windows => default_shell(|d| d.windows_shell.clone()),
                }
            };

            Some(Script {
                shell: res_shell,
                code: script.code.clone(),
            })
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
            when_script: resolve_shell(&config_step.when_script),
            package_manager: pkg_manager,
            packages,
            pre_script: resolve_shell(&config_step.pre_script),
            script: resolve_shell(&config_step.script),
            check_script: resolve_shell(&config_step.check_script),
            source_file: config_step.source_file.clone(),
        }
    }

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
            let res = run_noninteractive_script(check_script, self.directory(), script_checker)?;
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
    steps: &[&config::Step],
    params: &RunParameters,
    state_saver: &dyn StateSaver,
    script_checker: &mut dyn ScriptChecker,
    out: &mut impl Write,
) -> Result<Option<dry::RunPlan>> {
    let aliases = load_aliases(params.source_file_path.parent().unwrap())?;
    let mut steps: Vec<Step> = steps.iter().map(|s| Step::from(s, &aliases)).collect();
    check_scripts_before_run(&steps, script_checker)?;

    if params.dry_run {
        return dry::run(&steps).map(Some);
    }

    let mut logger = Logger::new(steps.len(), out);
    let mut interactive = params.interactive;

    for (i, step) in steps.iter_mut().enumerate() {
        logger.current_step = i + 1;

        if let Some(when_script) = &step.when_script {
            match run_script(
                when_script,
                Path::new(&step.source_file).parent().unwrap(),
                Some(script_checker),
                logger.out,
            ) {
                Ok(ScriptResult::Success) => (),
                Ok(ScriptResult::NotZeroExitStatus(code)) => {
                    logger.log(&format!(
                        "⏭️ PROGRESS Step '{}' skipped due to when-script returning exit code {}",
                        step.id, code
                    ))?;
                    continue;
                }
                Err(e) => bail!("Failed to run when-script for step '{}': {}", step.id, e),
            }
        }

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
        if interactive {
            match ask_confirmation(step, &completion, &mut logger)? {
                interactive::Decision::Run => {}
                interactive::Decision::Skip => continue,
                interactive::Decision::Abort => return Ok(None),
                interactive::Decision::LeaveInteractiveMode => interactive = false,
            }
        } else if completion == StepCompletedResult::Completed {
            logger.log(&format!(
                "✅ PROGRESS Step '{}' already completed, skipping",
                step.id
            ))?;
            continue;
        }

        logger.log(&format!("🚀 PROGRESS Running step '{}'...", step.id))?;
        run_step(step, script_checker, &mut logger)?;
        logger.log(&format!("✅ PROGRESS Step '{}' completed", step.id))?;
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

    writeln!(out, "✅ Run completed")?;

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
        check_step_script(step, "when-script", &step.when_script, script_checker)?;
        check_step_script(step, "pre-script", &step.pre_script, script_checker)?;
        check_step_script(step, "script", &step.script, script_checker)?;
        check_step_script(step, "check-script", &step.check_script, script_checker)?;
    }
    Ok(())
}

fn run_step(
    step: &Step,
    script_checker: &mut dyn ScriptChecker,
    logger: &mut Logger<impl Write>,
) -> Result<()> {
    let step_dir = step.directory();

    let mut run_step_script =
        |name: &str, script: &Option<Script>, logger: &mut Logger<_>| -> Result<()> {
            if let Some(script) = script {
                logger.log(&format!("⚙️ PROGRESS Running {name}..."))?;
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
            ..Default::default()
        }];

        fs::write(dir.path().join("file.txt").to_str().unwrap(), "temp file")
            .expect("Failed to write temp file");

        let _ = run(
            &steps.iter().collect::<Vec<&config::Step>>(),
            &RunParameters {
                dry_run: false,
                source_file_path: Path::new("/file.yaml").to_path_buf(),
                interactive: false,
            },
            &FakeStateSaver,
            &mut DefaultScriptChecker::new(),
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
            ..Default::default()
        }];

        let plan = run(
            &steps.iter().collect::<Vec<&config::Step>>(),
            &RunParameters {
                dry_run: true,
                source_file_path: Path::new("/file.yaml").to_path_buf(),
                interactive: false,
            },
            &FakeStateSaver,
            &mut DefaultScriptChecker::new(),
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
            ..Default::default()
        }];

        let plan = run(
            &steps.iter().collect::<Vec<&config::Step>>(),
            &RunParameters {
                dry_run: true,
                source_file_path: Path::new("/file.yaml").to_path_buf(),
                interactive: false,
            },
            &FakeStateSaver,
            &mut DefaultScriptChecker::new(),
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

    #[rstest]
    #[case(1)]
    #[case(2)]
    fn test_run_dry_when_script_exit_nonzero_skips_step(#[case] exit_code: i32) -> Result<()> {
        let mut output = Vec::new();
        mock_available_shells(HashSet::from_iter([Shell::Bash]));

        let steps = vec![config::Step {
            id: "step".to_string(),
            when_script: Some(config::Script {
                shell: Some(Shell::Bash),
                code: format!("exit {exit_code}").to_string(),
            }),
            source_file: "/file.yaml".to_string(),
            ..Default::default()
        }];

        let plan = run(
            &steps.iter().collect::<Vec<&config::Step>>(),
            &RunParameters {
                dry_run: true,
                source_file_path: Path::new("/file.yaml").to_path_buf(),
                interactive: false,
            },
            &FakeStateSaver,
            &mut DefaultScriptChecker::new(),
            &mut output,
        )?
        .unwrap();

        assert!(plan.steps_to_run.is_empty());
        assert!(plan.steps_skipped_by_when.contains(&steps[0].id));
        Ok(())
    }

    #[rstest]
    #[case(1)]
    #[case(2)]
    fn test_run_when_script_exit_nonzero_skips_step(#[case] exit_code: i32) -> Result<()> {
        let mut output = Vec::new();

        let steps = vec![config::Step {
            id: "step".to_string(),
            when_script: Some(config::Script {
                shell: Some(Shell::Bash),
                code: format!("exit {exit_code}").to_string(),
            }),
            source_file: "/file.yaml".to_string(),
            ..Default::default()
        }];

        run(
            &steps.iter().collect::<Vec<&config::Step>>(),
            &RunParameters {
                dry_run: false,
                source_file_path: Path::new("/file.yaml").to_path_buf(),
                interactive: false,
            },
            &FakeStateSaver,
            &mut DefaultScriptChecker::new(),
            &mut output,
        )?;

        let output = String::from_utf8_lossy(&output);

        assert!(
            output.contains(&format!(
                "skipped due to when-script returning exit code {exit_code}"
            )),
            "{}",
            output
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
            ..Default::default()
        }];

        let res = run(
            &steps.iter().collect::<Vec<&config::Step>>(),
            &RunParameters {
                dry_run: false,
                source_file_path: Path::new("/file.yaml").to_path_buf(),
                interactive: false,
            },
            &FakeStateSaver,
            &mut DefaultScriptChecker::new(),
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
    fn test_run_dry_when_script_exit_zero_runs_step() -> Result<()> {
        let mut output = Vec::new();
        mock_available_shells(HashSet::from_iter([Shell::Bash]));

        let steps = vec![config::Step {
            id: "step".to_string(),
            when_script: Some(config::Script {
                shell: Some(Shell::Bash),
                code: "exit 0".to_string(),
            }),
            source_file: "/file.yaml".to_string(),
            ..Default::default()
        }];

        let plan = run(
            &steps.iter().collect::<Vec<&config::Step>>(),
            &RunParameters {
                dry_run: true,
                source_file_path: Path::new("/file.yaml").to_path_buf(),
                interactive: false,
            },
            &FakeStateSaver,
            &mut DefaultScriptChecker::new(),
            &mut output,
        )?
        .unwrap();

        assert!(!plan.steps_to_run.is_empty());
        assert!(plan.steps_skipped_by_when.is_empty());
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
        unsafe {
            env::set_var("MEPRIS_INSTALL_COMMAND", "exit 0;");
        }
        let step = Step {
            id: "test".to_string(),
            check_script: Some(Script {
                shell: Shell::Bash,
                code: format!("exit {exit_code}"),
            }),
            source_file: "/test.yaml".to_string(),
            package_manager: PackageManager::Apt,
            packages: vec![],
            when_script: None,
            pre_script: None,
            script: None,
        };

        let result = step.is_completed(None)?;
        unsafe {
            env::remove_var("MEPRIS_INSTALL_COMMAND");
        }

        assert_eq!(result, StepCompletedResult::FailedCheckScript);
        Ok(())
    }

    #[test]
    #[serial]
    fn test_is_completed_pre_script_doesnt_require_check_script() -> Result<()> {
        unsafe {
            env::set_var("MEPRIS_IS_INSTALLED_RESULT", "0");
            env::set_var("MEPRIS_INSTALL_COMMAND", "exit 0;");
        }

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
            when_script: None,
            script: None,
            check_script: None,
        };

        let result = step.is_completed(None)?;

        unsafe {
            env::remove_var("MEPRIS_IS_INSTALLED_RESULT");
            env::remove_var("MEPRIS_INSTALL_COMMAND");
        }

        assert_eq!(result, StepCompletedResult::Completed);
        Ok(())
    }

    #[test]
    #[serial]
    fn test_is_completed_script_requires_check_script() -> Result<()> {
        unsafe {
            env::set_var("MEPRIS_INSTALL_COMMAND", "exit 0;");
        }
        let step = Step {
            id: "test".to_string(),
            pre_script: None,
            source_file: "/test.yaml".to_string(),
            package_manager: PackageManager::Apt,
            packages: vec![],
            when_script: None,
            script: Some(Script {
                shell: Shell::Bash,
                code: "exit 0".to_string(),
            }),
            check_script: None,
        };

        let result = step.is_completed(None)?;
        unsafe {
            env::remove_var("MEPRIS_INSTALL_COMMAND");
        }

        assert_eq!(result, StepCompletedResult::HasScriptWithoutCheck);
        Ok(())
    }

    #[test]
    #[serial]
    fn test_is_completed_script_package_manager_not_installed() -> Result<()> {
        let step = Step {
            id: "test".to_string(),
            pre_script: None,
            source_file: "/test.yaml".to_string(),
            package_manager: PackageManager::Choco,
            packages: vec![Package {
                name: "pkg".to_string(),
                used_alias: false,
            }],
            when_script: None,
            script: Some(Script {
                shell: Shell::Bash,
                code: "exit 0".to_string(),
            }),
            check_script: None,
        };

        let result = step.is_completed(None)?;

        assert_eq!(result, StepCompletedResult::NotInstalledPackageManager);
        Ok(())
    }
}
