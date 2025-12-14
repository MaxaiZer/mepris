use std::io::{self, Write};

use crate::config::{PackageSource, Step};

use super::logger::Logger;
use anyhow::{bail, Result};
use colored::Colorize;
use crate::runner::pkg::check_pkg_installed;

pub enum Decision {
    Run,
    Skip,
    Abort,
    LeaveInteractiveMode,
}

const MAX_SCRIPT_LINES: usize = 8;

pub fn ask_confirmation(step: &Step, logger: &mut Logger<impl Write>) -> Result<Decision> {
    let mut cmds = vec!["r=Run", "s=Skip", "a=Abort", "l=Leave interactive mode"];
    if need_truncate_step_output(step) {
        cmds.push("v=View full step");
    }
    let letters: Vec<String> = cmds
        .iter()
        .map(|s| s.split('=').next().unwrap().to_string())
        .collect();

    print_step(step, logger.out, false)?;
    logger.log(&format!(
        "\nPROGRESS What do you want to do? ({}): ",
        cmds.join(", ")
    ))?;

    loop {
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();

        input = input.trim().to_lowercase();
        if !letters.contains(&input) {
            logger.log("Invalid input, please try again.")?;
            continue;
        }

        match input.as_str() {
            "r" => return Ok(Decision::Run),
            "s" => return Ok(Decision::Skip),
            "a" => return Ok(Decision::Abort),
            "l" => return Ok(Decision::LeaveInteractiveMode),
            "v" => {
                print_step(step, logger.out, true)?;
                logger.log(&format!(
                    "\nPROGRESS What do you want to do? ({}): ",
                    cmds.join(", ")
                ))?;
            }
            _ => logger.log("Invalid input, please try again.")?,
        }
    }
}
fn need_truncate_step_output(step: &Step) -> bool {
    fn is_too_long(code: &str) -> bool {
        code.lines().nth(MAX_SCRIPT_LINES).is_some()
    }

    step.pre_script
        .as_ref()
        .is_some_and(|s| is_too_long(&s.code))
        || step.script.as_ref().is_some_and(|s| is_too_long(&s.code))
}

fn print_step(step: &Step, out: &mut impl Write, full: bool) -> Result<()> {

    let pkg_manager = match &step.package_source {
        Some(PackageSource::Manager(pm)) => pm.clone(),
        _ => bail!("Package manager is not resolved"),
    };

    writeln!(out, "step {}", step.id.cyan())?;
    let max_script_lines = match full {
        true => usize::MAX,
        false => MAX_SCRIPT_LINES,
    };

    if let Some(pre_script) = &step.pre_script {
        writeln!(out, "pre_script:")?;
        output_script(&pre_script.code, max_script_lines, out)?;
    }
    if !step.packages.is_empty() {
        let installed: Vec<&str> = step.packages.iter().map(|s| s.as_str()).filter(|p| check_pkg_installed(&pkg_manager, p).unwrap_or(false)).collect();
        let not_installed: Vec<&str> = step.packages.iter().map(|s| s.as_str()).filter(|p| !check_pkg_installed(&pkg_manager, p).unwrap_or(false)).collect();

        writeln!(out, "packages ({}):", pkg_manager.command().to_string())?;
        if !installed.is_empty()
        {
            writeln!(out, "  {}: {}", "already installed".green(), installed.join(", "))?;
        }
        if !not_installed.is_empty()
        {
            writeln!(out, "  {}: {}", "would install".yellow(), not_installed.join(", "))?;
        }
    }
    if let Some(script) = &step.script {
        writeln!(out, "script:")?;
        output_script(&script.code, max_script_lines, out)?;
    }
    Ok(())
}

fn output_script(script: &str, max_lines: usize, out: &mut impl Write) -> Result<()> {
    let mut iter = script.lines();
    for line in iter.by_ref().take(max_lines) {
        writeln!(out, "{}", line.magenta())?;
    }

    if iter.next().is_some() {
        writeln!(out, "...")?;
    }
    Ok(())
}
