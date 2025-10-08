use anyhow::Context;
use anyhow::Result;
use directories::ProjectDirs;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use strum::IntoEnumIterator;

use crate::config::PackageManager;
use crate::config::PackageSource;
use crate::config::Repository;

#[derive(Deserialize, Debug, Default)]
pub struct PackageAliases(HashMap<String, HashMap<PackageSource, String>>);

impl PackageAliases {
    pub fn resolve_name(&self, package: &str, manager: &PackageManager) -> String {
        let mut source = PackageSource::Manager(manager.clone());
        for repo in Repository::iter() {
            if repo.get_package_managers().contains(manager) {
                source = PackageSource::Repository(repo);
            }
        }

        self.0
            .get(package)
            .and_then(|map| map.get(&source))
            .cloned()
            .unwrap_or(package.to_string())
    }

    pub fn resolve_names(&self, packages: &[String], manager: &PackageManager) -> Vec<String> {
        packages
            .iter()
            .map(|pkg| self.resolve_name(pkg, manager))
            .collect()
    }

    pub fn merge(&self, other: &PackageAliases) -> PackageAliases {
        let mut merged = self.0.clone();

        for (pkg, local_map) in &other.0 {
            merged
                .entry(pkg.clone())
                .and_modify(|pkg_map| {
                    for (source, alias) in local_map {
                        pkg_map.insert(source.clone(), alias.clone());
                    }
                })
                .or_insert_with(|| local_map.clone());
        }

        PackageAliases(merged)
    }
}

pub fn load_aliases(file_directory: &Path) -> Result<PackageAliases> {
    let global_file_path = get_global_aliases_path()?;
    let local_file_path = file_directory.join("pkg_aliases.yaml");

    let mut aliases = PackageAliases::default();

    if global_file_path.exists() {
        let content = fs::read_to_string(&global_file_path)
            .with_context(|| format!("Failed to read {}", global_file_path.display()))?;
        aliases = serde_yaml::from_str(&content).with_context(|| {
            format!(
                "Failed to parse package aliases in {}",
                global_file_path.display()
            )
        })?;
    }

    if local_file_path.exists() {
        let content = fs::read_to_string(&local_file_path)
            .with_context(|| format!("Failed to read {}", local_file_path.display()))?;
        let local_aliases: PackageAliases = serde_yaml::from_str(&content).with_context(|| {
            format!(
                "Failed to parse package aliases in {}",
                local_file_path.display()
            )
        })?;
        Ok(aliases.merge(&local_aliases))
    } else {
        Ok(aliases)
    }
}

fn get_global_aliases_path() -> Result<PathBuf> {
    if let Ok(custom_path) = std::env::var("GLOBAL_ALIASES_PATH") {
        let path = PathBuf::from(custom_path);
        if path.exists() {
            return Ok(path);
        }
    }
    let dirs = ProjectDirs::from("", "", "mepris").context("Could not get project dirs")?;
    Ok(dirs.config_dir().join("pkg_aliases.yaml"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{PackageManager, PackageSource, Repository};

    #[test]
    fn test_resolve_name() {
        let mut aliases = PackageAliases::default();

        aliases.0.insert(
            "firefox".to_string(),
            vec![(
                PackageSource::Manager(PackageManager::Flatpak),
                "org.mozilla.firefox".to_string(),
            )]
            .into_iter()
            .collect(),
        );

        let name1 = aliases.resolve_name("firefox", &PackageManager::Pacman);
        assert_eq!(name1, "firefox");

        let name2 = aliases.resolve_name("firefox", &PackageManager::Flatpak);
        assert_eq!(name2, "org.mozilla.firefox");

        let name3 = aliases.resolve_name("chrome", &PackageManager::Pacman);
        assert_eq!(name3, "chrome");
    }

    #[test]
    fn test_resolve_name_aur() {
        let mut aliases = PackageAliases::default();

        aliases.0.insert(
            "vim".to_string(),
            vec![(
                PackageSource::Repository(Repository::Aur),
                "vim-aur".to_string(),
            )]
            .into_iter()
            .collect(),
        );

        let name_yay = aliases.resolve_name("vim", &PackageManager::Yay);
        assert_eq!(name_yay, "vim-aur");

        let name_paru = aliases.resolve_name("vim", &PackageManager::Paru);
        assert_eq!(name_paru, "vim-aur");

        let name_pacman = aliases.resolve_name("vim", &PackageManager::Pacman);
        assert_eq!(name_pacman, "vim");
    }

    #[test]
    fn test_resolve_names() {
        let mut aliases = PackageAliases::default();

        aliases.0.insert(
            "vim".to_string(),
            vec![(
                PackageSource::Repository(Repository::Aur),
                "vim-aur".to_string(),
            )]
            .into_iter()
            .collect(),
        );
        aliases.0.insert(
            "nano".to_string(),
            vec![(
                PackageSource::Manager(PackageManager::Pacman),
                "nano-pac".to_string(),
            )]
            .into_iter()
            .collect(),
        );

        let pkgs = vec!["vim".to_string(), "nano".to_string(), "htop".to_string()];
        let resolved = aliases.resolve_names(&pkgs, &PackageManager::Pacman);

        assert_eq!(resolved, vec!["vim", "nano-pac", "htop"]);
    }

    #[test]
    fn test_merge_aliases() {
        let mut a1 = PackageAliases::default();
        a1.0.insert(
            "pkg_both".to_string(),
            vec![
                (
                    PackageSource::Repository(Repository::Aur),
                    "pkg_both_aur".to_string(),
                ),
                (
                    PackageSource::Manager(PackageManager::Apt),
                    "pkg_both_apt".to_string(),
                ),
            ]
            .into_iter()
            .collect(),
        );
        a1.0.insert(
            "pkg_only_first".to_string(),
            vec![(
                PackageSource::Manager(PackageManager::Flatpak),
                "pkg_only_first_flatpak".to_string(),
            )]
            .into_iter()
            .collect(),
        );

        let mut a2 = PackageAliases::default();
        a2.0.insert(
            "pkg_both".to_string(),
            vec![
                (
                    PackageSource::Manager(PackageManager::Pacman),
                    "pkg_both_pacman".to_string(),
                ),
                (
                    PackageSource::Manager(PackageManager::Apt),
                    "pkg_both_apt_overriden".to_string(),
                ),
            ]
            .into_iter()
            .collect(),
        );
        a2.0.insert(
            "pkg_only_second".to_string(),
            vec![(
                PackageSource::Manager(PackageManager::Flatpak),
                "pkg_only_second_flatpak".to_string(),
            )]
            .into_iter()
            .collect(),
        );

        let merged = a1.merge(&a2);

        let pkg_both_map = merged.0.get("pkg_both").unwrap();
        assert_eq!(
            pkg_both_map
                .get(&PackageSource::Repository(Repository::Aur))
                .unwrap(),
            "pkg_both_aur"
        );
        assert_eq!(
            pkg_both_map
                .get(&PackageSource::Manager(PackageManager::Pacman))
                .unwrap(),
            "pkg_both_pacman"
        );
        assert_eq!(
            pkg_both_map
                .get(&PackageSource::Manager(PackageManager::Apt))
                .unwrap(),
            "pkg_both_apt_overriden"
        );

        let pkg_first_map = merged.0.get("pkg_only_first").unwrap();
        assert_eq!(
            pkg_first_map
                .get(&PackageSource::Manager(PackageManager::Flatpak))
                .unwrap(),
            "pkg_only_first_flatpak"
        );

        let pkg_second_map = merged.0.get("pkg_only_second").unwrap();
        assert_eq!(
            pkg_second_map
                .get(&PackageSource::Manager(PackageManager::Flatpak))
                .unwrap(),
            "pkg_only_second_flatpak"
        );
    }
}
