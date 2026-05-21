use crate::cli::ValidateArgs;
use crate::commands::utils::filters::filter_steps;
use crate::config::aliases::load_aliases;
use crate::config::{StepSelectionReason, ValidationMode};
use crate::logging::EventType;
use crate::runner::Script;
use crate::runner::script_checker::{DefaultScriptChecker, ScriptChecker};
use crate::{
    config,
    config::Step,
    runner::{self},
    system::os_info::OS_INFO,
    utils,
};
use anyhow::{Context, Result, bail};
use std::io::Write;
use tracing::{debug_span, info};

pub fn handle(args: ValidateArgs, _: &mut impl Write) -> Result<()> {
    let path = utils::file::get_absolute_path(&args.file, None)?;
    let pkg_aliases = load_aliases(&path)?;
    let mut script_checker = DefaultScriptChecker::new();

    let steps = config::load_steps(&args.file, ValidationMode::Full)?;
    if steps.is_empty() {
        bail!("The file doesn't contain any steps");
    }

    let filter_result = filter_steps(&steps, &OS_INFO, false, &args.steps, &args.tags_expr, &None)?;
    let mut steps = filter_result
        .filtered_steps
        .into_iter()
        .cloned()
        .collect::<Vec<Step>>();
    steps
        .iter_mut()
        .for_each(|s| s.selection_reason = Some(StepSelectionReason::MatchedFilter));

    let steps: Vec<runner::Step> = steps
        .iter()
        .map(|s| runner::Step::from(s, &pkg_aliases))
        .collect();

    check_scripts(&steps, &mut script_checker)?;
    info!("✅ Validation completed");
    Ok(())
}

fn check_scripts(steps: &[runner::Step], script_checker: &mut dyn ScriptChecker) -> Result<()> {
    let _span = debug_span!("check_scripts").entered();
    info!("Checking scripts...");

    let skip_if_shell_unavailable = true;
    let mut checked_count = 0;
    let check_step_script = |step: &runner::Step,
                             script_name: &str,
                             script: &Option<Script>,
                             script_checker: &mut dyn ScriptChecker|
     -> Result<usize> {
        if let Some(script) = script {
            script_checker
                .check_script(script, skip_if_shell_unavailable)
                .context(format!(
                    "Failed to check {script_name} in {}, step '{}'",
                    step.source_file, step.id
                ))?;
            return Ok(1);
        }
        Ok(0)
    };

    for step in steps.iter() {
        checked_count += check_step_script(step, "pre-script", &step.pre_script, script_checker)?;
        checked_count += check_step_script(step, "script", &step.script, script_checker)?;
        checked_count +=
            check_step_script(step, "check-script", &step.check_script, script_checker)?;
    }

    info!(event_type = %EventType::ScriptsCheckCompleted.as_str(), count=checked_count);
    Ok(())
}
