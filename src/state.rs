use std::{
    fs::{self, File},
    io::{Read, Write},
    path::PathBuf,
};

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Serialize, de::DeserializeOwned};

pub fn save<T: Serialize>(state: &T) -> Result<()> {
    let path = get_state_path()?;
    let mut file = File::create(path).context("Failed to create state file")?;
    file.write_all(serde_json::to_string(state)?.as_bytes())
        .context("Failed to write to state file")?;
    Ok(())
}

pub fn get<T: DeserializeOwned>() -> Result<T> {
    let path = get_state_path()?;
    let mut file = File::open(path).context("Failed to open state file")?;
    let mut content = String::new();
    file.read_to_string(&mut content)
        .context("Failed to read state file")?;
    serde_json::from_str(&content).context("Failed to parse state file")
}

fn get_state_path() -> Result<PathBuf> {
    if std::env::var("STATE_PATH").is_ok() {
        let path = std::env::var("STATE_PATH").map(PathBuf::from)?;
        return Ok(path);
    }
    let dirs = ProjectDirs::from("", "", "mepris").context("Could not get project dirs")?;
    let data_dir = dirs.data_dir();
    fs::create_dir_all(data_dir).context("Failed to create data directory")?;
    Ok(data_dir.join("state.json"))
}
