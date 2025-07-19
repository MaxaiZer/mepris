use std::io::Write;

use anyhow::Result;

use crate::{
    cli::{ResumeArgs, RunArgs},
    state,
};

use super::{run, utils::RunInfo};

pub fn handle(args: ResumeArgs, out: &mut impl Write) -> Result<()> {
    let state: RunInfo = match state::get() {
        Ok(state) => state,
        Err(_) => {
            writeln!(out, "Nothing to resume. Did you use the run command first?")?;
            return Ok(());
        }
    };
    if state.last_step_id.is_none() {
        writeln!(out, "Nothing to resume: last run was successful")?;
        return Ok(());
    }

    run::handle(
        RunArgs {
            file: state.file,
            tags: state.tags,
            steps: state.steps,
            start_step_id: state.last_step_id,
            dry_run: args.dry_run,
        },
        out,
    )
}
