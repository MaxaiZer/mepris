use crate::config;
use crate::runner::script_checker::ScriptChecker;
use crate::system::os_info::{OS_INFO, Platform};
use crate::system::shell::Shell;
use anyhow::{Context, bail};
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::{env, thread};

pub struct Script {
    pub shell: Shell,
    pub code: String,
}

impl Script {
    pub fn from(script: &config::Script, defaults: &Option<config::Defaults>) -> Self {
        let res_shell: Shell = if script.shell.is_some() {
            script.shell.as_ref().unwrap().clone()
        } else {
            let default_shell = |get_shell: fn(&config::Defaults) -> Option<Shell>| {
                defaults
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

        Script {
            shell: res_shell,
            code: script.code.clone(),
        }
    }
}

pub enum ScriptResult {
    Success,
    NotZeroExitStatus(i32),
}

pub fn run_script(
    script: &Script,
    dir: &Path,
    script_checker: Option<&mut dyn ScriptChecker>,
    out: &mut dyn Write,
) -> anyhow::Result<ScriptResult> {
    if let Some(script_checker) = script_checker {
        if !script_checker.is_checked(script) {
            script_checker.check_script(script, false)?;
        }
    }

    let (cmd, args) = get_script_cmd(script);

    let status = run_interactive_command(
        dir,
        (cmd, args),
        env::var("MEPRIS_TEST_SCRIPT_OUTPUT").is_ok(),
        out,
    )?;
    get_script_result(&status)
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

    let (cmd, args) = get_script_cmd(script);
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .current_dir(dir)
        .spawn()
        .context(format!("failed to run {}", cmd))?;

    let status = child.wait()?;
    get_script_result(&status)
}

fn get_script_cmd(script: &Script) -> (&str, Vec<&str>) {
    match script.shell {
        Shell::Bash => (Shell::Bash.get_command(), vec!["-c", &*script.code]),
        Shell::PowerShell | Shell::PowerShellCore => (
            script.shell.get_command(),
            vec!["-NoProfile", "-Command", &*script.code],
        ),
    }
}

fn get_script_result(status: &std::process::ExitStatus) -> anyhow::Result<ScriptResult> {
    if !status.success() {
        match status.code() {
            Some(code) => return Ok(ScriptResult::NotZeroExitStatus(code)),
            None => bail!("script terminated by signal"),
        }
    }
    Ok(ScriptResult::Success)
}

fn run_interactive_command(
    dir: &Path,
    (cmd, args): (&str, Vec<&str>),
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
