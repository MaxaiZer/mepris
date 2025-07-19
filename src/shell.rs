use strum::IntoEnumIterator;
use which::which;

use crate::config::Shell;
use std::collections::HashSet;
use std::sync::Mutex;

static AVAILABLE_SHELLS: Mutex<Option<HashSet<Shell>>> = Mutex::new(None);
static MOCKED_SHELLS: Mutex<Option<HashSet<Shell>>> = Mutex::new(None);

pub fn detect_shells() {
    let mut set = HashSet::new();

    for shell in Shell::iter() {
        if which(shell.get_command()).is_ok() {
            set.insert(shell);
        }
    }
    *AVAILABLE_SHELLS.lock().unwrap() = Some(set)
}

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
