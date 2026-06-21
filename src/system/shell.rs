use strum::IntoEnumIterator;
use which::which;

use crate::system::os_info::Platform;
use serde::Deserialize;
use std::collections::HashSet;
use std::sync::Mutex;
use strum_macros::EnumIter;

static AVAILABLE_SHELLS: Mutex<Option<HashSet<Shell>>> = Mutex::new(None);
static MOCKED_SHELLS: Mutex<Option<HashSet<Shell>>> = Mutex::new(None);

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Hash, EnumIter)]
#[serde(rename_all = "lowercase")]
pub enum Shell {
    Bash,
    PowerShell,
    #[serde(rename = "pwsh")]
    PowerShellCore,
    #[serde(rename = "nu")]
    Nushell,
}

impl Shell {
    pub fn get_command(&self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::PowerShell => "powershell",
            Self::PowerShellCore => "pwsh",
            Self::Nushell => "nu",
        }
    }

    pub fn default_for_platform(platform: Platform) -> Self {
        match platform {
            Platform::Linux => Shell::Bash,
            Platform::MacOS => Shell::Bash,
            Platform::Windows => Shell::PowerShell,
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
