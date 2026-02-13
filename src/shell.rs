use strum::IntoEnumIterator;
use which::which;

use std::collections::HashSet;
use std::ops::Deref;
use std::sync::Mutex;
use serde::Deserialize;
use strum_macros::EnumIter;
use crate::os_info::{Platform, OS_INFO};

static AVAILABLE_SHELLS: Mutex<Option<HashSet<Shell>>> = Mutex::new(None);
static MOCKED_SHELLS: Mutex<Option<HashSet<Shell>>> = Mutex::new(None);

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Hash, EnumIter)]
#[serde(rename_all = "lowercase")]
pub enum Shell {
    Bash,
    PowerShell,
    #[serde(rename = "pwsh")]
    PowerShellCore,
}

impl Shell {
    pub fn get_command(&self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::PowerShell => "powershell",
            Self::PowerShellCore => "pwsh",
        }
    }
    pub fn default_for_current_os() -> Self {
        let info = OS_INFO.deref();
        match info.platform {
            Platform::Linux => Shell::Bash,
            Platform::MacOS => Shell::Bash,
            Platform::Windows => Shell::PowerShell
        }
    }
}

pub fn detect_shells() {
    let mut set = HashSet::new();

    for shell in Shell::iter() {
        if which(shell.get_command()).is_ok() {
            set.insert(shell);
        }
    }
    *AVAILABLE_SHELLS.lock().unwrap() = Some(set)
}

#[cfg(test)]
pub fn mock_available_shells(mock: HashSet<Shell>) {
    *MOCKED_SHELLS.lock().unwrap() = Some(mock);
}

pub fn is_shell_available(shell: &Shell) -> bool {
    if let Some(mock) = &*MOCKED_SHELLS.lock().unwrap() {
        return mock.contains(shell);
    }

    AVAILABLE_SHELLS
        .lock()
        .unwrap()
        .as_ref()
        .map(|shells| shells.contains(shell))
        .unwrap_or(false)
}