use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
};
use std::collections::HashSet;

mod interactive;
mod logger;
pub(crate) mod dry;
mod pkg;
pub mod script_checker;

use crate::{config, config::{
    alias::load_aliases,
    PackageManager,
}};

use anyhow::{bail, Context, Result};
use colored::Colorize;
use interactive::ask_confirmation;
use logger::Logger;
use script_checker::ScriptChecker;
use crate::os_info::{Platform, OS_INFO};
use crate::runner::pkg::{install_packages, resolve_step_package_manager};
use crate::shell::Shell;

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

pub struct Script {
    shell: Shell,
    code: String,
}

pub struct Step {
    pub id: String,
    pub when_script: Option<Script>,
    pub package_manager: PackageManager,
    pub packages: Vec<String>,
    pub pre_script: Option<Script>,
    pub script: Option<Script>,
    pub source_file: String,
}

impl From<&config::Step> for Step {
    fn from(config: &config::Step) -> Self {

        let resolve_shell = |script: &Option<config::Script>| -> Option<Script> {

            if script.is_none() {
                return None;
            }

            let script = script.as_ref().unwrap();
            let res_shell: Shell;

            if script.shell.is_some() {
                res_shell = script.shell.as_ref().unwrap().clone();
            } else {
                res_shell = match OS_INFO.platform {
                    Platform::Linux => config.defaults.as_ref().and_then(|d| d.linux_shell.clone()).unwrap_or_else(Shell::default_for_current_os),
                    Platform::MacOS => config.defaults.as_ref().and_then(|d| d.macos_shell.clone()).unwrap_or_else(Shell::default_for_current_os),
                    Platform::Windows => config.defaults.as_ref().and_then(|d| d.windows_shell.clone()).unwrap_or_else(Shell::default_for_current_os),
                }
            }

            Some(Script {
                shell: res_shell,
                code: script.code.clone()
            })
        };

        Step {
            id: config.id.clone(),
            when_script: resolve_shell(&config.when_script),
            package_manager: resolve_step_package_manager(&config),
            packages: config.packages.clone(),
            pre_script: resolve_shell(&config.pre_script),
            script: resolve_shell(&config.script),
            source_file: config.source_file.clone(),
        }
    }
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

pub fn run(
    steps: &[&config::Step],
    params: &RunParameters,
    state_saver: &dyn StateSaver,
    script_checker: &mut dyn ScriptChecker,
    out: &mut impl Write,
) -> Result<Option<dry::RunPlan>> {

    let aliases = load_aliases(params.source_file_path.parent().unwrap())?;
    let mut steps: Vec<Step> = steps.iter().map(|s| (*s).into()).collect();
    check_scripts(&steps, script_checker, true)?;

    if params.dry_run {
        return dry::run(&steps, &aliases, script_checker).map(Some);
    }

    let mut logger = Logger::new(steps.len(), out);
    let mut interactive = params.interactive;

    for (i, step) in steps.iter_mut().enumerate() {
        logger.current_step = i + 1;
        step.packages = aliases.resolve_names(&step.packages, &step.package_manager);

        if let Some(when_script) = &step.when_script {
            match run_script(
                when_script,
                Path::new(&step.source_file).parent().unwrap(),
                script_checker,
                logger.out,
            ) {
                Ok(()) => (),
                Err(_) => {
                    logger.log(&format!(
                        "⏭️ PROGRESS Step '{}' skipped due to failed when script",
                        step.id
                    ))?;
                    continue;
                }
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

        if interactive {
            match ask_confirmation(&step, &mut logger)? {
                interactive::Decision::Run => {}
                interactive::Decision::Skip => continue,
                interactive::Decision::Abort => return Ok(None),
                interactive::Decision::LeaveInteractiveMode => interactive = false,
            }
        }

        logger.log(&format!("🚀 PROGRESS Running step '{}'...", step.id))?;
        run_step(&step, script_checker, &mut logger)?;
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

fn check_scripts(
    steps: &[Step],
    script_checker: &mut dyn ScriptChecker,
    skip_if_shell_unavailable: bool,
) -> Result<()> {
    for step in steps.iter() {
        if let Some(script) = &step.when_script {
            script_checker
                .check_script(script, skip_if_shell_unavailable)
                .context(format!(
                    "Failed to check when-script in {}, step '{}'",
                    step.source_file, step.id
                ))?;
        }
        if let Some(script) = &step.pre_script {
            script_checker
                .check_script(script, skip_if_shell_unavailable)
                .context(format!(
                    "Failed to check pre-script in {}, step '{}'",
                    step.source_file, step.id
                ))?;
        }
        if let Some(script) = &step.script {
            script_checker
                .check_script(script, skip_if_shell_unavailable)
                .context(format!(
                    "Failed to check script in {}, step '{}'",
                    step.source_file, step.id
                ))?;
        }
    }
    Ok(())
}

fn run_step(
    step: &Step,
    script_checker: &mut dyn ScriptChecker,
    logger: &mut Logger<impl Write>,
) -> Result<()> {
    let step_dir = Path::new(&step.source_file).parent().unwrap();

    if let Some(pre_script) = &step.pre_script {
        logger.log("⚙️ PROGRESS Running pre-script...")?;
        run_script(pre_script, step_dir, script_checker, logger.out).context(format!(
            "Failed to run pre_script in file {} step '{}'",
            step.source_file, step.id
        ))?;
    }
    if !step.packages.is_empty() {
        install_packages(&step.packages, &step.package_manager, logger)?;
    }
    if let Some(script) = &step.script {
        logger.log("⚙️ PROGRESS Running script...")?;
        run_script(script, step_dir, script_checker, logger.out).context(format!(
            "Failed to run script in file {} step '{}'",
            step.source_file, step.id
        ))?;
    }
    Ok(())
}

fn run_script(
    script: &Script,
    dir: &Path,
    script_checker: &mut dyn ScriptChecker,
    out: &mut impl Write,
) -> Result<()> {
    if !script_checker.is_checked(script) {
        script_checker.check_script(script, false)?;
    }

    let (cmd, args) = match script.shell {
        Shell::Bash => (Shell::Bash.get_command(), vec!["-c", &script.code]),
        Shell::PowerShell | Shell::PowerShellCore => (
            script.shell.get_command(),
            vec!["-NoProfile", "-Command", &script.code],
        ),
    };

    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .current_dir(dir)
        .spawn()?;

    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();

    let (tx, rx) = mpsc::channel::<Vec<u8>>();

    // read by bytes, not lines, because some programs wait for output on the same line ("Enter
    // password: <input here>") or display progress bars
    {
        let tx = tx.clone();
        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match stdout.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let _ = tx.send(buf[..n].to_vec());
                    }
                    Err(err) => {
                        eprintln!("error reading child stdout: {err}");
                        break;
                    }
                }
            }
        });
    }

    {
        let tx = tx.clone();
        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match stderr.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let _ = tx.send(buf[..n].to_vec());
                    }
                    Err(err) => {
                        eprintln!("error reading child stderr: {err}");
                        break;
                    }
                }
            }
        });
    }

    drop(tx);

    for chunk in rx {
        let s = String::from_utf8_lossy(&chunk);
        write!(out, "{s}")?;
        out.flush()?;
    }

    let status = child.wait()?;
    if !status.success() {
        match status.code() {
            Some(code) => bail!("{} script failed with code {}", cmd, code),
            None => bail!("{} script terminated by signal", cmd),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        config::PackageSource, shell::mock_available_shells,
    };

    use super::*;
    use std::{collections::HashSet, fs, io};
    use tempfile::tempdir;
    use crate::runner::script_checker::DefaultScriptChecker;

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
    fn test_run_dry_warns_abount_unavailable_shell() -> Result<()> {
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
    fn test_run_dry_warns_abount_unavailable_package_manager() -> Result<()> {
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

    #[test]
    fn test_run_dry_when_scripts() -> Result<()> {
        let mut output = Vec::new();
        mock_available_shells(HashSet::from_iter([Shell::Bash]));

        let steps = vec![config::Step {
            id: "step".to_string(),
            when_script: Some(config::Script {
                shell: Some(Shell::Bash),
                code: "exit 1".to_string(),
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
        assert!(plan.steps_ignored_by_when.contains(&steps[0].id));
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

        run_script(&script, Path::new("/"), &mut mock_checker, &mut io::sink())?;

        assert_eq!(mock_checker.check_value_calls, 0);

        mock_checker.check_value_calls = 0;
        mock_checker.is_checked_value = false;
        run_script(&script, Path::new("/"), &mut mock_checker, &mut io::sink())?;

        assert_eq!(mock_checker.check_value_calls, 1);
        Ok(())
    }
}
