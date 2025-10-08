use std::{
    fmt,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
};

mod interactive;
mod logger;

use crate::{
    check_script::ScriptChecker,
    config::{
        PackageManager, Script, Shell, Step,
        alias::{PackageAliases, load_aliases},
    },
    os_info::{OS_INFO, Platform},
    shell::is_shell_available,
};

use anyhow::{Context, Result, bail};
use colored::Colorize;
use interactive::ask_confirmation;
use logger::Logger;
use which::which;

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

#[derive(Default)]
pub struct StepDryRun {
    pub id: String,
    pub missing_shells: Vec<String>,
    pub package_manager: Option<PackageManagerInfo>,
    pub packages_to_install: Vec<PackageInfo>,
}

pub struct PackageInfo {
    pub name: String,
    pub use_alias: bool,
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

pub struct DryRunPlan {
    pub steps_to_run: Vec<StepDryRun>,
    pub steps_ignored_by_when: Vec<String>,
}

pub fn run(
    steps: &[&Step],
    params: &RunParameters,
    state_saver: &dyn StateSaver,
    script_checker: &mut dyn ScriptChecker,
    out: &mut impl Write,
) -> Result<Option<DryRunPlan>> {
    check_scripts(steps, script_checker, true)?;
    let aliases = load_aliases(params.source_file_path.parent().unwrap())?;

    if params.dry_run {
        return run_dry(steps, &aliases, script_checker).map(Some);
    }

    let default_package_manager = default_package_manager(OS_INFO.platform)?;
    let mut logger = Logger::new(steps.len(), out);
    let mut interactive = params.interactive;

    for (i, step) in steps.iter().cloned().enumerate() {
        logger.current_step = i + 1;

        let mut step = (*step).clone();
        step.packages = aliases.resolve_names(&step.packages, &default_package_manager);

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
        run_step(&step, &default_package_manager, script_checker, &mut logger)?;
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

fn run_dry(
    steps: &[&Step],
    aliases: &PackageAliases,
    script_checker: &mut dyn ScriptChecker,
) -> Result<DryRunPlan> {
    let mut res = DryRunPlan {
        steps_to_run: vec![],
        steps_ignored_by_when: vec![],
    };
    let default_package_manager = default_package_manager(OS_INFO.platform)?;

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
                    res.steps_ignored_by_when.push(step.id.clone());
                    continue;
                }
            }
        }

        let mut step_dry_run = StepDryRun {
            id: step.id.clone(),
            ..Default::default()
        };

        if !step.packages.is_empty() {
            let package_manager = step_package_manager(&default_package_manager, step);

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

fn check_scripts(
    steps: &[&Step],
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
    default_package_manager: &PackageManager,
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
        let manager = step_package_manager(default_package_manager, step);
        install_packages(&step.packages, &manager, logger)?;
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
        Shell::PowerShellCore => (
            Shell::PowerShellCore.get_command(),
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

pub fn default_package_manager(platform: Platform) -> Result<PackageManager> {
    if platform == Platform::MacOS {
        return Ok(PackageManager::Brew);
    }
    if platform == Platform::Windows {
        return Ok(PackageManager::Winget);
    }

    let managers = [
        PackageManager::Pacman,
        PackageManager::Apt,
        PackageManager::Dnf,
        PackageManager::Zypper,
    ];

    for mgr in managers {
        if which(mgr.command()).is_ok() {
            return Ok(mgr);
        }
    }
    bail!("Could not detect package manager")
}

pub fn step_package_manager(default_manager: &PackageManager, step: &Step) -> PackageManager {
    if let Some(source) = &step.package_source {
        if let Some(manager) = source
            .get_package_managers()
            .iter()
            .find(|m| which(m.command()).is_ok())
        {
            return manager.clone();
        } else {
            return source.get_package_managers()[0].clone();
        }
    }

    if let Some(win_pm) = step
        .defaults
        .as_ref()
        .and_then(|d| d.windows_package_manager.clone())
        && OS_INFO.platform == Platform::Windows
    {
        return win_pm;
    }

    default_manager.clone()
}

fn install_packages(
    packages: &[String],
    manager: &PackageManager,
    logger: &mut Logger<impl Write>,
) -> Result<()> {
    if which(manager.command()).is_err() {
        bail!("Package manager {} not found", manager.command());
    }

    logger.log(&format!(
        "📦 PROGRESS Installing packages: {}",
        packages.join(", ")
    ))?;

    let commands = manager.commands_to_install(packages);
    for cmd in commands {
        let status = Command::new(cmd.bin)
            .args(cmd.args)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .context(format!("Failed to install {}", packages.join(", ")))?;

        if !status.success() {
            bail!("Failed to install {}", packages.join(", "));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        check_script::DefaultScriptChecker, config::PackageSource, shell::mock_available_shells,
    };

    use super::*;
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
    #[cfg(unix)]
    fn test_run_script_from_file_dir() -> Result<()> {
        use std::{collections::HashSet, io};

        use anyhow::Ok;

        mock_available_shells(HashSet::from_iter([Shell::Bash]));
        let dir = tempdir().expect("Failed to create temp dir");
        let step_path = dir.path().join("file.yaml").to_str().unwrap().to_string();

        let steps = vec![Step {
            id: "parent".to_string(),
            script: Some(Script {
                shell: Shell::Bash,
                code: "cat file.txt".to_string(),
            }),
            source_file: step_path.clone(),
            ..Default::default()
        }];

        fs::write(dir.path().join("file.txt").to_str().unwrap(), "temp file")
            .expect("Failed to write temp file");

        let _ = run(
            &steps.iter().collect::<Vec<&Step>>(),
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

        let steps = vec![Step {
            id: "step".to_string(),
            script: Some(Script {
                shell: Shell::PowerShellCore,
                code: "cat file.txt".to_string(),
            }),
            ..Default::default()
        }];

        let plan = run(
            &steps.iter().collect::<Vec<&Step>>(),
            &RunParameters {
                dry_run: true,
                source_file_path: Path::new("/file.yaml").to_path_buf(),
                interactive: false,
            },
            &FakeStateSaver,
            &mut DefaultScriptChecker::new(),
            &mut output,
        )
        .unwrap()
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

        let steps = vec![Step {
            id: "step".to_string(),
            packages: vec!["git".to_string()],
            package_source: Some(PackageSource::Manager(PackageManager::Choco)),
            ..Default::default()
        }];

        let plan = run(
            &steps.iter().collect::<Vec<&Step>>(),
            &RunParameters {
                dry_run: true,
                source_file_path: Path::new("/file.yaml").to_path_buf(),
                interactive: false,
            },
            &FakeStateSaver,
            &mut DefaultScriptChecker::new(),
            &mut output,
        )
        .unwrap()
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

        let steps = vec![Step {
            id: "step".to_string(),
            when_script: Some(Script {
                shell: Shell::Bash,
                code: "exit 1".to_string(),
            }),
            source_file: "/file.yaml".to_string(),
            ..Default::default()
        }];

        let plan = run(
            &steps.iter().collect::<Vec<&Step>>(),
            &RunParameters {
                dry_run: true,
                source_file_path: Path::new("/file.yaml").to_path_buf(),
                interactive: false,
            },
            &FakeStateSaver,
            &mut DefaultScriptChecker::new(),
            &mut output,
        )
        .unwrap()
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
