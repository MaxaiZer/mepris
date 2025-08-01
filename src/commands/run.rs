use std::io::Write;

use crate::{
    check_script::DefaultScriptChecker,
    cli::RunArgs,
    config::Step,
    os_info::{OS_INFO, OsInfo},
    parser,
    runner::{self, DryRunPlan},
};
use anyhow::{Result, bail};

use super::utils::{
    RunStateSaver, check_steps_exist, check_unique_id, filter_by_os, filter_by_tags,
    filter_steps_start_with_id,
};

pub fn handle(args: RunArgs, out: &mut impl Write) -> Result<()> {
    let state_saver = RunStateSaver {
        file: args.file.clone(),
        tags: args.tags.clone(),
        steps: args.steps.clone(),
    };
    let mut script_checker = DefaultScriptChecker::new();

    let steps = parser::parse(&args.file)?;
    if steps.is_empty() {
        bail!("The file doesn't contain any steps");
    }

    check_unique_id(&steps)?;

    let filter_result = filter_steps(&steps, &OS_INFO, &args)?;

    let params = runner::RunParameters {
        dry_run: args.dry_run,
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
        check_steps_exist(&res.filtered_steps, &args.steps)?;
        res.filtered_steps.retain(|s| args.steps.contains(&s.id));
    }

    if !args.tags.is_empty() {
        let filter_by_tags = filter_by_tags(&res.filtered_steps, &args.tags)?;
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
    dry_run_plan: &DryRunPlan,
    out: &mut impl Write,
) -> Result<()> {
    let to_ids = |steps: &[&Step]| -> String {
        steps
            .iter()
            .map(|s| s.id.as_str())
            .collect::<Vec<&str>>()
            .join(", ")
    };

    for step_id in &dry_run_plan.steps_to_run {
        writeln!(out, "🚀 Would run step '{step_id}'")?;

        if dry_run_plan.packages_to_install.contains_key(step_id) {
            let packages = dry_run_plan
                .packages_to_install
                .get(step_id)
                .unwrap()
                .join(", ");
            writeln!(out, "📦 Would install packages {packages}")?;
        }

        if dry_run_plan.missing_shells.contains_key(step_id) {
            let shells = dry_run_plan.missing_shells.get(step_id).unwrap().join(", ");
            writeln!(
                out,
                "⚠️ Step '{step_id}' uses shell(s) that are not currently available. Make sure they are installed in the previous steps: {shells}",
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
fn test_filter_tags() -> Result<()> {
    use anyhow::Ok;

    let steps = vec![
        Step {
            id: "id1".to_string(),
            os: Some(crate::config::parse("!linux").unwrap()),
            ..Default::default()
        },
        Step {
            id: "id2".to_string(),
            os: Some(crate::config::parse("linux").unwrap()),
            tags: vec!["tag1".to_string()],
            ..Default::default()
        },
        Step {
            id: "id3".to_string(),
            os: Some(crate::config::parse("linux").unwrap()),
            tags: vec!["tag1".to_string(), "tag2".to_string()],
            ..Default::default()
        },
        Step {
            id: "id4".to_string(),
            os: Some(crate::config::parse("!linux").unwrap()),
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
        tags: vec![],
        steps: vec![],
        start_step_id: None,
        dry_run: false,
    };

    let mut res = filter_steps(&steps, &os_info, &args).unwrap();
    assert_eq!(res.filtered_steps.len(), 2);
    assert_eq!(res.excluded_by_tags.len(), 0);
    assert_eq!(res.excluded_by_os.len(), 2);
    assert_eq!(res.skipped.len(), 0);

    args.tags = vec!["tag1".to_string()];
    res = filter_steps(&steps, &os_info, &args).unwrap();
    assert_eq!(res.filtered_steps.len(), 2);
    assert_eq!(res.excluded_by_tags.len(), 2);

    args.steps = vec!["id2".to_string()];
    res = filter_steps(&steps, &os_info, &args).unwrap();
    assert_eq!(res.filtered_steps.len(), 1);
    assert_eq!(res.excluded_by_tags.len(), 0);

    args.steps = vec![];
    args.start_step_id = Some("id3".to_string());
    res = filter_steps(&steps, &os_info, &args).unwrap();
    assert_eq!(res.filtered_steps.len(), 1);
    assert_eq!(res.excluded_by_tags.len(), 2);
    assert_eq!(res.skipped.len(), 1);

    Ok(())
}
