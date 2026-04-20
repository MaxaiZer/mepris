pub mod filters;
pub mod sort;

use crate::runner::state;
use crate::{
    config::Step,
    runner::{self, RunState},
};
use anyhow::{Context, Result, bail};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

pub struct RunStateSaver {
    pub file: String,
    pub tags_expr: Option<String>,
    pub steps: Vec<String>,
}

impl runner::StateSaver for RunStateSaver {
    fn save(&self, state: &RunState) -> anyhow::Result<()> {
        state::save(&RunInfo {
            file: self.file.clone(),
            tags_expr: self.tags_expr.clone(),
            steps: self.steps.clone(),
            interactive: state.interactive,
            last_step_id: state.last_step_id.clone(),
        })
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct RunInfo {
    pub file: String,
    pub tags_expr: Option<String>,
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
            let steps_str = if steps.len() > 1 { "steps" } else { "step" };
            msg.push_str(&format!(
                "\n{} (required by {steps_str} {})",
                env,
                steps.join(", ")
            ));
        });
        bail!(msg);
    }

    Ok(())
}

pub fn check_step_env(step: &Step) -> Result<()> {
    let mut missing: HashMap<String, Vec<String>> = HashMap::new();

    for env in step.env.iter() {
        if std::env::var(env).is_err() {
            missing
                .entry(env.clone())
                .or_default()
                .push(step.id.clone());
        }
    }

    if !missing.is_empty() {
        let mut msg = String::from("Undefined environment variables:");
        missing.iter().for_each(|(env, steps)| {
            let steps_str = if steps.len() > 1 { "steps" } else { "step" };
            msg.push_str(&format!(
                "\n{} (required by {steps_str} {})",
                env,
                steps.join(", ")
            ));
        });
        bail!(msg);
    }

    Ok(())
}

pub fn check_tags_exist(steps: &[&Step], tags_to_check: &[String]) -> Result<()> {
    let all_tags: HashSet<_> = steps.iter().flat_map(|s| &s.tags).collect();

    let unknown_tags: Vec<_> = tags_to_check
        .iter()
        .filter(|tag| !all_tags.contains(tag))
        .map(|tag| tag.as_str())
        .collect();

    if !unknown_tags.is_empty() {
        bail!("Unknown tags: {}", unknown_tags.join(", "));
    }
    Ok(())
}
