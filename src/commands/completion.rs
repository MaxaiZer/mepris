use crate::cli::{CompletionArgs, Shell};
use anyhow::Result;
use clap::CommandFactory;
use clap_complete::{
    aot::{Bash, Fish, PowerShell, Zsh},
    generate,
};
use std::io::Write;

pub fn handle(args: CompletionArgs, out: &mut impl Write) -> Result<()> {
    let mut cmd = crate::cli::Cli::command();

    match args.shell {
        Shell::Bash => {
            let mut buf = Vec::new();
            generate(Bash, &mut cmd, "mepris", &mut buf);

            let mut generated = String::from_utf8(buf).expect("invalid utf-8");

            generated = generated.replace(
                "case \"${prev}\" in",
                &format!(
                    "{}\ncase \"${{prev}}\" in",
                    include_str!("../completions/custom.bash"),
                ),
            );
            writeln!(out, "{generated}")?;
        }
        Shell::Zsh => {
            let mut buf = Vec::new();
            generate(Zsh, &mut cmd, "mepris", &mut buf);

            let mut generated = String::from_utf8(buf).expect("invalid utf-8");
            generated = generated.replace("STEPS:_default", "STEPS:_mepris_complete_steps");
            generated = generated.replace("TAGS_EXPR:_default", "TAGS_EXPR:_mepris_complete_tags");
            
            writeln!(out, "{}", include_str!("../completions/custom.zsh"))?;
            writeln!(out, "{generated}")?;
        }
        Shell::Fish => {
            generate(Fish, &mut cmd, "mepris", out);
            writeln!(out, "{}", include_str!("../completions/custom.fish"))?;
        }
        Shell::PowerShell => {
            let mut buf = Vec::new();
            generate(PowerShell, &mut cmd, "mepris", &mut buf);

            let mut generated = String::from_utf8(buf).expect("invalid utf-8");
            generated = generated.replace(
                "Register-ArgumentCompleter -Native -CommandName 'mepris' -ScriptBlock",
                "$oldCompleter =",
            );

            writeln!(out, "{generated}")?;
            writeln!(out, "{}", include_str!("../completions/custom.ps1"))?;
        }
        Shell::Nushell => {
            let mut buf = Vec::new();
            generate(clap_complete_nushell::Nushell, &mut cmd, "mepris", &mut buf);

            let mut generated = String::from_utf8(buf).expect("invalid utf-8");
            generated = generated.replace("--file(-f): string", "--file(-f): path");
            generated = generated.replace(
                "--tag(-t): string",
                "--tag(-t): string@\"nu-complete mepris tags\"",
            );
            generated = generated.replace(
                "--step(-s): string",
                "--step(-s): string@\"nu-complete mepris steps\"",
            );

            writeln!(out, "{generated}")?;
            writeln!(out, "{}", include_str!("../completions/custom.nu"))?;
        }
    }

    Ok(())
}
