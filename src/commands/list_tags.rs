use std::io::Write;

use crate::commands::utils::filters::filter_by_os;
use crate::{cli::ListTagsArgs, config, config::Step, system::os_info::OS_INFO};
use anyhow::Result;

pub fn handle(args: ListTagsArgs, out: &mut impl Write) -> Result<()> {
    let steps = config::load_steps(&args.file)?;

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
