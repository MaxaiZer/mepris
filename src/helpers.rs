use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub fn get_absolute_path(file: &str, base_dir: Option<&Path>) -> Result<PathBuf> {
    let path = Path::new(file);

    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }

    let cwd;
    let base = match base_dir {
        Some(p) => p,
        None => {
            cwd = std::env::current_dir().context("Failed to get current working directory")?;
            cwd.as_path()
        }
    };
    base.join(path)
        .canonicalize()
        .with_context(|| format!("Failed to canonicalize file path '{file}'"))
}
