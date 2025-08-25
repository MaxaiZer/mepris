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

    let interactive = args.interactive || state.interactive;

    run::handle(
        RunArgs {
            file: state.file,
            tags_expr: state.tags_expr,
            steps: state.steps,
            start_step_id: state.last_step_id,
            interactive,
            dry_run: args.dry_run,
        },
        out,
    )
}
