use std::io::Write;

use super::utils::check_unique_id;
use crate::commands::utils::filters::filter_by_os;
use crate::config::parser;
use crate::{cli::ListTagsArgs, config::Step, system::os_info::OS_INFO};
use anyhow::Result;

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
