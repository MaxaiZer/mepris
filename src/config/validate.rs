use crate::config::Step;
use anyhow::bail;
use std::collections::HashMap;
use std::path::Path;

pub fn validate(steps: &[Step]) -> anyhow::Result<()> {
    check_unique_id(steps)?;
    Ok(())
}

fn check_unique_id(steps: &[Step]) -> anyhow::Result<()> {
    let mut steps_id: HashMap<String, usize> = HashMap::new();

    for (idx, step) in steps.iter().enumerate() {
        if !steps_id.contains_key(&step.id) {
            steps_id.insert(step.id.clone(), idx);
            continue;
        }

        let duplicate_idx = steps_id.get(&step.id).unwrap();
        let duplicate = &steps[*duplicate_idx];

        if step.source_file == duplicate.source_file {
            bail!(
                "Duplicate step '{}' in file '{}'",
                step.id,
                get_file_name(&step.source_file)
            );
        }
        bail!(
            "Duplicate step '{}' in files '{}' and '{}'",
            step.id,
            get_file_name(&step.source_file),
            get_file_name(&duplicate.source_file)
        );
    }
    Ok(())
}

fn get_file_name(full_path: &str) -> &str {
    Path::new(full_path)
        .file_name()
        .and_then(|os_str| os_str.to_str())
        .unwrap_or(full_path)
}
