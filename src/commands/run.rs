use std::io::Write;

use crate::{
    cli::RunArgs,
    commands::utils::filter_by_ids,
    config::Step,
    helpers,
    os_info::{OsInfo, OS_INFO},
    parser::{self},
    runner::{self, dry},
};
use anyhow::{bail, Result};
use colored::Colorize;
use crate::runner::script_checker::DefaultScriptChecker;
use super::utils::{
    check_env, check_unique_id, filter_by_os, filter_by_tags, filter_steps_start_with_id,
    load_env, RunStateSaver,
};

pub fn handle(args: RunArgs, out: &mut impl Write) -> Result<()> {
    let state_saver = RunStateSaver {
        file: helpers::get_absolute_path(&args.file, None)?
            .to_str()
            .unwrap()
            .to_string(),
        tags_expr: args.tags_expr.clone(),
        steps: args.steps.clone(),
    };
    let mut script_checker = DefaultScriptChecker::new();

    let steps = parser::parse(&args.file)?;
    if steps.is_empty() {
        bail!("The file doesn't contain any steps");
    }

    check_unique_id(&steps)?;

    let filter_result = filter_steps(&steps, &OS_INFO, &args)?;

    load_env(&args.file)?;
    check_env(&filter_result.filtered_steps)?;

    let params = runner::RunParameters {
        source_file_path: state_saver.file.clone().into(),
        dry_run: args.dry_run,
        interactive: args.interactive,
    };
    let dry_run_plan = runner::run(
        &filter_result.filtered_steps,
        &params,
        &state_saver,
        &mut script_checker,
        out,
    )?;

    if args.dry_run
        && let Some(dry_run_plan) = dry_run_plan
    {
        print_info(
            &filter_result.excluded_by_tags,
            &filter_result.excluded_by_os,
            &filter_result.skipped,
            &dry_run_plan,
            out,
        )?;
    }
    Ok(())
}

struct FilterResult<'a> {
    filtered_steps: Vec<&'a Step>,
    excluded_by_tags: Vec<&'a Step>,
    excluded_by_os: Vec<&'a Step>,
    skipped: Vec<&'a Step>,
}

fn filter_steps<'a>(
    steps: &'a [Step],
    os_info: &OsInfo,
    args: &RunArgs,
) -> Result<FilterResult<'a>> {
    let mut res = FilterResult {
        filtered_steps: steps.iter().collect::<Vec<&Step>>(),
        excluded_by_tags: vec![],
        excluded_by_os: vec![],
        skipped: vec![],
    };

    if !args.steps.is_empty() {
        let filter_by_ids = filter_by_ids(&res.filtered_steps, &args.steps)?;
        res.filtered_steps = filter_by_ids.matching;
    }

    if args.tags_expr.is_some() {
        let filter_by_tags = filter_by_tags(&res.filtered_steps, args.tags_expr.as_ref().unwrap())?;
        res.excluded_by_tags = filter_by_tags.not_matching;
        res.filtered_steps = filter_by_tags.matching;
    }

    let filter_by_os = filter_by_os(&res.filtered_steps, os_info)?;
    res.filtered_steps = filter_by_os.matching;
    res.excluded_by_os = filter_by_os.not_matching;

    if let Some(start_step_id) = args.start_step_id.as_ref() {
        let filter_start_with_id = filter_steps_start_with_id(&res.filtered_steps, start_step_id)?;
        res.filtered_steps = filter_start_with_id.matching;
        res.skipped = filter_start_with_id.not_matching;
    }
    Ok(res)
}

fn print_info(
    excluded_by_tags: &[&Step],
    excluded_by_os: &[&Step],
    skipped: &[&Step],
    dry_run_plan: &dry::RunPlan,
    out: &mut impl Write,
) -> Result<()> {
    let to_ids = |steps: &[&Step]| -> String {
        steps
            .iter()
            .map(|s| s.id.as_str())
            .collect::<Vec<&str>>()
            .join(", ")
    };

    for step in &dry_run_plan.steps_to_run {
        let step_id = step.id.as_str();
        writeln!(out, "🚀 Would run step {}", step_id.cyan())?;

        if !step.packages_to_install.is_empty() {
            let get_pkgs = |installed: bool| {
                return step.packages_to_install
                    .iter()
                    .filter(|p| p.installed == installed)
                    .map(|p| p.to_string())
                    .collect::<Vec<String>>()
                    .join(", ");
            };
            let mut installed_packages = get_pkgs(true);
            let mut not_installed_packages = get_pkgs(false);

            if !installed_packages.is_empty() {
                installed_packages = "Already installed ".to_owned().green().to_string() + &installed_packages;
                if !not_installed_packages.is_empty() {
                    installed_packages = installed_packages + ",";
                }
            }
            if !not_installed_packages.is_empty() {
                not_installed_packages = "Would install packages ".to_owned().yellow().to_string() + &not_installed_packages;
                if !installed_packages.is_empty() {
                    not_installed_packages =  " ".to_owned() + &not_installed_packages.replace("Would", "would");
                }
            }

            let manager_info = &step.package_manager.as_ref().unwrap();

            let info = format!("📦 {}{} ({})", installed_packages, not_installed_packages, manager_info.name);
            writeln!(out, "{}", info)?;

            if !manager_info.installed {
                writeln!(
                    out,
                    "{} Step '{step_id}' uses package manager that is not currently available. Make sure it's installed in the previous steps",
                    "Warning:".yellow(),
                )?;
            }
        }

        if !step.missing_shells.is_empty() {
            let shells = step.missing_shells.join(", ");
            writeln!(
                out,
                "{} Step '{step_id}' uses shell(s) that are not currently available. Make sure they are installed in the previous steps: {shells}",
                "Warning:".yellow(),
            )?;
        }
    }

    if dry_run_plan.steps_to_run.is_empty() {
        writeln!(out, "❌ No steps would be run")?;
    }

    if !excluded_by_tags.is_empty() {
        writeln!(
            out,
            "🚫 Ignored steps due to tag mismatch: {}",
            to_ids(excluded_by_tags)
        )?;
    }

    if !excluded_by_os.is_empty() {
        writeln!(
            out,
            "🚫 Ignored steps due to OS mismatch: {}",
            to_ids(excluded_by_os)
        )?;
    }

    if !skipped.is_empty() {
        writeln!(out, "⏭️ Skipped steps due to resume: {}", to_ids(skipped))?;
    }

    if !dry_run_plan.steps_ignored_by_when.is_empty() {
        writeln!(
            out,
            "🚫 Ignored steps due to failed when script: {}",
            dry_run_plan.steps_ignored_by_when.join(", ")
        )?;
    }

    Ok(())
}

#[test]
fn test_filter() -> Result<()> {
    use crate::config::expr::parse;
    use anyhow::Ok;

    let steps = vec![
        Step {
            id: "id1".to_string(),
            os: Some(parse("!linux").unwrap()),
            ..Default::default()
        },
        Step {
            id: "id2".to_string(),
            os: Some(parse("linux").unwrap()),
            tags: vec!["tag1".to_string()],
            ..Default::default()
        },
        Step {
            id: "id3".to_string(),
            os: Some(parse("linux").unwrap()),
            tags: vec!["tag1".to_string(), "tag2".to_string()],
            ..Default::default()
        },
        Step {
            id: "id4".to_string(),
            os: Some(parse("!linux").unwrap()),
            tags: vec!["tag3".to_string()],
            ..Default::default()
        },
    ];

    let os_info = OsInfo {
        platform: crate::os_info::Platform::Linux,
        id: None,
        id_like: vec![],
    };

    let mut args = RunArgs {
        file: "file".to_string(),
        tags_expr: None,
        steps: vec![],
        start_step_id: None,
        interactive: false,
        dry_run: false,
    };

    let mut res = filter_steps(&steps, &os_info, &args).unwrap();
    assert_eq!(res.filtered_steps.len(), 2, "testing filter only by os");
    assert_eq!(res.excluded_by_tags.len(), 0, "testing filter only by os");
    assert_eq!(res.excluded_by_os.len(), 2, "testing filter only by os");
    assert_eq!(res.skipped.len(), 0, "testing filter only by os");

    args.tags_expr = Some("tag1".to_string());
    res = filter_steps(&steps, &os_info, &args).unwrap();
    assert_eq!(res.filtered_steps.len(), 2, "testing filter by single tag");
    assert_eq!(
        res.excluded_by_tags.len(),
        2,
        "testing filter by single tag"
    );

    args.tags_expr = Some("tag1 && tag2".to_string());
    res = filter_steps(&steps, &os_info, &args).unwrap();
    assert_eq!(
        res.filtered_steps.len(),
        1,
        "testing filter by tag1 AND tag2"
    );
    assert_eq!(
        res.excluded_by_tags.len(),
        3,
        "testing filter by tag1 AND tag2"
    );

    args.steps = vec!["id3".to_string()];
    res = filter_steps(&steps, &os_info, &args).unwrap();
    assert_eq!(res.filtered_steps.len(), 1, "testing filter by step id");
    assert_eq!(res.excluded_by_tags.len(), 0, "testing filter by step id");

    args.tags_expr = Some("tag1".to_string());
    args.steps = vec![];
    args.start_step_id = Some("id3".to_string());
    res = filter_steps(&steps, &os_info, &args).unwrap();
    assert_eq!(
        res.filtered_steps.len(),
        1,
        "testing filter by start step id"
    );
    assert_eq!(
        res.excluded_by_tags.len(),
        2,
        "testing filter by start step id"
    );
    assert_eq!(res.skipped.len(), 1, "testing filter by start step id");

    Ok(())
}

#[test]
fn test_unknown_tags() -> Result<()> {
    use anyhow::Ok;

    let steps = vec![
        Step {
            id: "id1".to_string(),
            ..Default::default()
        },
        Step {
            id: "id2".to_string(),
            tags: vec!["tag1".to_string()],
            ..Default::default()
        },
        Step {
            id: "id3".to_string(),
            tags: vec!["tag1".to_string(), "tag2".to_string()],
            ..Default::default()
        },
        Step {
            id: "id4".to_string(),
            tags: vec!["tag3".to_string()],
            ..Default::default()
        },
    ];

    let os_info = OsInfo {
        platform: crate::os_info::Platform::Linux,
        id: None,
        id_like: vec![],
    };

    let mut args = RunArgs {
        file: "file".to_string(),
        tags_expr: Some("tag4".to_string()),
        steps: vec![],
        start_step_id: None,
        interactive: false,
        dry_run: false,
    };

    assert!(filter_steps(&steps, &os_info, &args).is_err());

    args.tags_expr = Some("tag1 || tag4".to_string());
    assert!(filter_steps(&steps, &os_info, &args).is_err());

    args.tags_expr = Some("!tag4".to_string());
    assert!(filter_steps(&steps, &os_info, &args).is_err());

    Ok(())
}
