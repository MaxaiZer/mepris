use std::io::Write;

use anyhow::Result;
use commands::{completion, list_steps, list_tags, resume, run};

pub mod cli;
pub mod commands;
mod config;
mod helpers;
mod os_info;
mod parser;
mod runner;
pub mod shell;
mod state;

pub fn run(cli: crate::cli::Cli, out: &mut impl Write) -> Result<()> {
    match cli.command {
        cli::Commands::Run(args) => run::handle(args, out)?,
        cli::Commands::Resume(args) => resume::handle(args, out)?,
        cli::Commands::ListSteps(args) => list_steps::handle(args, out)?,
        cli::Commands::ListTags(args) => list_tags::handle(args, out)?,
        cli::Commands::Completion(args) => completion::handle(args, out)?,
    }
    Ok(())
}
