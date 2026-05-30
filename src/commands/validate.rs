use crate::cli::ValidateArgs;
use crate::commands::utils::filters::{FilterConfig, filter_steps};
use crate::config::expr::os::os_expr_possible_platforms;
use crate::config::{Script, ValidationMode};
use crate::logging::EventType;
use crate::runner::script::resolve_shell;
use crate::runner::script_checker::{DefaultScriptChecker, ScriptChecker};
use crate::system::os_info::Platform::{Linux, MacOS, Windows};
use crate::system::shell::{Shell, is_shell_available};
use crate::{
    config,
    config::Step,
    runner::{self},
};
use anyhow::{Context, Result, bail};
use std::collections::HashSet;
use std::io::Write;
use tracing::{debug_span, info, warn};

impl From<&ValidateArgs> for FilterConfig {
    fn from(args: &ValidateArgs) -> Self {
        let mut conf = FilterConfig::new().apply_ids(args.steps.clone());
        if let Some(tags_expr) = args.tags_expr.as_ref() {
            conf = conf.apply_tags(tags_expr.clone());
        }
        conf
    }
}

pub fn handle(args: ValidateArgs, _: &mut impl Write) -> Result<()> {
    let mut script_checker = DefaultScriptChecker::new();

    let steps = config::load_steps(&args.file, ValidationMode::Full)?;
    if steps.is_empty() {
        bail!("The file doesn't contain any steps");
    }

    let filter_result = filter_steps(&steps, &FilterConfig::from(&args))?;
    let steps = filter_result
        .filtered_steps
        .into_iter()
        .cloned()
        .collect::<Vec<Step>>();

    check_scripts(&steps, &mut script_checker)?;
    info!("✅ Validation completed");
    Ok(())
}

fn check_scripts(steps: &[Step], script_checker: &mut dyn ScriptChecker) -> Result<()> {
    let _span = debug_span!("check_scripts").entered();
    info!("Checking scripts...");

    let mut checked_count = 0;
    let mut unavailable_shell_msgs: Vec<Shell> = vec![];

    for step in steps.iter() {
        check_step_script(
            step,
            "when-script",
            &step.when_script,
            &mut checked_count,
            &mut unavailable_shell_msgs,
            script_checker,
        )?;
        check_step_script(
            step,
            "pre-script",
            &step.pre_script,
            &mut checked_count,
            &mut unavailable_shell_msgs,
            script_checker,
        )?;
        check_step_script(
            step,
            "script",
            &step.script,
            &mut checked_count,
            &mut unavailable_shell_msgs,
            script_checker,
        )?;
        check_step_script(
            step,
            "check-script",
            &step.check_script,
            &mut checked_count,
            &mut unavailable_shell_msgs,
            script_checker,
        )?;
        for require in &step.requires {
            check_step_script(
                step,
                &format!("requirement '{}' when-script", require.id),
                &require.when_script,
                &mut checked_count,
                &mut unavailable_shell_msgs,
                script_checker,
            )?;
        }
    }

    info!(event_type = %EventType::ScriptsCheckCompleted.as_str(), count=checked_count);
    Ok(())
}

fn check_step_script(
    step: &Step,
    script_name: &str,
    script: &Option<Script>,
    checked_count: &mut usize,
    unavailable_shell_msgs: &mut Vec<Shell>,
    script_checker: &mut dyn ScriptChecker,
) -> Result<()> {
    let script = match script {
        Some(s) => s,
        None => return Ok(()),
    };

    let mut resolved_shells: HashSet<Shell> = HashSet::new();
    if script.shell.is_some() {
        resolved_shells.insert(script.shell.as_ref().unwrap().clone());
    } else {
        let platforms = if let Some(os_expr) = step.os.as_ref() {
            os_expr_possible_platforms(os_expr)
        } else {
            HashSet::from([Windows, Linux, MacOS])
        };

        platforms
            .iter()
            .map(|platform| resolve_shell(*platform, &step.defaults))
            .for_each(|shell| {
                resolved_shells.insert(shell);
            })
    }

    let mut checked = false;
    for shell in resolved_shells {
        let runner_script = runner::Script {
            shell,
            code: script.code.clone(),
        };

        if !is_shell_available(&runner_script.shell)
            && !unavailable_shell_msgs.contains(&runner_script.shell)
        {
            warn!(
                "Skipping script checks for '{}': shell not available",
                runner_script.shell.get_command()
            );
            unavailable_shell_msgs.push(runner_script.shell);
            continue;
        }

        script_checker
            .check_script(&runner_script, true)
            .context(format!(
                "Failed to check {script_name} in {}, step '{}'",
                step.source_file, step.id
            ))?;
        checked = true;
    }

    if checked {
        *checked_count += 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serial_test::serial;
    use crate::config::Defaults;
    use crate::config::expr::parse;
    use crate::system::shell::mock_available_shells;
    use super::*;

    #[derive(Default)]
    struct MockChecker {
        calls: usize,
    }

    impl ScriptChecker for MockChecker {
        fn check_script(&mut self, script: &runner::Script, _: bool) -> Result<()> {
            self.calls += 1;
            Ok(())
        }

        fn is_checked(&self, script: &runner::Script) -> bool {
            false
        }
    }

    #[test]
    #[serial]
    fn test_check_step_script_shell_unavailable() {
        mock_available_shells(HashSet::new());

        let mut checker = MockChecker::default();

        let step = Step {
            id: "step1".into(),
            source_file: "file.yaml".into(),
            script: Some(Script {
                shell: Some(Shell::Bash),
                code: "echo hi".into(),
            }),
            ..Default::default()
        };

        let mut checked = 0usize;
        let mut unavailable = vec![];

        check_step_script(
            &step,
            "script",
            &step.script,
            &mut checked,
            &mut unavailable,
            &mut checker,
        ).unwrap();

        assert_eq!(checked, 0);
        assert_eq!(checker.calls, 0);
    }

    #[test]
    #[serial]
    fn test_check_step_script_shell_available() {
        mock_available_shells([Shell::Bash].into_iter().collect());

        let mut checker = MockChecker::default();

        let step = Step {
            id: "step1".into(),
            source_file: "file.yaml".into(),
            script: Some(Script {
                shell: Some(Shell::Bash),
                code: "echo hi".into(),
            }),
            ..Default::default()
        };

        let mut checked = 0usize;
        let mut unavailable = vec![];

        check_step_script(
            &step,
            "script",
            &step.script,
            &mut checked,
            &mut unavailable,
            &mut checker,
        ).unwrap();

        assert_eq!(checked, 1);
        assert_eq!(checker.calls, 1);
    }

    #[test]
    #[serial]
    fn test_check_step_script_resolved_shells() {
        mock_available_shells([Shell::Bash].into_iter().collect());

        let mut checker = MockChecker::default();

        let step = Step {
            id: "step1".into(),
            source_file: "file.yaml".into(),
            os: Some(parse("windows || linux").unwrap()),
            script: Some(Script {
                shell: None,
                code: "echo hi".into(),
            }),
            defaults: Some(Defaults {
                windows_package_manager: None,
                windows_shell: Some(Shell::PowerShellCore),
                linux_shell: None,
                macos_shell: None,
            }),
            ..Default::default()
        };

        let mut checked = 0usize;
        let mut unavailable = vec![];

        check_step_script(
            &step,
            "script",
            &step.script,
            &mut checked,
            &mut unavailable,
            &mut checker,
        ).unwrap();

        assert!(checked >= 1);
        assert!(!unavailable.is_empty());
        assert_eq!(unavailable[0], Shell::PowerShellCore);
        assert_eq!(checker.calls, 1);
    }
}
