use std::{
    collections::HashMap,
    io::{self, Write},
    path::Path,
    process::{Command, Stdio},
};

use crate::{
    check_script::ScriptChecker,
    config::{PackageManager, Script, Shell, Step},
    os_info::{OS_INFO, Platform},
    shell::is_shell_available,
};

use anyhow::{Context, Result, bail};
use which::which;

pub struct RunParameters {
    pub dry_run: bool,
}

pub struct RunState {
    pub last_step_id: Option<String>,
}

pub trait StateSaver {
    fn save(&self, info: &RunState) -> Result<()>;
}

pub struct DryRunPlan {
    pub steps_to_run: Vec<String>,
    pub steps_ignored_by_when: Vec<String>,
    pub missing_shells: HashMap<String, Vec<String>>,
    pub packages_to_install: HashMap<String, Vec<String>>,
}

pub fn run(
    steps: &[&Step],
    params: &RunParameters,
    state_saver: &dyn StateSaver,
    script_checker: &mut dyn ScriptChecker,
    out: &mut impl Write,
) -> Result<Option<DryRunPlan>> {
    check_scripts(steps, script_checker, true)?;

    if params.dry_run {
        return run_dry(steps, script_checker).map(Some);
    }

    let default_package_manager = default_package_manager(OS_INFO.platform)?;

    for step in steps {
        if let Some(when_script) = &step.when_script {
            match run_script(
                when_script,
                Path::new(&step.source_file).parent().unwrap(),
                script_checker,
                out,
            ) {
                Ok(()) => (),
                Err(_) => continue,
            }
        }

        if state_saver
            .save(&RunState {
                last_step_id: Some(step.id.clone()),
            })
            .is_err()
        {
            writeln!(out, "⚠️Failed to save run state")?;
        }

        writeln!(out, "🚀 Running step '{}'...", step.id)?;
        run_step(step, &default_package_manager, script_checker, out)?;
        writeln!(out, "✅ Step '{}' complete", step.id)?;
    }

    if state_saver.save(&RunState { last_step_id: None }).is_err() {
        writeln!(out, "⚠️Failed to save run state")?;
    }

    Ok(None)
}

fn run_dry(steps: &[&Step], script_checker: &mut dyn ScriptChecker) -> Result<DryRunPlan> {
    let mut res = DryRunPlan {
        steps_to_run: vec![],
        steps_ignored_by_when: vec![],
        missing_shells: HashMap::new(),
        packages_to_install: HashMap::new(),
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
                    res.steps_ignored_by_when.push(step.id.clone());
                    continue;
                }
            }
        }

        res.steps_to_run.push(step.id.clone());

        if !step.packages.is_empty() {
            res.packages_to_install
                .insert(step.id.clone(), step.packages.clone());
        }

        let not_available_shells = step
            .all_used_shells()
            .into_iter()
            .filter(|s| !is_shell_available(s))
            .map(|s| s.get_command())
            .collect::<Vec<&str>>();
        if !not_available_shells.is_empty() {
            res.missing_shells.insert(
                step.id.clone(),
                not_available_shells.iter().map(|s| s.to_string()).collect(),
            );
        }
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
    out: &mut impl Write,
) -> Result<()> {
    let step_dir = Path::new(&step.source_file).parent().unwrap();

    if let Some(pre_script) = &step.pre_script {
        writeln!(out, "⚙️ Running pre-script...")?;
        run_script(pre_script, step_dir, script_checker, out).context(format!(
            "Failed to run pre_script in file {} step '{}'",
            step.source_file, step.id
        ))?;
    }
    if !step.packages.is_empty() {
        let manager = step_package_manager(default_package_manager, step);
        install_packages(&step.packages, &manager, out)?;
    }
    if let Some(script) = &step.script {
        writeln!(out, "⚙️ Running script...")?;
        run_script(script, step_dir, script_checker, out).context(format!(
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
        Shell::Bash => (Shell::Bash.get_command(), vec![]),
        Shell::PowerShellCore => (
            Shell::PowerShellCore.get_command(),
            vec!["-NoProfile", "-Command", "-"],
        ),
    };

    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .current_dir(dir)
        .spawn()?;

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(script.code.as_bytes())?;

    let output = child.wait_with_output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !stdout.is_empty() {
        writeln!(out, "{stdout}")?;
    }

    if !output.status.success() {
        bail!("{} script failed:\nstderr:\n{}", cmd, stderr);
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
    if let Some(manager) = &step.package_manager {
        return manager.clone();
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
    out: &mut impl Write,
) -> Result<()> {
    if which(manager.command()).is_err() {
        bail!("Package manager {} not found", manager.command());
    }

    writeln!(out, "📦 Installing packages: {}", packages.join(", "))?;

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
    use crate::{check_script::DefaultScriptChecker, shell::mock_available_shells};

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
            &RunParameters { dry_run: false },
            &FakeStateSaver,
            &mut DefaultScriptChecker::new(),
            &mut io::stdout(),
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
            &RunParameters { dry_run: true },
            &FakeStateSaver,
            &mut DefaultScriptChecker::new(),
            &mut output,
        )
        .unwrap()
        .unwrap();

        assert!(plan.missing_shells.contains_key(&steps[0].id));
        let shells = plan.missing_shells.get(&steps[0].id).unwrap();
        assert!(
            shells.contains(
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
            &RunParameters { dry_run: true },
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

        run_script(
            &script,
            Path::new("/"),
            &mut mock_checker,
            &mut io::stdout(),
        )?;

        assert_eq!(mock_checker.check_value_calls, 0);

        mock_checker.check_value_calls = 0;
        mock_checker.is_checked_value = false;
        run_script(
            &script,
            Path::new("/"),
            &mut mock_checker,
            &mut io::stdout(),
        )?;

        assert_eq!(mock_checker.check_value_calls, 1);
        Ok(())
    }
}
