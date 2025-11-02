use crate::cli::CompletionArgs;
use anyhow::{bail, Result};
use clap::CommandFactory;
use clap_complete::{
    Shell,
    aot::{Bash, Elvish, Fish, PowerShell, Zsh},
    generate,
};
use std::io::Write;

pub fn handle(args: CompletionArgs, out: &mut impl Write) -> Result<()> {
    let mut cmd = crate::cli::Cli::command();
    match args.shell {
        Shell::Bash => generate(Bash, &mut cmd, "mepris", out),
        Shell::Zsh => generate(Zsh, &mut cmd, "mepris", out),
        Shell::Fish => {
            generate(Fish, &mut cmd, "mepris", out);
            writeln!(out, "{}", include_str!("../completions/custom.fish"))?;
        }
        Shell::Elvish => generate(Elvish, &mut cmd, "mepris", out),
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
        _ => bail!("Unsupported shell: {:?}", args.shell),
    }
    Ok(())
}
