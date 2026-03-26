use crate::system::pkg::PackageManager;
use anyhow::{Context, bail};
use serde_json::Value;
use std::collections::HashSet;

pub fn parse_packages_list_func(
    manager: &PackageManager,
) -> anyhow::Result<fn(String) -> anyhow::Result<HashSet<String>>> {
    match manager {
        PackageManager::Scoop => Ok(SCOOP_PARSE_PACKAGES_LIST),
        PackageManager::Choco => Ok(CHOCO_PARSE_PACKAGES_LIST),
        PackageManager::Winget => Ok(WINGET_PARSE_PACKAGES_LIST),
        PackageManager::Npm => Ok(NPM_PARSE_PACKAGES_LIST),
        _ => bail!("unsupported package manager"),
    }
}

const WINGET_PARSE_PACKAGES_LIST: fn(String) -> anyhow::Result<HashSet<String>> = |output| {
    let mut ids = HashSet::new();
    let v: Value = serde_json::from_str(&output)
        .context(format!("couldn't parse winget output: {}", output))?;

    if let Some(sources) = v.get("Sources").and_then(|s| s.as_array()) {
        for source in sources {
            if let Some(packages) = source.get("Packages").and_then(|p| p.as_array()) {
                for package in packages {
                    if let Some(id) = package.get("PackageIdentifier").and_then(|id| id.as_str()) {
                        ids.insert(id.to_string());
                    }
                }
            }
        }
    }

    Ok(ids)
};

const SCOOP_PARSE_PACKAGES_LIST: fn(String) -> anyhow::Result<HashSet<String>> = |output| {
    let mut ids = HashSet::new();
    for line in output.lines().skip(3) {
        let line = line.trim();
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 && !line.to_lowercase().contains("failed") {
            ids.insert(parts[0].to_string());
        }
    }
    Ok(ids)
};

const CHOCO_PARSE_PACKAGES_LIST: fn(String) -> anyhow::Result<HashSet<String>> = |output| {
    let mut ids = HashSet::new();
    for line in output.lines() {
        let line = line.trim();
        if let Some((name, _)) = line.split_once('|') {
            ids.insert(name.to_string());
        }
    }

    Ok(ids)
};

const NPM_PARSE_PACKAGES_LIST: fn(String) -> anyhow::Result<HashSet<String>> = |output| {
    let mut ids = HashSet::new();
    for line in output.lines() {
        let line = line.trim();
        if let Some(pos) = line.rfind("node_modules") {
            let after = &line[pos + "node_modules".len()..];
            let name = after.trim_start_matches(['/', '\\']).replace('\\', "/");
            if !name.is_empty() {
                ids.insert(name.to_string());
            }
        }
    }
    Ok(ids)
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_winget_parse() {
        let output = include_str!("../../../tests/fixtures/winget.txt");
        let res = WINGET_PARSE_PACKAGES_LIST(output.parse().unwrap()).unwrap();
        assert!(res.contains("7zip.7zip"));
    }

    #[test]
    fn test_choco_parse() {
        let output = include_str!("../../../tests/fixtures/choco.txt");
        let res = CHOCO_PARSE_PACKAGES_LIST(output.parse().unwrap()).unwrap();
        assert!(res.contains("7zip"));
        assert!(res.contains("dbeaver"));
    }

    #[test]
    fn test_scoop_parse() {
        let output = include_str!("../../../tests/fixtures/scoop.txt");
        let res = SCOOP_PARSE_PACKAGES_LIST(output.parse().unwrap()).unwrap();
        assert!(res.contains("7zip"));
        assert!(!res.contains("meld"));
    }

    #[test]
    fn test_npm_parse() {
        let output = include_str!("../../../tests/fixtures/npm.txt");
        let res = NPM_PARSE_PACKAGES_LIST(output.to_string()).unwrap();
        assert!(res.contains("opencode-ai"));
        assert!(res.contains("yaml-language-server"));
        assert!(res.contains("@angular/cli"));
    }
}
