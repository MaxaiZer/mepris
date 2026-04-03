use super::utils::{RunStateSaver, check_env, check_unique_id, load_env};
use crate::commands::utils::filters::StepFilter::{ByIds, ByOs, ByStartId, ByTags, ByWhenScript};
use crate::commands::utils::filters::{ExcludedStep, ExcludedStepVecExt, StepFilter, filter_steps};
use crate::commands::utils::sort::toposort_steps;
use crate::config::StepSelectionReason;
use crate::config::parser::{self};
use crate::runner::dry::StepRun;
use crate::runner::script_checker::DefaultScriptChecker;
use crate::runner::{CliInteractor, Interactor, StepCompletedResult};
use crate::{
    cli::RunArgs,
    config::Step,
    runner::{self, dry},
    system::os_info::OS_INFO,
    utils,
};
use anyhow::{Result, bail};
use colored::Colorize;
use std::io::{BufReader, Write, stdin};
use std::path::Path;

pub fn handle(args: RunArgs, out: &mut impl Write) -> Result<()> {
    let state_saver = RunStateSaver {
        file: utils::file::get_absolute_path(&args.file, None)?
            .to_str()
            .unwrap()
            .to_string(),
        tags_expr: args.tags_expr.clone(),
        steps: args.steps.clone(),
    };
    let mut script_checker = DefaultScriptChecker::new();
    let interactor: Option<&mut dyn Interactor> = if args.interactive {
        Some(&mut CliInteractor::new(BufReader::new(stdin())))
    } else {
        None
    };

    let steps = parser::parse(&args.file)?;
    if steps.is_empty() {
        bail!("The file doesn't contain any steps");
    }

    check_unique_id(&steps)?;

    let filter_result = filter_steps(
        &steps,
        &OS_INFO,
        &args.steps,
        &args.tags_expr,
        &args.start_step_id,
    )?;

    load_env(&args.file)?;
    check_env(&filter_result.filtered_steps)?;

    let steps = toposort_steps(&filter_result, &OS_INFO)?;

    let params = runner::RunParameters {
        source_file_path: state_saver.file.clone().into(),
        dry_run: args.dry_run,
    };

    let dry_run_plan = runner::run(
        &steps[..],
        &params,
        &state_saver,
        &mut script_checker,
        interactor,
        out,
    )?;

    if args.dry_run
        && let Some(dry_run_plan) = dry_run_plan
    {
        print_info(&filter_result.excluded_steps(), &dry_run_plan, out)?;
    }
    Ok(())
}

fn print_info(
    excluded_steps: &[&ExcludedStep],
    dry_run_plan: &dry::RunPlan,
    out: &mut impl Write,
) -> Result<()> {
    let has_pulled_dependencies = !dry_run_plan.steps_to_run.is_empty()
        && dry_run_plan.steps_to_run.first().unwrap().selection_reason
            == StepSelectionReason::Dependency;
    let mut previous_source_file = "";
    let mut printed_selected_steps_line = false;

    if has_pulled_dependencies {
        writeln!(out, "[PULLED DEPENDENCIES]")?;
    }

    for step in &dry_run_plan.steps_to_run {
        if has_pulled_dependencies
            && step.selection_reason != StepSelectionReason::Dependency
            && !printed_selected_steps_line
        {
            writeln!(out, "\n[SELECTED STEPS]")?;
            previous_source_file = "";
            printed_selected_steps_line = true;
        }
        let step_extra_info = step_extra_info(step, &dry_run_plan.steps_to_run);

        let cur_source_file = Path::new(&step.source_file)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap();
        if cur_source_file != previous_source_file {
            if !previous_source_file.is_empty() {
                writeln!(out)?;
            }
            writeln!(out, "From {}:", cur_source_file)?;
            previous_source_file = cur_source_file;
        }

        let step_id = step.id.as_str();

        if step.step_completed_result == StepCompletedResult::Completed {
            writeln!(
                out,
                "  ✅ Step {} completed {}",
                step_id.cyan(),
                step_extra_info
            )?;
            continue;
        }

        if step.step_completed_result == StepCompletedResult::HasScriptWithoutCheck {
            if step.packages_to_install.is_empty() {
                writeln!(
                    out,
                    "  ❔ Step {}: no check-script; step would run {}",
                    step_id.cyan(),
                    step_extra_info,
                )?;
            } else {
                writeln!(
                    out,
                    "  ❔ Step {}: all packages installed, but no check-script; step would still run {}",
                    step_id.cyan(),
                    step_extra_info,
                )?;
            }

            print_shells_info(step, out)?;
            continue;
        }

        writeln!(
            out,
            "  🚀 Would run step {} {}",
            step_id.cyan(),
            step_extra_info,
        )?;

        print_packages_info(step, out)?;
        print_shells_info(step, out)?;
    }

    if dry_run_plan.steps_to_run.is_empty() {
        writeln!(out, "❌ No steps would be run")?;
    }

    let excluded_steps = excluded_steps.not_excluded_by(ByIds);
    if !excluded_steps.is_empty() && !dry_run_plan.steps_to_run.is_empty() {
        writeln!(out)?;
    }

    let excluded_steps = print_excluded(
        &excluded_steps,
        ByTags,
        "⏭️ Skipped steps due to tag mismatch",
        out,
    )?;
    let excluded_steps = print_excluded(
        &excluded_steps,
        ByOs,
        "⏭️ Skipped steps due to OS mismatch",
        out,
    )?;
    let excluded_steps = print_excluded(
        &excluded_steps,
        ByWhenScript,
        "⏭️ Skipped steps due to failed when-script",
        out,
    )?;
    _ = print_excluded(
        &excluded_steps,
        ByStartId,
        "⏭️ Skipped steps due to resume",
        out,
    )?;

    Ok(())
}

fn print_packages_info(step: &StepRun, out: &mut impl Write) -> Result<()> {
    if step.packages_to_install.is_empty() {
        return Ok(());
    }

    let get_pkgs = |installed: bool| {
        step.packages_to_install
            .iter()
            .filter(|p| p.installed == installed)
            .map(|p| p.to_string())
            .collect::<Vec<String>>()
            .join(", ")
    };
    let mut installed_packages = get_pkgs(true);
    let mut not_installed_packages = get_pkgs(false);

    if !installed_packages.is_empty() {
        installed_packages =
            "Already installed ".to_owned().green().to_string() + &installed_packages;
        if !not_installed_packages.is_empty() {
            installed_packages += ",";
        }
    }
    if !not_installed_packages.is_empty() {
        not_installed_packages =
            "Would install packages ".to_owned().yellow().to_string() + &not_installed_packages;
        if !installed_packages.is_empty() {
            not_installed_packages =
                " ".to_owned() + &not_installed_packages.replace("Would", "would");
        }
    }

    let manager_info = &step.package_manager.as_ref().unwrap();

    let info = format!(
        "  📦 {}{} ({})",
        installed_packages, not_installed_packages, manager_info.name
    );
    writeln!(out, "{}", info)?;

    if !manager_info.installed {
        writeln!(
            out,
            "  {} Step '{}' uses package manager that is not currently available. Make sure it's installed in the previous steps",
            step.id,
            "Warning:".yellow(),
        )?;
    }
    Ok(())
}

fn print_shells_info(step: &StepRun, out: &mut impl Write) -> Result<()> {
    if !step.missing_shells.is_empty() {
        let shells = step.missing_shells.join(", ");
        writeln!(
            out,
            "  {} Step '{}' uses shell(s) that are not currently available. Make sure they are installed in the previous steps: {shells}",
            step.id,
            "Warning:".yellow(),
        )?;
    }
    Ok(())
}

fn print_excluded<'a>(
    excluded_steps: &'a [&'a ExcludedStep],
    filter: StepFilter,
    msg: &str,
    out: &mut impl Write,
) -> Result<Vec<&'a ExcludedStep<'a>>> {
    let by_filter = excluded_steps.excluded_by(filter);
    if !by_filter.is_empty() {
        writeln!(out, "{}: {}", msg, to_ids(&by_filter))?;
    }
    Ok(excluded_steps.not_excluded_by(filter))
}

fn step_extra_info(step: &StepRun, steps: &[StepRun]) -> String {
    let mut dependency_of_info = String::new();
    let mut dependencies_info = String::new();

    if !step.dependency_of.is_empty() {
        dependency_of_info = "dependency of ".to_string() + &step.dependency_of.join(", ") + ""
    }

    if !step.dependencies.is_empty() {
        let pending = steps
            .iter()
            .filter(|s| {
                step.dependencies.contains(&s.id)
                    && s.step_completed_result != StepCompletedResult::Completed
            })
            .map(|s| s.id.clone())
            .collect::<Vec<String>>();

        if !pending.is_empty() {
            dependencies_info = "pending steps: ".to_string() + &pending.join(", ") + ""
        }
    }

    let all_info = match (dependency_of_info.is_empty(), dependencies_info.is_empty()) {
        (true, true) => return "".to_owned(),
        (true, false) => dependencies_info.to_string(),
        (false, true) => dependency_of_info.to_string(),
        (false, false) => format!("{}, {}", dependency_of_info, dependencies_info),
    };

    "(".to_owned() + &all_info + ")"
}

fn to_ids(steps: &[&Step]) -> String {
    steps
        .iter()
        .map(|s| s.id.as_str())
        .collect::<Vec<&str>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Script;
    use crate::config::expr::Expr;
    use crate::runner::StepCompletedResult;
    use crate::runner::dry::{PackageInfo, PackageManagerInfo, RunPlan, StepRun};
    use crate::system::os_info::{OsInfo, Platform};
    use std::io::Cursor;

    fn create_step_run(
        id: &str,
        completed_result: StepCompletedResult,
        selection_reason: StepSelectionReason,
    ) -> StepRun {
        StepRun {
            id: id.to_string(),
            source_file: "test.yaml".to_string(),
            step_completed_result: completed_result,
            selection_reason,
            ..Default::default()
        }
    }

    fn create_os_info() -> OsInfo {
        OsInfo {
            platform: Platform::Linux,
            id: None,
            id_like: vec![],
        }
    }
    #[test]
    fn test_print_info_with_tags_exclusion() {
        let mut output = Cursor::new(Vec::new());
        let steps = [
            Step {
                id: "step1".to_string(),
                ..Default::default()
            },
            Step {
                id: "step2".to_string(),
                ..Default::default()
            },
            Step {
                id: "step3".to_string(),
                tags: vec!["tag".to_string()],
                ..Default::default()
            },
        ];
        let os_info = create_os_info();

        let filter_res =
            filter_steps(&steps, &os_info, &vec![], &Some("tag".to_string()), &None).unwrap();
        let dry_run_plan = RunPlan {
            steps_to_run: vec![create_step_run(
                "step3",
                StepCompletedResult::HasScriptWithoutCheck,
                StepSelectionReason::MatchedFilter,
            )],
        };

        print_info(&filter_res.excluded_steps(), &dry_run_plan, &mut output).unwrap();

        let output_str = String::from_utf8(output.into_inner()).unwrap();
        assert!(
            output_str.contains("⏭️ Skipped steps due to tag mismatch: step1, step2"),
            "unexpected output: \n{}",
            output_str
        );
    }

    #[test]
    fn test_print_info_with_os_exclusion() {
        let mut output = Cursor::new(Vec::new());
        let steps = [
            Step {
                id: "step1".to_string(),
                os: Some(Expr::Var("windows".to_string())),
                ..Default::default()
            },
            Step {
                id: "step2".to_string(),
                os: Some(Expr::Var("windows".to_string())),
                ..Default::default()
            },
            Step {
                id: "step3".to_string(),
                ..Default::default()
            },
        ];
        let os_info = create_os_info();

        let filter_res = filter_steps(&steps, &os_info, &vec![], &None, &None).unwrap();
        let dry_run_plan = RunPlan {
            steps_to_run: vec![create_step_run(
                "step3",
                StepCompletedResult::HasScriptWithoutCheck,
                StepSelectionReason::MatchedFilter,
            )],
        };

        print_info(&filter_res.excluded_steps(), &dry_run_plan, &mut output).unwrap();

        let output_str = String::from_utf8(output.into_inner()).unwrap();
        assert!(output_str.contains("⏭️ Skipped steps due to OS mismatch: step1, step2"));
    }

    #[test]
    fn test_print_info_with_when_script_exclusion() {
        let mut output = Cursor::new(Vec::new());
        let steps = [
            Step {
                id: "step1".to_string(),
                when_script: Some(Script {
                    shell: None,
                    code: "exit 1".to_string(),
                }),
                source_file: "/test.yaml".to_string(),
                ..Default::default()
            },
            Step {
                id: "step2".to_string(),
                when_script: Some(Script {
                    shell: None,
                    code: "exit 1".to_string(),
                }),
                source_file: "/test.yaml".to_string(),
                ..Default::default()
            },
        ];
        let os_info = create_os_info();

        let filter_res = filter_steps(&steps, &os_info, &vec![], &None, &None).unwrap();
        let dry_run_plan = RunPlan {
            steps_to_run: vec![create_step_run(
                "step3",
                StepCompletedResult::HasScriptWithoutCheck,
                StepSelectionReason::MatchedFilter,
            )],
        };

        print_info(&filter_res.excluded_steps(), &dry_run_plan, &mut output).unwrap();

        let output_str = String::from_utf8(output.into_inner()).unwrap();
        assert!(output_str.contains("⏭️ Skipped steps due to failed when-script: step1, step2"));
    }

    #[test]
    fn test_print_info_with_start_id_exclusion() {
        let mut output = Cursor::new(Vec::new());
        let steps = [
            Step {
                id: "step1".to_string(),
                ..Default::default()
            },
            Step {
                id: "step2".to_string(),
                ..Default::default()
            },
            Step {
                id: "step3".to_string(),
                ..Default::default()
            },
        ];
        let os_info = create_os_info();

        let filter_res =
            filter_steps(&steps, &os_info, &vec![], &None, &Some("step3".to_string())).unwrap();
        let dry_run_plan = RunPlan {
            steps_to_run: vec![create_step_run(
                "step3",
                StepCompletedResult::HasScriptWithoutCheck,
                StepSelectionReason::MatchedFilter,
            )],
        };

        print_info(&filter_res.excluded_steps(), &dry_run_plan, &mut output).unwrap();

        let output_str = String::from_utf8(output.into_inner()).unwrap();
        assert!(
            output_str.contains("⏭️ Skipped steps due to resume: step1, step2"),
            "unexpected output: \n{}",
            output_str
        );
    }

    #[test]
    fn test_print_info_tags_and_os() {
        let mut output = Cursor::new(Vec::new());
        let steps = [
            Step {
                id: "step1".to_string(),
                ..Default::default()
            },
            Step {
                id: "step2".to_string(),
                tags: vec!["tag".to_string()],
                os: Some(Expr::Var("windows".to_string())),
                ..Default::default()
            },
            Step {
                id: "step3".to_string(),
                tags: vec!["tag".to_string()],
                ..Default::default()
            },
        ];
        let os_info = create_os_info();

        let filter_res =
            filter_steps(&steps, &os_info, &vec![], &Some("tag".to_string()), &None).unwrap();
        let dry_run_plan = RunPlan {
            steps_to_run: vec![create_step_run(
                "step3",
                StepCompletedResult::HasScriptWithoutCheck,
                StepSelectionReason::MatchedFilter,
            )],
        };

        print_info(&filter_res.excluded_steps(), &dry_run_plan, &mut output).unwrap();

        let output_str = String::from_utf8(output.into_inner()).unwrap();
        assert!(
            output_str.contains("⏭️ Skipped steps due to tag mismatch: step1"),
            "unexpected output: \n{}",
            output_str
        );
        assert!(
            output_str.contains("⏭️ Skipped steps due to OS mismatch: step2"),
            "unexpected output: \n{}",
            output_str
        );
    }

    #[test]
    fn test_print_info_tags_os_when() {
        let mut output = Cursor::new(Vec::new());
        let steps = [
            Step {
                id: "step1".to_string(),
                ..Default::default()
            },
            Step {
                id: "step2".to_string(),
                ..Default::default()
            },
            Step {
                id: "step3".to_string(),
                tags: vec!["tag".to_string()],
                when_script: Some(Script {
                    shell: None,
                    code: "exit 1".to_string(),
                }),
                source_file: "/test.yaml".to_string(),
                ..Default::default()
            },
            Step {
                id: "step4".to_string(),
                tags: vec!["tag".to_string()],
                os: Some(Expr::Var("windows".to_string())),
                ..Default::default()
            },
        ];
        let os_info = create_os_info();

        let filter_res =
            filter_steps(&steps, &os_info, &vec![], &Some("tag".to_string()), &None).unwrap();
        let dry_run_plan = RunPlan {
            steps_to_run: vec![create_step_run(
                "step4",
                StepCompletedResult::HasScriptWithoutCheck,
                StepSelectionReason::MatchedFilter,
            )],
        };

        print_info(&filter_res.excluded_steps(), &dry_run_plan, &mut output).unwrap();

        let output_str = String::from_utf8(output.into_inner()).unwrap();
        assert!(
            output_str.contains("⏭️ Skipped steps due to tag mismatch: step1, step2"),
            "unexpected output: \n{}",
            output_str
        );
        assert!(
            output_str.contains("⏭️ Skipped steps due to OS mismatch: step4"),
            "unexpected output: \n{}",
            output_str
        );
        assert!(
            output_str.contains("⏭️ Skipped steps due to failed when-script: step3"),
            "unexpected output: \n{}",
            output_str
        );
    }

    #[test]
    fn test_print_info_all_filters() {
        let mut output = Cursor::new(Vec::new());
        let steps = [
            Step {
                id: "step1".to_string(),
                tags: vec!["tag".to_string()],
                ..Default::default()
            },
            Step {
                id: "step2".to_string(),
                tags: vec!["tag".to_string()],
                os: Some(Expr::Var("windows".to_string())),
                ..Default::default()
            },
            Step {
                id: "step3".to_string(),
                tags: vec!["tag".to_string()],
                when_script: Some(Script {
                    shell: None,
                    code: "exit 1".to_string(),
                }),
                source_file: "/test.yaml".to_string(),
                ..Default::default()
            },
            Step {
                id: "step4".to_string(),
                ..Default::default()
            },
            Step {
                id: "step5".to_string(),
                tags: vec!["tag".to_string()],
                ..Default::default()
            },
        ];
        let os_info = create_os_info();

        let filter_res = filter_steps(
            &steps,
            &os_info,
            &vec![],
            &Some("tag".to_string()),
            &Some("step5".to_string()),
        )
        .unwrap();
        let dry_run_plan = RunPlan {
            steps_to_run: vec![create_step_run(
                "step5",
                StepCompletedResult::HasScriptWithoutCheck,
                StepSelectionReason::MatchedFilter,
            )],
        };

        print_info(&filter_res.excluded_steps(), &dry_run_plan, &mut output).unwrap();

        let output_str = String::from_utf8(output.into_inner()).unwrap();
        assert!(
            output_str.contains("⏭️ Skipped steps due to tag mismatch: step4"),
            "unexpected output: \n{}",
            output_str
        );
        assert!(
            output_str.contains("⏭️ Skipped steps due to OS mismatch: step2"),
            "unexpected output: \n{}",
            output_str
        );
        assert!(
            output_str.contains("⏭️ Skipped steps due to failed when-script: step3"),
            "unexpected output: \n{}",
            output_str
        );
        assert!(
            output_str.contains("⏭️ Skipped steps due to resume: step1"),
            "unexpected output: \n{}",
            output_str
        );
    }

    #[test]
    fn test_print_info_excludes_by_ids() {
        let mut output = Cursor::new(Vec::new());
        let steps = [
            Step {
                id: "step1".to_string(),
                ..Default::default()
            },
            Step {
                id: "step2".to_string(),
                ..Default::default()
            },
            Step {
                id: "step3".to_string(),
                ..Default::default()
            },
        ];
        let os_info = create_os_info();

        let filter_res =
            filter_steps(&steps, &os_info, &vec!["step3".to_string()], &None, &None).unwrap();
        let dry_run_plan = RunPlan {
            steps_to_run: vec![create_step_run(
                "step3",
                StepCompletedResult::HasScriptWithoutCheck,
                StepSelectionReason::MatchedFilter,
            )],
        };

        print_info(&filter_res.excluded_steps(), &dry_run_plan, &mut output).unwrap();

        let output_str = String::from_utf8(output.into_inner()).unwrap();
        assert!(
            !output_str.contains("step1"),
            "unexpected output: \n{}",
            output_str
        );
        assert!(
            !output_str.contains("step2"),
            "unexpected output: \n{}",
            output_str
        );
    }

    #[test]
    fn test_print_info_empty_excluded_steps() {
        let mut output = Cursor::new(Vec::new());
        let steps = [Step {
            id: "step1".to_string(),
            ..Default::default()
        }];
        let os_info = create_os_info();

        let filter_res = filter_steps(&steps, &os_info, &vec![], &None, &None).unwrap();
        let dry_run_plan = RunPlan {
            steps_to_run: vec![create_step_run(
                "step1",
                StepCompletedResult::HasScriptWithoutCheck,
                StepSelectionReason::MatchedFilter,
            )],
        };

        print_info(&filter_res.excluded_steps(), &dry_run_plan, &mut output).unwrap();

        let output_str = String::from_utf8(output.into_inner()).unwrap();
        assert!(
            !output_str.contains("⏭️"),
            "unexpected output: \n{}",
            output_str
        );
    }

    #[test]
    fn test_print_info_no_steps_to_run() {
        let mut output = Cursor::new(Vec::new());
        let steps = [Step {
            id: "step1".to_string(),
            os: Some(Expr::Var("windows".to_string())),
            ..Default::default()
        }];
        let os_info = create_os_info();

        let filter_res = filter_steps(&steps, &os_info, &vec![], &None, &None).unwrap();
        let dry_run_plan = RunPlan {
            steps_to_run: vec![],
        };

        print_info(&filter_res.excluded_steps(), &dry_run_plan, &mut output).unwrap();

        let output_str = String::from_utf8(output.into_inner()).unwrap();
        assert!(
            output_str.contains("❌ No steps would be run"),
            "unexpected output: \n{}",
            output_str
        );
        assert!(
            output_str.contains("⏭️ Skipped steps due to OS mismatch: step1"),
            "unexpected output: \n{}",
            output_str
        );
    }

    #[test]
    fn test_print_info_with_dependencies() {
        let mut output = Cursor::new(Vec::new());
        let excluded_steps: Vec<&ExcludedStep> = vec![];
        let dry_run_plan = RunPlan {
            steps_to_run: vec![
                create_step_run(
                    "dep1",
                    StepCompletedResult::Completed,
                    StepSelectionReason::Dependency,
                ),
                create_step_run(
                    "step1",
                    StepCompletedResult::HasScriptWithoutCheck,
                    StepSelectionReason::MatchedFilter,
                ),
            ],
        };

        print_info(&excluded_steps, &dry_run_plan, &mut output).unwrap();

        let output_str = String::from_utf8(output.into_inner()).unwrap();
        assert!(output_str.contains("[PULLED DEPENDENCIES]"));
        assert!(output_str.contains("\n[SELECTED STEPS]"));
    }

    #[test]
    fn test_print_info_step_with_pending_steps() {
        let mut output = Cursor::new(Vec::new());
        let excluded_steps: Vec<&ExcludedStep> = vec![];
        let dry_run_plan = RunPlan {
            steps_to_run: vec![
                StepRun {
                    id: "step1".to_string(),
                    source_file: "/test.yaml".to_string(),
                    step_completed_result: StepCompletedResult::NotInstalledPackageManager,
                    dependencies: vec![],
                    dependency_of: vec!["step2".to_string()],
                    ..Default::default()
                },
                StepRun {
                    id: "step2".to_string(),
                    source_file: "/test.yaml".to_string(),
                    dependencies: vec!["step1".to_string()],
                    ..Default::default()
                },
            ],
        };

        print_info(&excluded_steps, &dry_run_plan, &mut output).unwrap();

        let output_str = String::from_utf8(output.into_inner()).unwrap();
        assert!(output_str.contains("dependency of step2"));
        assert!(output_str.contains("pending steps: step1"));
    }

    #[test]
    fn test_print_info_completed_step() {
        let mut output = Cursor::new(Vec::new());
        let excluded_steps: Vec<&ExcludedStep> = vec![];
        let dry_run_plan = RunPlan {
            steps_to_run: vec![create_step_run(
                "step1",
                StepCompletedResult::Completed,
                StepSelectionReason::MatchedFilter,
            )],
        };

        print_info(&excluded_steps, &dry_run_plan, &mut output).unwrap();

        let clean = strip_ansi_escapes::strip(output.into_inner());
        let output_str = String::from_utf8(clean).unwrap();
        assert!(
            output_str.contains("✅ Step step1 completed"),
            "unexpected output: \n{}",
            output_str
        );
    }

    #[test]
    fn test_print_info_step_without_check_script() {
        let mut output = Cursor::new(Vec::new());
        let excluded_steps_ref: Vec<&ExcludedStep> = vec![];
        let mut step_run = create_step_run(
            "step1",
            StepCompletedResult::HasScriptWithoutCheck,
            StepSelectionReason::MatchedFilter,
        );
        step_run.packages_to_install = vec![];
        let dry_run_plan = RunPlan {
            steps_to_run: vec![step_run],
        };

        print_info(&excluded_steps_ref, &dry_run_plan, &mut output).unwrap();

        let clean = strip_ansi_escapes::strip(output.into_inner());
        let output_str = String::from_utf8(clean).unwrap();
        assert!(
            output_str.contains("❔ Step step1: no check-script; step would run"),
            "unexpected output: \n{}",
            output_str
        );
    }

    #[test]
    fn test_print_info_with_packages_to_install() {
        let mut output = Cursor::new(Vec::new());
        let excluded_steps_ref: Vec<&ExcludedStep> = vec![];
        let mut step_run = create_step_run(
            "step1",
            StepCompletedResult::NotInstalledPackages(vec!["pkg2".to_string()]),
            StepSelectionReason::MatchedFilter,
        );
        step_run.packages_to_install = vec![
            PackageInfo {
                name: "pkg1".to_string(),
                use_alias: false,
                installed: true,
            },
            PackageInfo {
                name: "pkg2".to_string(),
                use_alias: false,
                installed: false,
            },
        ];
        step_run.package_manager = Some(PackageManagerInfo {
            name: "apt".to_string(),
            installed: true,
        });
        let dry_run_plan = RunPlan {
            steps_to_run: vec![step_run],
        };

        print_info(&excluded_steps_ref, &dry_run_plan, &mut output).unwrap();

        let clean = strip_ansi_escapes::strip(output.into_inner());
        let output_str = String::from_utf8(clean).unwrap();
        assert!(
            output_str.contains("Would run step step1"),
            "unexpected output: \n{}",
            output_str
        );
        assert!(
            output_str.contains("Already installed pkg1"),
            "unexpected output: \n{}",
            output_str
        );
        assert!(
            output_str.contains("would install packages pkg2"),
            "unexpected output: \n{}",
            output_str
        );
        assert!(
            output_str.contains("apt"),
            "unexpected output: \n{}",
            output_str
        );
    }

    #[test]
    fn test_print_info_with_missing_shells() {
        let mut output = Cursor::new(Vec::new());
        let excluded_steps_ref: Vec<&ExcludedStep> = vec![];
        let mut step_run = create_step_run(
            "step1",
            StepCompletedResult::HasScriptWithoutCheck,
            StepSelectionReason::MatchedFilter,
        );
        step_run.missing_shells = vec!["fish".to_string(), "zsh".to_string()];
        let dry_run_plan = RunPlan {
            steps_to_run: vec![step_run],
        };

        print_info(&excluded_steps_ref, &dry_run_plan, &mut output).unwrap();

        let output_str = String::from_utf8(output.into_inner()).unwrap();
        assert!(
            output_str.contains("fish, zsh"),
            "unexpected output: \n{}",
            output_str
        );
    }
}
