use crate::runner::Script;
use crate::runner::script_checker::ScriptChecker;
use crate::system::shell::Shell;
use anyhow::bail;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;

pub enum ScriptResult {
    Success,
    NotZeroExitStatus(i32),
}

pub fn run_script(
    script: &Script,
    dir: &Path,
    script_checker: Option<&mut dyn ScriptChecker>,
    out: &mut impl Write,
) -> anyhow::Result<ScriptResult> {
    if let Some(script_checker) = script_checker {
        if !script_checker.is_checked(script) {
            script_checker.check_script(script, false)?;
        }
    }

    let (cmd, args) = get_script_cmd(script);

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
        .spawn()?;

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
