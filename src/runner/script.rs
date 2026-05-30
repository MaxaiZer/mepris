use crate::config;
use crate::runner::script_checker::ScriptChecker;
use crate::system::os_info::{OS_INFO, Platform};
use crate::system::shell::Shell;
use anyhow::{Context, bail};
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use std::{env, thread};
use tempfile::Builder;
use tempfile::TempPath;

pub struct Script {
    pub shell: Shell,
    pub code: String,
}

impl Script {
    pub fn from(script: &config::Script, defaults: &Option<config::Defaults>) -> Self {
        let res_shell: Shell = if script.shell.is_some() {
            script.shell.as_ref().unwrap().clone()
        } else {
            resolve_shell(OS_INFO.platform, defaults)
        };

        Script {
            shell: res_shell,
            code: script.code.clone(),
        }
    }
}

pub fn resolve_shell(platform: Platform, defaults: &Option<config::Defaults>) -> Shell {
    let default_shell = |get_shell: fn(&config::Defaults) -> Option<Shell>| {
        defaults
            .as_ref()
            .and_then(get_shell)
            .unwrap_or_else(|| Shell::default_for_platform(platform))
    };

    match platform {
        Platform::Linux => default_shell(|d| d.linux_shell.clone()),
        Platform::MacOS => default_shell(|d| d.macos_shell.clone()),
        Platform::Windows => default_shell(|d| d.windows_shell.clone()),
    }
}

pub enum ScriptStatus {
    Success,
    Failed(i32),
}

impl ScriptStatus {
    pub fn code(&self) -> i32 {
        match self {
            ScriptStatus::Success => 0,
            ScriptStatus::Failed(code) => *code,
        }
    }
}

pub struct ScriptResult {
    pub status: ScriptStatus,
    pub time: Duration,
}

pub fn run_script(
    script: &Script,
    dir: &Path,
    script_checker: Option<&mut dyn ScriptChecker>,
    out: &mut dyn Write,
) -> anyhow::Result<ScriptResult> {
    if let Some(script_checker) = script_checker {
        if requires_syntax_check_before_run(script.shell.clone())
            && !script_checker.is_checked(script)
        {
            script_checker
                .check_script(script, false)
                .context("validation failed")?;
        }
    }

    let (cmd, args, _temp_file) = get_script_cmd(script);
    let time = Instant::now();
    let status = run_interactive_command(
        dir,
        (cmd, &args),
        env::var("MEPRIS_TEST_SCRIPT_OUTPUT").is_ok(),
        out,
    )?;
    get_script_result(&status, &time)
}

pub fn run_noninteractive_script(
    script: &Script,
    dir: &Path,
    script_checker: Option<&mut dyn ScriptChecker>,
) -> anyhow::Result<ScriptResult> {
    if let Some(script_checker) = script_checker {
        if !script_checker.is_checked(script) {
            script_checker.check_script(script, false)?;
        }
    }

    let (cmd, args, _temp_file) = get_script_cmd(script);
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .current_dir(dir)
        .spawn()
        .context(format!("failed to run {}", cmd))?;

    let time = Instant::now();
    let status = child.wait()?;
    get_script_result(&status, &time)
}

fn get_script_cmd(script: &Script) -> (&str, Vec<String>, Option<TempPath>) {
    match script.shell {
        Shell::Bash => (
            Shell::Bash.get_command(),
            vec!["-c".into(), script.code.clone()],
            None,
        ),
        Shell::PowerShell | Shell::PowerShellCore => {
            let mut temp = Builder::new()
                .suffix(".ps1")
                .tempfile()
                .context("failed to create temp file")
                .unwrap();

            writeln!(temp, "{}", script.code)
                .context("failed to write script to temp file")
                .unwrap();

            let path = temp.path().to_string_lossy().to_string();
            let temp_path = temp.into_temp_path();
            (
                script.shell.get_command(),
                vec!["-NoProfile".into(), "-File".into(), path],
                Some(temp_path),
            )
        }
    }
}

fn requires_syntax_check_before_run(shell: Shell) -> bool {
    match shell {
        Shell::Bash => true,
        Shell::PowerShell | Shell::PowerShellCore => false,
    }
}

fn get_script_result(
    status: &std::process::ExitStatus,
    time: &Instant,
) -> anyhow::Result<ScriptResult> {
    if !status.success() {
        match status.code() {
            Some(code) => {
                return Ok(ScriptResult {
                    status: ScriptStatus::Failed(code),
                    time: time.elapsed(),
                });
            }
            None => bail!("script terminated by signal"),
        }
    }
    Ok(ScriptResult {
        status: ScriptStatus::Success,
        time: time.elapsed(),
    })
}

fn run_interactive_command(
    dir: &Path,
    (cmd, args): (&str, &[String]),
    testable_output: bool,
    out: &mut dyn Write,
) -> anyhow::Result<std::process::ExitStatus> {
    let mut command = Command::new(cmd);
    command.args(args);
    command.stdin(Stdio::inherit());

    if testable_output {
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
    } else {
        command.stdout(Stdio::inherit()).stderr(Stdio::inherit());
    }

    let mut child = command
        .current_dir(dir)
        .spawn()
        .context(format!("failed to run {}", cmd))?;

    if testable_output {
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
    }

    let status = child.wait()?;
    Ok(status)
}

#[cfg(test)]
mod tests {
    use crate::config::Defaults;
    use crate::runner::script::resolve_shell;
    use crate::system::os_info::Platform::{Linux, MacOS, Windows};
    use crate::system::shell::Shell::{Bash, PowerShell, PowerShellCore};

    #[test]
    fn test_resolve_shell_no_defaults() {
        let linux_shell = resolve_shell(Linux, &None);
        let windows_shell = resolve_shell(Windows, &None);
        let macos_shell = resolve_shell(MacOS, &None);

        assert_eq!(linux_shell, Bash);
        assert_eq!(windows_shell, PowerShell);
        assert_eq!(macos_shell, Bash);
    }

    #[test]
    fn test_resolve_shell_none_defaults() {

        let defaults = Defaults {
            windows_package_manager: None,
            windows_shell: None,
            linux_shell: None,
            macos_shell: None,
        };

        let linux_shell = resolve_shell(Linux, &Some(defaults.clone()));
        let windows_shell = resolve_shell(Windows, &Some(defaults.clone()));
        let macos_shell = resolve_shell(MacOS, &Some(defaults.clone()));

        assert_eq!(linux_shell, Bash);
        assert_eq!(windows_shell, PowerShell);
        assert_eq!(macos_shell, Bash);
    }

    #[test]
    fn test_resolve_shell_defaults_override() {

        let defaults = Defaults {
            windows_package_manager: None,
            windows_shell: Some(Bash),
            linux_shell: Some(PowerShellCore),
            macos_shell: Some(PowerShell),
        };

        let linux_shell = resolve_shell(Linux, &Some(defaults.clone()));
        let windows_shell = resolve_shell(Windows, &Some(defaults.clone()));
        let macos_shell = resolve_shell(MacOS, &Some(defaults.clone()));

        assert_eq!(linux_shell, PowerShellCore);
        assert_eq!(windows_shell, Bash);
        assert_eq!(macos_shell, PowerShell);
    }
}
