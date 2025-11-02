use std::io::Write;

use crate::{cli::ListTagsArgs, config::Step, os_info::OS_INFO, parser};
use anyhow::Result;

use super::utils::{check_unique_id, filter_by_os};

pub fn handle(args: ListTagsArgs, out: &mut impl Write) -> Result<()> {
    let steps = parser::parse(&args.file)?;
    check_unique_id(&steps)?;

    let mut steps = steps.iter().collect::<Vec<&Step>>();
    steps = filter_by_os(&steps, &OS_INFO).map(|res| res.matching)?;

    let mut tags: Vec<&str> = steps
        .iter()
        .flat_map(|step| step.tags.iter().map(|t| t.as_str()))
        .collect();
    tags.sort();
    tags.dedup();
    tags.iter().for_each(|t| writeln!(out, "{t}").unwrap());
    Ok(())
}
