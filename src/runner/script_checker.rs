use std::collections::HashSet;
use std::{io::Write, process::Command};

use anyhow::{Context, Result, bail};
use blake3::Hasher;
use tempfile::NamedTempFile;
use crate::runner::Script;
use crate::system::shell::Shell;
use crate::system::shell::is_shell_available;

pub trait ScriptChecker {
    fn check_script(&mut self, script: &Script, skip_if_shell_unavailable: bool) -> Result<()>;
    fn is_checked(&self, script: &Script) -> bool;
}

pub struct DefaultScriptChecker {
    checked: HashSet<String>,
}

impl DefaultScriptChecker {
    pub fn new() -> Self {
        Self {
            checked: HashSet::new(),
        }
    }
}

impl Default for DefaultScriptChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl ScriptChecker for DefaultScriptChecker {
    fn check_script(&mut self, script: &Script, skip_if_shell_unavailable: bool) -> Result<()> {
        if skip_if_shell_unavailable && !is_shell_available(&script.shell) {
            return Ok(());
        }

        let mut temp_file = NamedTempFile::new().context("Failed to create temp file")?;
        temp_file
            .write_all(script.code.as_bytes())
            .context("Failed to write script content")?;
        let path_str = temp_file
            .path()
            .to_str()
            .context("Temp file path is not valid UTF-8")?;

        let cmd = script.shell.get_command();
        let ps_command = format!("[scriptblock]::Create((Get-Content -Raw '{path_str}'))");
        let args = match script.shell {
            Shell::Bash => vec!["-n", path_str],
            Shell::PowerShell => vec!["-NoProfile", "-Command", &ps_command],
            Shell::PowerShellCore => vec!["-NoProfile", "-Command", &ps_command],
        };

        let output = Command::new(cmd)
            .args(args)
            .output()
            .with_context(|| format!("Failed to run shell: {cmd}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let mut to_replace = path_str.to_string();
            to_replace.push(':');

            let filtered = stderr.replace(&to_replace, "");
            bail!("{cmd} syntax error: {}", filtered.trim())
        }

        self.checked.insert(hash_script(script));
        Ok(())
    }

    fn is_checked(&self, script: &Script) -> bool {
        self.checked.contains(&hash_script(script))
    }
}

fn hash_script(script: &Script) -> String {
    let mut hasher = Hasher::new();
    hasher.update(script.shell.get_command().as_bytes());
    hasher.update(script.code.as_bytes());
    hasher.finalize().to_hex().to_string()
}

#[test]
#[cfg(unix)]
fn test_checked_script_saved() {
    let mut checker = DefaultScriptChecker::new();
    let script = Script {
        shell: Shell::Bash,
        code: "echo \"bash\"".to_string(),
    };
    assert!(!checker.is_checked(&script));
    checker.check_script(&script, false).unwrap();
    assert!(checker.is_checked(&script));
}
