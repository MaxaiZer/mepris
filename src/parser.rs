use std::{collections::HashSet, fs, path::Path};

use crate::{
    config::{Config, Defaults, Step},
    helpers,
};
use anyhow::{Context, Result};

pub fn parse(file: &str) -> Result<Vec<Step>> {
    let mut visited_files = HashSet::new();
    parse_recursive(file, &mut visited_files, None, None)
}

fn parse_recursive(
    file: &str,
    visited_files: &mut HashSet<String>,
    base_dir: Option<&Path>,
    inherited_defaults: Option<Defaults>,
) -> Result<Vec<Step>> {
    let abs_path = helpers::get_absolute_path(file, base_dir)
        .with_context(|| format!("Failed to resolve absolute path for '{file}'"))?;
    let abs_path_str = abs_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Invalid UTF-8 in path: {:?}", abs_path))?
        .to_string();

    if visited_files.contains(&abs_path_str) {
        return Ok(vec![]);
    }

    visited_files.insert(abs_path_str.clone());

    let content = fs::read_to_string(&abs_path)
        .with_context(|| format!("Failed to read file '{abs_path_str}'"))?;
    let config: Config = serde_yaml::from_str(&content)
        .with_context(|| format!("YAML parse error in file '{abs_path_str}'"))?;

    let config_defaults = Defaults::merge(&inherited_defaults, &config.defaults);

    let mut steps = vec![];

    if let Some(includes) = config.includes {
        for include in includes {
            let nested_dir = abs_path.parent().unwrap_or(Path::new("."));
            let nested = parse_recursive(
                &include,
                visited_files,
                Some(nested_dir),
                Some(config_defaults.clone()),
            )
            .with_context(|| format!("Failed to parse included file '{include}'"))?;
            steps.extend(nested);
        }
    }

    if let Some(mut own_steps) = config.steps {
        for own_step in &mut own_steps {
            own_step.source_file.clone_from(&abs_path_str);
            own_step.defaults = Some(config_defaults.clone());
        }
        steps.extend(own_steps);
    }

    Ok(steps)
}

#[cfg(test)]
mod tests {
    use crate::config::PackageManager;

    use super::*;
    use std::fs;
    use tempfile::tempdir;
    use crate::shell::Shell;

    #[test]
    fn test_parse_with_relative_include() {
        let dir = tempdir().expect("Failed to create temp dir");

        let parent_path = dir.path().join("parent.yaml");
        let child_path = dir.path().join("child.yaml");

        fs::write(
            &parent_path,
            r#"
            includes:
              - child.yaml
            steps:
              - id: "step1"
            "#,
        )
        .expect("Failed to write parent.yaml");

        fs::write(
            &child_path,
            r#"
            steps:
              - id: "step2"
            "#,
        )
        .expect("Failed to write child.yaml");

        let steps = parse(parent_path.to_str().unwrap()).expect("Failed to parse YAML");

        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].id, "step2");
        assert_eq!(steps[1].id, "step1");

        assert!(
            steps[0].source_file.ends_with("child.yaml"),
            "Expected child.yaml, got {}",
            steps[0].source_file
        );
        assert!(
            steps[1].source_file.ends_with("parent.yaml"),
            "Expected parent.yaml, got {}",
            steps[1].source_file
        );
    }

    #[test]
    fn test_parse_with_relative_include_folder() {
        let dir = tempdir().expect("Failed to create temp dir");
        fs::create_dir(dir.path().join("tmp")).expect("Failed to create child dir");

        let parent_path = dir.path().join("parent.yaml");
        let child_path = dir.path().join("tmp/child.yaml");

        fs::write(
            &parent_path,
            r#"
            includes:
              - tmp/child.yaml
            steps:
              - id: "step1"
            "#,
        )
        .expect("Failed to write parent.yaml");

        fs::write(
            &child_path,
            r#"
            steps:
              - id: "step2"
            "#,
        )
        .expect("Failed to write child.yaml");

        let steps = parse(parent_path.to_str().unwrap()).expect("Failed to parse YAML");

        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].id, "step2");
        assert_eq!(steps[1].id, "step1");

        assert!(
            steps[0].source_file.ends_with("tmp/child.yaml"),
            "Expected tmp/child.yaml, got {}",
            steps[0].source_file
        );
        assert!(
            steps[1].source_file.ends_with("parent.yaml"),
            "Expected parent.yaml, got {}",
            steps[1].source_file
        );
    }

    #[test]
    fn test_parse_defaults_overrides() {
        let dir = tempdir().expect("Failed to create temp dir");
        let parent_path = dir.path().join("parent.yaml");

        fs::write(
            &parent_path,
            r#"
            includes:
              - child_with_override.yaml
              - child_without_override.yaml
            defaults:
              windows_package_manager: scoop
              windows_shell: bash
              linux_shell: pwsh
              macos_shell: pwsh
            steps:
              - id: "step1"
            "#,
        )
        .expect("Failed to write parent.yaml");

        fs::write(
            dir.path().join("child_with_override.yaml"),
            r#"
            defaults:
              windows_package_manager: choco
              windows_shell: pwsh
              linux_shell: bash
              macos_shell: bash
            steps:
              - id: "step_with_override"
            "#,
        )
        .expect("Failed to write child_with_override.yaml");

        fs::write(
            dir.path().join("child_without_override.yaml"),
            r#"
            steps:
              - id: "step_without_override"
            "#,
        )
        .expect("Failed to write child_without_override.yaml");

        let steps = parse(parent_path.to_str().unwrap()).expect("Failed to parse YAML");

        let find_defaults = |step_id: &str| -> Defaults {
            steps
                .iter()
                .find(|s| s.id == step_id)
                .unwrap()
                .defaults
                .clone()
                .unwrap()
        };

        assert_eq!(steps.len(), 3);
        assert_eq!(find_defaults("step_with_override").windows_package_manager.unwrap(), PackageManager::Choco);
        assert_eq!(find_defaults("step_with_override").windows_shell.unwrap(), Shell::PowerShellCore);
        assert_eq!(find_defaults("step_with_override").linux_shell.unwrap(), Shell::Bash);
        assert_eq!(find_defaults("step_with_override").macos_shell.unwrap(), Shell::Bash);
        assert_eq!(find_defaults("step_without_override").windows_package_manager.unwrap(), PackageManager::Scoop);
        assert_eq!(find_defaults("step_without_override").windows_shell.unwrap(), Shell::Bash);
        assert_eq!(find_defaults("step_without_override").linux_shell.unwrap(), Shell::PowerShellCore);
        assert_eq!(find_defaults("step_without_override").macos_shell.unwrap(), Shell::PowerShellCore);
    }
}
