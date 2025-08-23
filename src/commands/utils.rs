use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use crate::{
    config::{Step, eval_expr},
    os_info::OsInfo,
    runner::{self, RunState},
    state,
};
use anyhow::{Context, Result, bail};

pub struct RunStateSaver {
    pub file: String,
    pub tags: Vec<String>,
    pub steps: Vec<String>,
}

impl runner::StateSaver for RunStateSaver {
    fn save(&self, state: &RunState) -> anyhow::Result<()> {
        state::save(&RunInfo {
            file: self.file.clone(),
            tags: self.tags.clone(),
            steps: self.steps.clone(),
            interactive: state.interactive,
            last_step_id: state.last_step_id.clone(),
        })
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct RunInfo {
    pub file: String,
    pub tags: Vec<String>,
    pub steps: Vec<String>,
    pub interactive: bool,
    pub last_step_id: Option<String>,
}

pub fn load_env(config_file_path: &str) -> Result<()> {
    let env_path = Path::new(config_file_path).parent().unwrap().join(".env");
    if let Ok(false) = std::fs::exists(&env_path) {
        return Ok(());
    }
    dotenvy::from_filename_override(env_path)
        .map(|_| Ok(()))
        .context("Failed to load .env file")?
}

pub fn check_env(steps: &[&Step]) -> Result<()> {
    let mut missing: HashMap<String, Vec<String>> = HashMap::new();

    for step in steps {
        for env in step.env.iter() {
            if std::env::var(env).is_err() {
                missing
                    .entry(env.clone())
                    .or_default()
                    .push(step.id.clone());
            }
        }
    }

    if !missing.is_empty() {
        let mut msg = String::from("Undefined environment variables:");
        missing.iter().for_each(|(env, steps)| {
            msg.push_str(&format!(
                "\n{} (required by steps {})",
                env,
                steps.join(", ")
            ));
        });
        bail!(msg);
    }

    Ok(())
}

pub fn check_unique_id(steps: &[Step]) -> Result<()> {
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

pub fn check_steps_exist(steps: &[&Step], steps_id_to_check: &[String]) -> Result<()> {
    let available_ids: HashSet<_> = steps.iter().map(|s| &s.id).collect();

    let unknown_steps: Vec<_> = steps_id_to_check
        .iter()
        .filter(|id| !available_ids.contains(id))
        .map(|id| id.as_str())
        .collect();

    if !unknown_steps.is_empty() {
        anyhow::bail!("Unknown steps: {}", unknown_steps.join(", "));
    }
    Ok(())
}

pub struct FilterResult<'a> {
    pub matching: Vec<&'a Step>,
    pub not_matching: Vec<&'a Step>,
}

pub fn filter_by_tags<'a>(steps: &[&'a Step], tags: &[String]) -> Result<FilterResult<'a>> {
    check_tags_exist(steps, tags)?;

    let (matching, not_matching): (Vec<_>, Vec<_>) = steps
        .iter()
        .partition(|s| s.tags.iter().any(|tag| tags.contains(tag)));
    Ok(FilterResult {
        matching,
        not_matching,
    })
}

pub fn filter_steps_start_with_id<'a>(
    steps: &[&'a Step],
    start_step_id: &str,
) -> Result<FilterResult<'a>> {
    if let Some(pos) = steps.iter().position(|s| s.id == start_step_id) {
        let (not_matching, matching) = steps.split_at(pos);

        Ok(FilterResult {
            matching: matching.to_vec(),
            not_matching: not_matching.to_vec(),
        })
    } else {
        bail!("Start step '{start_step_id}' not found in file");
    }
}

pub fn filter_by_os<'a>(steps: &[&'a Step], os_info: &OsInfo) -> Result<FilterResult<'a>> {
    let (matching, not_matching): (Vec<_>, Vec<_>) = steps.iter().partition(|s| {
        if s.os.is_none() {
            return true;
        }
        if let Some(os_expr) = &s.os
            && eval_expr(os_expr, os_info)
        {
            return true;
        }
        false
    });
    Ok(FilterResult {
        matching,
        not_matching,
    })
}

pub fn check_tags_exist(steps: &[&Step], tags_to_check: &[String]) -> Result<()> {
    let all_tags: HashSet<_> = steps.iter().flat_map(|s| &s.tags).collect();

    let unknown_tags: Vec<_> = tags_to_check
        .iter()
        .filter(|tag| !all_tags.contains(tag))
        .map(|tag| tag.as_str())
        .collect();

    if !unknown_tags.is_empty() {
        anyhow::bail!("Unknown tags: {}", unknown_tags.join(", "));
    }
    Ok(())
}

fn get_file_name(full_path: &str) -> &str {
    Path::new(full_path)
        .file_name()
        .and_then(|os_str| os_str.to_str())
        .unwrap_or(full_path)
}
