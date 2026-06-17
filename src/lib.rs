use std::io::Write;

use anyhow::Result;
use commands::{completion, list_steps, list_tags, resume, run};

pub mod cli;
pub mod commands;
pub mod config;
pub mod runner;
pub mod system;
mod utils;
pub use utils::test::{EnvGuard, run_with_cwd};

mod logging;
use crate::cli::Normalize;
use crate::commands::validate;
pub use logging::setup_tracing;
pub use logging::test::run_with_tracing;

pub fn run(cli: crate::cli::Cli, out: &mut impl Write) -> Result<()> {
    match cli.command {
        cli::Commands::Run(args) => run::handle(args.normalize()?, out)?,
        cli::Commands::Resume(args) => resume::handle(args, out)?,
        cli::Commands::Validate(args) => validate::handle(args.normalize()?, out)?,
        cli::Commands::ListSteps(args) => list_steps::handle(args.normalize()?, out)?,
        cli::Commands::ListTags(args) => list_tags::handle(args.normalize()?, out)?,
        cli::Commands::Completion(args) => completion::handle(args, out)?,
    }
    Ok(())
}
