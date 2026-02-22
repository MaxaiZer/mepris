use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader},
};

use anyhow::{anyhow, bail, Context, Result};
use once_cell::sync::Lazy;
use which::which;
use crate::system::pkg::PackageManager;

pub static OS_INFO: Lazy<OsInfo> = Lazy::new(|| get_os_info().expect("Failed to get OS info"));
pub static DEFAULT_PACKAGE_MANAGER: Lazy<PackageManager> = Lazy::new(|| default_package_manager().expect("Failed to define the default package manager"));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Linux,
    MacOS,
    Windows,
}

impl Platform {
    pub fn detect() -> Self {
        match std::env::consts::OS {
            "linux" => Self::Linux,
            "macos" => Self::MacOS,
            "windows" => Self::Windows,
            _ => panic!("Unknown platform"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::MacOS => "macos",
            Self::Windows => "windows",
        }
    }
}

#[derive(Debug)]
pub struct OsInfo {
    pub platform: Platform,
    pub id: Option<String>,
    pub id_like: Vec<String>,
}

fn get_os_info() -> Result<OsInfo> {
    let platform = Platform::detect();

    if platform != Platform::Linux {
        return Ok(OsInfo {
            platform,
            id: None,
            id_like: vec![],
        });
    }

    let file = File::open("/etc/os-release").context("Failed to open /etc/os-release")?;
    let reader = BufReader::new(file);
    let mut file_values = HashMap::new();

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, val)) = line.split_once('=') {
            let val = val.trim_matches('"').to_string();
            file_values.insert(key.to_string(), val);
        }
    }

    let id = file_values.get("ID").cloned();
    let id_like = file_values.get("ID_LIKE").map_or_else(Vec::new, |val| {
        val.split_whitespace().map(str::to_string).collect()
    });

    Ok(OsInfo {
        platform,
        id,
        id_like,
    })
}

fn default_package_manager() -> Result<PackageManager> {
    let platform = OS_INFO.platform;
    if let Ok(fake) = std::env::var("MEPRIS_FAKE_PACKAGE_MANAGER") {
        return fake.parse().map_err(|_| anyhow!("Invalid fake package manager"));
    }

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