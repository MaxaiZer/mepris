use std::io;

use clap::Parser;
use colored::Colorize;
use mepris::{cli, system::shell};

fn main() {
    shell::detect_shells();
    let cli = cli::Cli::parse();
    let mut out = io::stdout();
    match mepris::run(cli, &mut out) {
        Ok(_) => {}
        Err(e) => {
            eprintln!("{} {:#}", "Error:".red().bold(), e);
            std::process::exit(1);
        }
    }
}
