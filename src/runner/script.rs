use crate::{config, utils};
use crate::runner::script_checker::ScriptChecker;
use crate::system::os_info::{OS_INFO, Platform};
use crate::system::shell::Shell;
use anyhow::{Context, bail};
use std::collections::BTreeMap;
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
    pub env_values: BTreeMap<String, String>,
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
            env_values: BTreeMap::new(),
        }
    }

    pub fn from_step(
        script: &config::Script,
        defaults: &Option<config::Defaults>,
        step_id: &str,
        with: &Option<BTreeMap<String, String>>,
    ) -> anyhow::Result<Self> {
        let mut script = Self::from(script, defaults);
        script.env_values = resolve_template_env(step_id, with)?;
        Ok(script)
    }
}

pub fn resolve_template_env(
    step_id: &str,
    with: &Option<BTreeMap<String, String>>,
) -> anyhow::Result<BTreeMap<String, String>> {
    let mut resolved = BTreeMap::new();
    if let Some(values) = with {
        for (key, value) in values {
            match resolve_template_value(step_id, key, value) {
                Ok(value) => {
                    resolved.insert(key.clone(), value);
                }
                Err(err) => return Err(err),
            }
        }
    }
    Ok(resolved)
}

fn resolve_template_value(step_id: &str, key: &str, value: &str) -> anyhow::Result<String> {
    let mut out = String::new();
    let chars: Vec<char> = value.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        if chars[i] == '$' && i + 1 < len && chars[i + 1] == '{' {
            match chars[i + 2..].iter().position(|&c| c == '}') {
                Some(close) => {
                    let name: String = chars[i + 2..i + 2 + close].iter().collect();
                    if utils::env::is_valid_env_var_name(&name) {
                        match env::var(name.as_str()) {
                            Ok(value) => out.push_str(&value),
                            Err(_) => bail!(
                                "step '{}': undefined environment variable '{}' referenced in 'with' value for key '{}'",
                                step_id,
                                name,
                                key
                            ),
                        }
                    } else {
                        bail!(
                            "step '{}': invalid variable name '{}' in 'with' value for key '{}' (expected [A-Za-z_][A-Za-z0-9_]*)",
                            step_id,
                            name,
                            key
                        );
                    }
                    i = i + 2 + close + 1;
                }
                None => {
                    out.push('$');
                    i += 1;
                }
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }

    Ok(out)
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
        &script.env_values,
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
    let mut command = Command::new(cmd);
    command.args(args);
    for (key, value) in &script.env_values {
        command.env(key, value);
    }
    command.stdin(Stdio::null());
    command.stdout(Stdio::null());
    command.stderr(Stdio::null());
    command.current_dir(dir);

    let mut child = command.spawn().context(format!("failed to run {}", cmd))?;

    let time = Instant::now();
    let status = child.wait()?;
    get_script_result(&status, &time)
}

fn get_script_cmd(script: &Script) -> (&str, Vec<String>, Option<TempPath>) {
    match script.shell {
        Shell::Bash => (
            Shell::Bash.get_command(),
            vec!["-e".into(), "-c".into(), script.code.clone()],
            None,
        ),
        Shell::PowerShell | Shell::PowerShellCore => {
            let mut temp = Builder::new()
                .suffix(".ps1")
                .tempfile()
                .context("failed to create temp file")
                .unwrap();
            let error_policy = r#"
            $ErrorActionPreference = 'Stop'
            $PSNativeCommandUseErrorActionPreference = $true
            "#;
            writeln!(temp, "{error_policy}\n{}", script.code)
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
    env_values: &BTreeMap<String, String>,
    testable_output: bool,
    out: &mut dyn Write,
) -> anyhow::Result<std::process::ExitStatus> {
    let mut command = Command::new(cmd);
    command.args(args);
    for (key, value) in env_values {
        command.env(key, value);
    }
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
    use serial_test::serial;

    use super::*;

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

    #[test]
    fn test_resolve_template_value_constant_and_bare_dollar() {
        let value = resolve_template_value("s", "k", "/usr/bin").unwrap();
        assert_eq!(value, "/usr/bin");

        let value = resolve_template_value("s", "k", "$HOME and $ A").unwrap();
        assert_eq!(value, "$HOME and $ A");
    }

    #[test]
    #[serial]
    fn test_resolve_template_value_substitution() {
        let _guard = crate::EnvGuard::new("MEPRIS_TPL_TEST_HOME", "/home/user");
        let value = resolve_template_value("s", "k", "${MEPRIS_TPL_TEST_HOME}/data").unwrap();
        assert_eq!(value, "/home/user/data");

        let value = resolve_template_value("s", "k", "pre-${MEPRIS_TPL_TEST_HOME}-post").unwrap();
        assert_eq!(value, "pre-/home/user-post");
    }

    #[test]
    fn test_resolve_template_value_missing_var() {
        let err = resolve_template_value("my-step", "PATH_VAR", "${MEPRIS_UNSET_VARIABLE_12345}")
            .err()
            .unwrap();
        let msg = err.to_string();
        assert!(msg.contains("my-step"), "{msg}");
        assert!(msg.contains("PATH_VAR"), "{msg}");
        assert!(msg.contains("MEPRIS_UNSET_VARIABLE_12345"), "{msg}");
    }

    #[test]
    fn test_resolve_template_value_invalid_name() {
        let msg = resolve_template_value("s", "k", "${1BAD}")
            .err()
            .unwrap()
            .to_string();
        assert!(msg.contains("invalid variable name '1BAD'"), "{msg}");
    }

    #[test]
    fn test_resolve_template_value_unclosed_brace_is_literal() {
        let value = resolve_template_value("s", "k", "${UNCLOSED").unwrap();
        assert_eq!(value, "${UNCLOSED");

        let value = resolve_template_value("s", "k", "$").unwrap();
        assert_eq!(value, "$");
    }

    #[test]
    fn test_resolve_template_env_map() {
        let mut with = BTreeMap::new();
        with.insert("A".to_string(), "foo".to_string());
        let resolved = resolve_template_env("s", &Some(with)).unwrap();
        assert_eq!(resolved.get("A").unwrap(), "foo");

        assert!(resolve_template_env("s", &None).unwrap().is_empty());
    }
}
