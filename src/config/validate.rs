use crate::config::Step;
use anyhow::bail;
use std::cmp::PartialEq;
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(PartialEq)]
pub enum ValidationMode {
    Minimal,
    IdIntegrity,
    Full,
}

pub fn validate(steps: &[Step], mode: ValidationMode) -> anyhow::Result<()> {
    if mode == ValidationMode::Minimal {
        return Ok(());
    }

    let mut errors: Vec<String> = Vec::new();
    check_unique_id(steps, &mut errors);
    if mode == ValidationMode::Full {
        check_provides_requires(steps, &mut errors);
    }

    if !errors.is_empty() {
        bail!("validation failed:\n{}", errors.join("\n"));
    }

    Ok(())
}
fn check_unique_id(steps: &[Step], errors: &mut Vec<String>) {
    let mut steps_id: HashMap<String, usize> = HashMap::new();

    for (idx, step) in steps.iter().enumerate() {
        if let Some(&duplicate_idx) = steps_id.get(&step.id) {
            let duplicate = &steps[duplicate_idx];

            if step.source_file == duplicate.source_file {
                errors.push(format!(
                    "duplicate step '{}' in file '{}'",
                    step.id,
                    get_file_name(&step.source_file)
                ));
            } else {
                errors.push(format!(
                    "duplicate step '{}' in files '{}' and '{}'",
                    step.id,
                    get_file_name(&step.source_file),
                    get_file_name(&duplicate.source_file)
                ));
            }

            continue;
        }

        steps_id.insert(step.id.clone(), idx);
    }
}

fn check_provides_requires(steps: &[Step], errors: &mut Vec<String>) {
    let available_provides: HashMap<String, Vec<String>> = steps
        .iter()
        .flat_map(|s| s.provides.iter().map(move |p| (p.clone(), s.id.clone())))
        .fold(HashMap::new(), |mut acc, (provide, step_id)| {
            acc.entry(provide).or_default().push(step_id);
            acc
        });

    for step in steps {
        let mut seen_provides = HashSet::new();
        for provide in &step.provides {
            if !seen_provides.insert(provide.clone()) {
                errors.push(format!(
                    "step '{}': duplicated provide '{}'",
                    step.id, provide
                ));
            }
        }

        let mut seen_requires = HashSet::new();
        let mut unknown_requires = Vec::new();

        for require in &step.requires {
            let req = &require.id;

            if !seen_requires.insert(req.clone()) {
                errors.push(format!(
                    "step '{}': duplicated require '{}'",
                    step.id, req
                ));
            }

            if seen_provides.contains(req) {
                errors.push(format!(
                    "step '{}': self-reference on '{}'",
                    step.id, req
                ));
            }

            if !available_provides.contains_key(req) {
                unknown_requires.push(req.clone());
            }
        }

        if !unknown_requires.is_empty() {
            errors.push(format!(
                "step '{}': unknown requirements: {}",
                step.id,
                unknown_requires.join(", ")
            ));
        }
    }
}

fn get_file_name(full_path: &str) -> &str {
    Path::new(full_path)
        .file_name()
        .and_then(|os_str| os_str.to_str())
        .unwrap_or(full_path)
}

#[cfg(test)]
mod tests {
    use crate::config::validate::validate;
    use crate::config::{Require, Step, ValidationMode};

    fn make_step(id: &str, requires: Vec<&str>, provides: Vec<&str>) -> Step {
        Step {
            id: id.to_string(),
            requires: requires
                .into_iter()
                .map(|s| Require {
                    id: s.to_string().clone(),
                    os: None,
                    when_script: None,
                })
                .collect(),
            provides: provides.into_iter().map(String::from).collect(),
            ..Default::default()
        }
    }
    #[test]
    fn test_doubled_provide() {
        let step1 = make_step("step1", vec![], vec!["postgres", "postgres"]);
        let steps = vec![step1];
        let result = validate(&steps, ValidationMode::Full);

        assert!(result.is_err());
        let err_str = result.unwrap_err().to_string();
        assert!(
            err_str.contains("step 'step1': duplicated provide 'postgres'"),
            "unexpected err: {}",
            err_str
        );

        let result = validate(&steps, ValidationMode::IdIntegrity);
        assert!(result.is_ok());
    }

    #[test]
    fn test_doubled_require() {
        let steps = vec![make_step("step1", vec!["postgres", "postgres"], vec![])];
        let result = validate(&steps, ValidationMode::Full);

        assert!(result.is_err());
        let err_str = result.unwrap_err().to_string();
        assert!(
            err_str.contains("step 'step1': duplicated require 'postgres'"),
            "unexpected err: {}",
            err_str
        );
    }

    #[test]
    fn test_self_reference() {
        let steps = vec![make_step(
            "step1",
            vec!["step1_completed"],
            vec!["step1_completed"],
        )];
        let result = validate(&steps, ValidationMode::Full);

        assert!(result.is_err());
        let err_str = result.unwrap_err().to_string();
        assert!(
            err_str.contains("self-reference"),
            "unexpected err: {}",
            err_str
        );

        let result = validate(&steps, ValidationMode::IdIntegrity);
        assert!(result.is_ok());
    }

    #[test]
    fn test_unknown_requires() {
        let steps = vec![
            make_step("step1", vec![], vec!["step1_completed"]),
            make_step("step2", vec![], vec![]),
            make_step("step3", vec!["step2_completed"], vec![]),
        ];
        let result = validate(&steps, ValidationMode::Full);

        assert!(result.is_err());
        let err_str = result.unwrap_err().to_string();
        assert!(
            err_str.contains("unknown requirements"),
            "unexpected err: {}",
            err_str
        );

        let result = validate(&steps, ValidationMode::IdIntegrity);
        assert!(result.is_ok());
    }
}
