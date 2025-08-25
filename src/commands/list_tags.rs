use std::{collections::HashMap, io::Write};

use crate::{cli::ListTagsArgs, config::Step, os_info::OS_INFO, parser};
use anyhow::Result;

use super::utils::{check_tags_exist, check_unique_id, filter_by_os};

pub fn handle(args: ListTagsArgs, out: &mut impl Write) -> Result<()> {
    let steps = parser::parse(&args.file)?;
    check_unique_id(&steps)?;

    let mut steps = steps.iter().collect::<Vec<&Step>>();
    if !args.all {
        steps = filter_by_os(&steps, &OS_INFO).map(|res| res.matching)?;
    }
    check_tags_exist(&steps, &args.tags)?;

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
