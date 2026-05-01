use std::io;

use clap::Parser;
use colored::Colorize;
use mepris::{cli, setup_tracing, system::shell};

fn main() {
    shell::detect_shells();
    let cli = cli::Cli::parse();
    let mut out = io::stdout();

    let debug = match &cli.command {
        cli::Commands::Run(args) => args.debug,
        cli::Commands::Resume(args) => args.debug,
        _ => false,
    };
    setup_tracing(debug);

    match mepris::run(cli, &mut out) {
        Ok(_) => {}
        Err(e) => {
            eprintln!("{} {:#}", "Error:".red().bold(), e);
            std::process::exit(1);
        }
    }
}
