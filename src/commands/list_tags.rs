use std::{collections::HashMap, io::Write};

use crate::{cli::ListTagsArgs, parser};
use anyhow::Result;

use super::utils::check_unique_id;

pub fn handle(args: ListTagsArgs, out: &mut impl Write) -> Result<()> {
    let steps = parser::parse(&args.file)?;
    check_unique_id(&steps)?;

    let mut tags_list: HashMap<String, Vec<String>> = HashMap::new();

    for step in steps.iter() {
        step.tags.iter().for_each(|tag| {
            if args.tags.is_empty() || args.tags.contains(tag) {
                tags_list
                    .entry(tag.clone())
                    .or_default()
                    .push(step.id.clone());
            }
        });
    }
    for (tag, steps) in tags_list {
        writeln!(out, "{tag}:")?;
        for step in steps {
            writeln!(out, "- {step}")?;
        }
    }
    Ok(())
}
