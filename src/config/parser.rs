use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::Path,
};

use crate::{
    config::{Config, Defaults, Step},
    utils,
};
use anyhow::{Context, Result, bail};
use indexmap::IndexSet;

pub fn parse(file: &str) -> Result<Vec<Step>> {
    let mut visited_files = HashSet::new();
    let mut steps = parse_recursive(file, &mut visited_files, None, None)?;
    expand_templates(&mut steps)?;
    normalize_steps(&mut steps);
    Ok(steps)
}

fn parse_recursive(
    file: &str,
    visited_files: &mut HashSet<String>,
    base_dir: Option<&Path>,
    inherited_defaults: Option<Defaults>,
) -> Result<Vec<Step>> {
    let abs_path = utils::file::get_absolute_path(file, base_dir)
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
    let de = serde_yaml::Deserializer::from_str(&content);
    let config: Config = serde_path_to_error::deserialize(de).map_err(|e| {
        crate::error::YamlParseError(crate::error::yaml_error(
            &abs_path_str,
            &content,
            e.inner(),
            &e.path().to_string(),
        ))
    })?;

    let config_defaults = Defaults::merge(&inherited_defaults, &config.defaults);

    let mut steps = vec![];

    if let Some(includes) = config.includes {
        for include in includes {
            let nested_dir = abs_path.parent().unwrap_or(Path::new("../.."));
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

fn normalize_steps(steps: &mut Vec<Step>) {
    for step in steps {
        if step.provides.is_empty() && !step.packages.is_empty() {
            let unique_packages = step.packages.iter().cloned().collect::<IndexSet<String>>();
            step.provides = unique_packages.iter().cloned().collect();
        }
    }
}

fn expand_templates(steps: &mut [Step]) -> Result<()> {
    if !steps
        .iter()
        .any(|s| s.extends.is_some() || s.with.is_some())
    {
        return Ok(());
    }

    let mut errors = Vec::new();

    // Id uniqueness is required once steps can reference each other by id.
    {
        let mut ids: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        for (idx, step) in steps.iter().enumerate() {
            ids.entry(step.id.as_str()).or_default().push(idx);
        }
        for (id, idxs) in &ids {
            if idxs.len() > 1 {
                let files = idxs
                    .iter()
                    .map(|idx| format!("'{}'", steps[*idx].source_file))
                    .collect::<Vec<_>>()
                    .join(", ");
                errors.push(format!(
                    "duplicate step id '{}': ids must be unique because steps are referenced by 'extends' (steps in {})",
                    id, files
                ));
            }
        }
    }

    // Pairs of (consumer index, cloned extended step) for the fill phase below.
    let mut expansions: Vec<(usize, Step)> = Vec::new();

    for (idx, step) in steps.iter().enumerate() {
        if let Some(target) = &step.extends {
            match steps.iter().position(|s| s.id == *target) {
                None => errors.push(format!(
                    "step '{}' uses 'extends' with id '{}' but no step with that id exists",
                    step.id, target
                )),
                Some(template_idx) => {
                    let template = &steps[template_idx];

                    // A step that itself extends another cannot be used as a template.
                    // This rule also forbids self-extend and all inheritance chains/cycles.
                    if template.extends.is_some() {
                        errors.push(format!(
                            "step '{}' uses 'extends' with id '{}', but step '{}' itself uses 'extends': only steps without 'extends' can be used as templates",
                            step.id, target, template.id
                        ));
                    }

                    let given: BTreeMap<String, String> = step.with.clone().unwrap_or_default();

                    // Every variable from the extended step's 'env' must be satisfied either via
                    // this step's 'with' or declared in this step's own 'env'.
                    let mut unsatisfied = Vec::new();
                    for var in &template.env {
                        if !given.contains_key(var.as_str()) && !step.env.iter().any(|e| e == var) {
                            unsatisfied.push(var.clone());
                        }
                    }
                    if !unsatisfied.is_empty() {
                        errors.push(format!(
                            "step '{}' extends step '{}': the following variables from its 'env' are neither provided via 'with' nor declared in the step's own 'env': {}",
                            step.id,
                            template.id,
                            unsatisfied.join(", ")
                        ));
                    }

                    for key in given.keys() {
                        if !utils::env::is_valid_env_var_name(key) {
                            errors.push(format!(
                                "step '{}' declares invalid variable name '{}' in 'with' (expected [A-Za-z_][A-Za-Z0-9_]*)",
                                step.id, key
                            ));
                        }
                    }

                    let mut conflicts = Vec::new();
                    if step.pre_script.is_some() && template.pre_script.is_some() {
                        conflicts.push("pre_script");
                    }
                    if step.script.is_some() && template.script.is_some() {
                        conflicts.push("script");
                    }
                    if step.when_script.is_some() && template.when_script.is_some() {
                        conflicts.push("when");
                    }
                    if step.check_script.is_some() && template.check_script.is_some() {
                        conflicts.push("check");
                    }
                    if !conflicts.is_empty() {
                        errors.push(format!(
                            "step '{}' defines {} which are also defined in '{}'",
                            step.id,
                            conflicts.join(", "),
                            template.id
                        ));
                    }

                    expansions.push((idx, template.clone()));
                }
            }
        } else if step.with.is_some() {
            errors.push(format!("step '{}' uses 'with' without 'extends'", step.id));
        }
    }

    if !errors.is_empty() {
        bail!("invalid step extends:\n  - {}", errors.join("\n  - "));
    }

    for (idx, template) in expansions {
        let step = &mut steps[idx];
        if step.pre_script.is_none() && template.pre_script.is_some() {
            step.pre_script = template.pre_script.clone();
        }
        if step.script.is_none() && template.script.is_some() {
            step.script = template.script.clone();
        }
        if step.when_script.is_none() && template.when_script.is_some() {
            step.when_script = template.when_script.clone();
        }
        if step.check_script.is_none() && template.check_script.is_some() {
            step.check_script = template.check_script.clone();
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::config::PackageManager;
    use crate::config::expr::os::os_expr_possible_platforms;
    use crate::system::os_info::Platform;

    use super::*;
    use crate::system::shell::Shell;
    use std::fs;
    use tempfile::tempdir;

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
        assert_eq!(
            find_defaults("step_with_override")
                .windows_package_manager
                .unwrap(),
            PackageManager::Choco
        );
        assert_eq!(
            find_defaults("step_with_override").windows_shell.unwrap(),
            Shell::PowerShellCore
        );
        assert_eq!(
            find_defaults("step_with_override").linux_shell.unwrap(),
            Shell::Bash
        );
        assert_eq!(
            find_defaults("step_with_override").macos_shell.unwrap(),
            Shell::Bash
        );
        assert_eq!(
            find_defaults("step_without_override")
                .windows_package_manager
                .unwrap(),
            PackageManager::Scoop
        );
        assert_eq!(
            find_defaults("step_without_override")
                .windows_shell
                .unwrap(),
            Shell::Bash
        );
        assert_eq!(
            find_defaults("step_without_override").linux_shell.unwrap(),
            Shell::PowerShellCore
        );
        assert_eq!(
            find_defaults("step_without_override").macos_shell.unwrap(),
            Shell::PowerShellCore
        );
    }

    #[test]
    fn test_parse_script_full_form() {
        let dir = tempdir().expect("Failed to create temp dir");
        let parent_path = dir.path().join("file.yaml");

        fs::write(
            &parent_path,
            r#"
            defaults:
              windows_shell: bash
            steps:
              - id: "step1"
                script:
                  shell: pwsh
                  run: exit 0
            "#,
        )
        .expect("Failed to write file.yaml");

        let steps = parse(parent_path.to_str().unwrap()).expect("Failed to parse YAML");

        assert_eq!(steps.len(), 1);
        assert_eq!(
            steps[0].script.as_ref().unwrap().shell.as_ref().unwrap(),
            &Shell::PowerShellCore
        );
        assert_eq!(steps[0].script.as_ref().unwrap().code, "exit 0");
    }

    #[test]
    fn test_deduplicates_packages_into_provides() {
        let dir = tempdir().expect("Failed to create temp dir");
        let parent_path = dir.path().join("file.yaml");

        fs::write(
            &parent_path,
            r#"
            steps:
              - id: "step1"
                packages: [pkg1, pkg2, pkg3, pkg2, pkg4]
            "#,
        )
        .expect("Failed to write file.yaml");

        let steps = parse(parent_path.to_str().unwrap()).expect("Failed to parse YAML");

        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].provides.len(), 4);
        assert_eq!(steps[0].provides[0], "pkg1");
        assert_eq!(steps[0].provides[1], "pkg2");
        assert_eq!(steps[0].provides[2], "pkg3");
        assert_eq!(steps[0].provides[3], "pkg4");
    }

    #[test]
    fn test_parse_step_os() {
        let dir = tempdir().expect("Failed to create temp dir");
        let parent_path = dir.path().join("file.yaml");

        fs::write(
            &parent_path,
            r#"
            steps:
              - id: win-step
                os: windows
            "#,
        )
        .expect("Failed to write file.yaml");

        let steps = parse(parent_path.to_str().unwrap()).expect("Failed to parse YAML");

        assert_eq!(steps.len(), 1);
        assert!(steps[0].os.is_some());
        let mut expected = HashSet::new();
        expected.insert(Platform::Windows);
        assert_eq!(
            os_expr_possible_platforms(steps[0].os.as_ref().unwrap()),
            expected
        );
    }

    #[test]
    fn test_parse_require_with_os() {
        let dir = tempdir().expect("Failed to create temp dir");
        let parent_path = dir.path().join("file.yaml");

        fs::write(
            &parent_path,
            r#"
            steps:
              - id: neovim
                requires:
                  - carapace
                  - id: linux-helpers
                    os: linux
            "#,
        )
        .expect("Failed to write file.yaml");

        let steps = parse(parent_path.to_str().unwrap()).expect("Failed to parse YAML");

        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].requires.len(), 2);

        assert_eq!(steps[0].requires[0].id, "carapace");
        assert!(steps[0].requires[0].os.is_none());

        let with_os = &steps[0].requires[1];
        assert_eq!(with_os.id, "linux-helpers");
        let mut expected = HashSet::new();
        expected.insert(Platform::Linux);
        assert_eq!(
            os_expr_possible_platforms(with_os.os.as_ref().unwrap()),
            expected
        );
    }

    #[test]
    fn test_extends_basic_expansion() {
        let dir = tempdir().expect("Failed to create temp dir");
        let path = dir.path().join("file.yaml");

        fs::write(
            &path,
            r#"
            steps:
              - id: run-app
                env: [EXE_PATH, PROCESS_NAME]
                tags: [t1]
                os: windows
                packages: [pkg1]
                requires: [other]
                provides: [prov]
                script: "echo $EXE_PATH"

              - id: consumer
                extends: run-app
                env: [PROCESS_NAME]
                with:
                  EXE_PATH: "/opt/app/devicehost.exe"
            "#,
        )
        .expect("Failed to write file.yaml");

        let steps = parse(path.to_str().unwrap()).expect("Failed to parse YAML");

        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].id, "run-app");
        assert_eq!(steps[1].id, "consumer");

        let consumer = &steps[1];
        assert_eq!(consumer.script.as_ref().unwrap().code, "echo $EXE_PATH");
        assert_eq!(consumer.extends.as_deref(), Some("run-app"));
        assert_eq!(
            consumer.with.as_ref().unwrap().get("EXE_PATH").unwrap(),
            "/opt/app/devicehost.exe"
        );

        // Only the 4 script-related fields are inherited: nothing else leaks over.
        assert_eq!(consumer.env, vec!["PROCESS_NAME"]);
        assert!(consumer.tags.is_empty());
        assert!(consumer.os.is_none());
        assert!(consumer.packages.is_empty());
        assert!(consumer.pre_script.is_none());
        assert!(consumer.when_script.is_none());
        assert!(consumer.check_script.is_none());

        // The referenced step stays in the list with its own fields intact.
        let template = &steps[0];
        assert_eq!(template.env, vec!["EXE_PATH", "PROCESS_NAME"]);
        assert_eq!(template.script.as_ref().unwrap().code, "echo $EXE_PATH");
        assert_eq!(template.tags, vec!["t1"]);
    }

    #[test]
    fn test_extends_per_field_merge() {
        let dir = tempdir().expect("Failed to create temp dir");
        let path = dir.path().join("file.yaml");

        fs::write(
            &path,
            r#"
            steps:
              - id: base
                script: "echo main"
                when: "echo cond"

              - id: consumer
                extends: base
                pre_script: "echo pre"
                check: "echo check"
            "#,
        )
        .expect("Failed to write file.yaml");

        let steps = parse(path.to_str().unwrap()).expect("Failed to parse YAML");

        assert_eq!(steps.len(), 2);

        let consumer = &steps[1];
        assert_eq!(consumer.script.as_ref().unwrap().code, "echo main");
        assert_eq!(consumer.when_script.as_ref().unwrap().code, "echo cond");
        assert_eq!(consumer.pre_script.as_ref().unwrap().code, "echo pre");
        assert_eq!(consumer.check_script.as_ref().unwrap().code, "echo check");

        let base = &steps[0];
        assert_eq!(base.script.as_ref().unwrap().code, "echo main");
        assert_eq!(base.when_script.as_ref().unwrap().code, "echo cond");
    }

    #[test]
    fn test_extends_conflicting_script_error() {
        let dir = tempdir().expect("Failed to create temp dir");
        let path = dir.path().join("file.yaml");

        fs::write(
            &path,
            r#"
            steps:
              - id: base
                script: "echo main"
              - id: consumer
                extends: base
                script: "echo own"
            "#,
        )
        .expect("Failed to write file.yaml");

        let err = parse(path.to_str().unwrap()).err().unwrap();
        assert!(
            err.to_string()
                .contains("step 'consumer' defines script which are also defined in 'base'"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_extends_unknown_target_error() {
        let dir = tempdir().expect("Failed to create temp dir");
        let path = dir.path().join("file.yaml");

        fs::write(
            &path,
            r#"
            steps:
              - id: consumer
                extends: missing
            "#,
        )
        .expect("Failed to write file.yaml");

        let err = parse(path.to_str().unwrap()).err().unwrap();
        assert!(
            err.to_string().contains(
                "step 'consumer' uses 'extends' with id 'missing' but no step with that id exists"
            ),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_extends_chain_error() {
        let dir = tempdir().expect("Failed to create temp dir");
        let path = dir.path().join("file.yaml");

        fs::write(
            &path,
            r#"
            steps:
              - id: a
                script: "echo a"
              - id: b
                extends: a
              - id: c
                extends: b
            "#,
        )
        .expect("Failed to write file.yaml");

        let err = parse(path.to_str().unwrap()).err().unwrap();
        assert!(
            err.to_string()
                .contains("only steps without 'extends' can be used as templates"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_extends_cycle_error() {
        let dir = tempdir().expect("Failed to create temp dir");
        let path = dir.path().join("file.yaml");

        fs::write(
            &path,
            r#"
            steps:
              - id: a
                extends: b
              - id: b
                extends: a
            "#,
        )
        .expect("Failed to write file.yaml");

        let err = parse(path.to_str().unwrap()).err().unwrap();
        assert!(
            err.to_string()
                .contains("only steps without 'extends' can be used as templates"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_extends_self_reference_error() {
        let dir = tempdir().expect("Failed to create temp dir");
        let path = dir.path().join("file.yaml");

        fs::write(
            &path,
            r#"
            steps:
              - id: solo
                extends: solo
            "#,
        )
        .expect("Failed to write file.yaml");

        let err = parse(path.to_str().unwrap()).err().unwrap();
        assert!(
            err.to_string()
                .contains("only steps without 'extends' can be used as templates"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_extends_unsatisfied_env_var_error() {
        let dir = tempdir().expect("Failed to create temp dir");
        let path = dir.path().join("file.yaml");

        fs::write(
            &path,
            r#"
            steps:
              - id: base
                env: [A, B]
                script: "echo $A $B"

              - id: consumer
                extends: base
                with:
                  A: foo
            "#,
        )
        .expect("Failed to write file.yaml");

        let err = parse(path.to_str().unwrap()).err().unwrap();
        assert!(
            err.to_string().contains(
                "step 'consumer' extends step 'base': the following variables from its 'env' are neither provided via 'with' nor declared in the step's own 'env': B"
            ),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_extends_extra_with_key_ok() {
        let dir = tempdir().expect("Failed to create temp dir");
        let path = dir.path().join("file.yaml");

        fs::write(
            &path,
            r#"
            steps:
              - id: base
                env: [A]
                script: "echo $A"

              - id: consumer
                extends: base
                with:
                  A: foo
                  EXTRA: bar
            "#,
        )
        .expect("Failed to write file.yaml");

        let steps = parse(path.to_str().unwrap()).expect("Failed to parse YAML");

        assert_eq!(steps.len(), 2);
        let with = steps[1].with.as_ref().unwrap();
        assert_eq!(with.get("A").unwrap(), "foo");
        assert_eq!(with.get("EXTRA").unwrap(), "bar");
    }

    #[test]
    fn test_extends_invalid_with_key_name_error() {
        let dir = tempdir().expect("Failed to create temp dir");
        let path = dir.path().join("file.yaml");

        fs::write(
            &path,
            r#"
            steps:
              - id: base
                script: "echo x"

              - id: consumer
                extends: base
                with:
                  1BAD: foo
            "#,
        )
        .expect("Failed to write file.yaml");

        let err = parse(path.to_str().unwrap()).err().unwrap();
        assert!(
            err.to_string()
                .contains("step 'consumer' declares invalid variable name '1BAD' in 'with'"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_with_without_extends_error() {
        let dir = tempdir().expect("Failed to create temp dir");
        let path = dir.path().join("file.yaml");

        fs::write(
            &path,
            r#"
            steps:
              - id: s1
                with:
                  A: foo
            "#,
        )
        .expect("Failed to write file.yaml");

        let err = parse(path.to_str().unwrap()).err().unwrap();
        assert!(
            err.to_string()
                .contains("step 's1' uses 'with' without 'extends'"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_duplicate_id_error_when_extends_used() {
        let dir = tempdir().expect("Failed to create temp dir");
        let path = dir.path().join("file.yaml");

        fs::write(
            &path,
            r#"
            steps:
              - id: dup
                script: "echo a"
              - id: tpl
              - id: dup
                extends: tpl
            "#,
        )
        .expect("Failed to write file.yaml");

        let err = parse(path.to_str().unwrap()).err().unwrap();
        assert!(
            err.to_string().contains("duplicate step id 'dup'"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_extends_target_defined_in_include() {
        let dir = tempdir().expect("Failed to create temp dir");

        let parent_path = dir.path().join("parent.yaml");
        let child_path = dir.path().join("child.yaml");

        fs::write(
            &parent_path,
            r#"
            includes:
              - child.yaml
            steps:
              - id: consumer
                extends: base
                with:
                  A: foo
            "#,
        )
        .expect("Failed to write parent.yaml");

        fs::write(
            &child_path,
            r#"
            steps:
              - id: base
                env: [A]
                script: "echo from-include"
            "#,
        )
        .expect("Failed to write child.yaml");

        let steps = parse(parent_path.to_str().unwrap()).expect("Failed to parse YAML");

        assert_eq!(steps.len(), 2);
        let consumer = steps.iter().find(|s| s.id == "consumer").unwrap();
        assert_eq!(consumer.script.as_ref().unwrap().code, "echo from-include");
    }
}
