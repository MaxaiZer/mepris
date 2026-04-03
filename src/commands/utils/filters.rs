use crate::commands::utils::check_tags_exist;
use crate::commands::utils::filters::StepFilter::{ByIds, ByOs, ByTags};
use crate::config::Step;
use crate::config::expr::{eval_os_expr, eval_tags_expr, parse};
use crate::runner;
use crate::runner::script::{ScriptResult, run_noninteractive_script};
use crate::system::os_info::OsInfo;
use anyhow::{Context, bail};
use std::collections::HashMap;
use std::path::Path;

pub struct AllFiltersResult<'a> {
    pub filtered_steps: Vec<&'a Step>,
    excluded_steps: indexmap::IndexMap<String, ExcludedStep<'a>>,
}

impl<'a> AllFiltersResult<'a> {
    pub fn new() -> Self {
        Self {
            filtered_steps: Vec::new(),
            excluded_steps: indexmap::IndexMap::new(),
        }
    }

    pub fn add_failed_step(&mut self, step: &'a Step, filter: StepFilter) {
        self.excluded_steps
            .entry(step.id.clone())
            .and_modify(|e| e.failed_filters.push(filter))
            .or_insert(ExcludedStep {
                step,
                failed_filters: vec![filter],
            });
    }

    pub fn excluded_by(&self, filter: StepFilter) -> Vec<&'a Step> {
        self.excluded_steps
            .values()
            .filter(|v| v.failed_filters.contains(&filter))
            .map(|v| v.step)
            .collect()
    }

    fn is_excluded_by(&self, step_id: &str, filter: StepFilter) -> bool {
        self.excluded_steps
            .get(step_id)
            .is_some_and(|e| e.failed_filters.contains(&filter))
    }

    pub fn excluded_steps(&self) -> Vec<&ExcludedStep<'a>> {
        self.excluded_steps.values().collect()
    }
}

pub struct ExcludedStep<'a> {
    pub failed_filters: Vec<StepFilter>,
    pub step: &'a Step,
}

pub trait ExcludedStepVecExt<'a> {
    fn excluded_by(&self, filter: StepFilter) -> Vec<&'a Step>;
    fn not_excluded_by(&self, filter: StepFilter) -> Vec<&ExcludedStep<'a>>;
}

impl<'a> ExcludedStepVecExt<'a> for [&ExcludedStep<'a>] {
    fn excluded_by(&self, filter: StepFilter) -> Vec<&'a Step> {
        self.iter()
            .filter(|s| s.failed_filters.contains(&filter))
            .map(|s| &s.step)
            .cloned()
            .collect()
    }

    fn not_excluded_by(&self, filter: StepFilter) -> Vec<&ExcludedStep<'a>> {
        self.iter()
            .filter(|s| !s.failed_filters.contains(&filter))
            .cloned()
            .collect()
    }
}

#[derive(PartialEq, Clone, Copy)]
pub enum StepFilter {
    ByIds,
    ByTags,
    ByOs,
    ByWhenScript,
    ByStartId,
}

pub struct FilterResult<'a> {
    pub matching: Vec<&'a Step>,
    pub not_matching: Vec<&'a Step>,
}

pub fn filter_by_ids<'a>(steps: &[&'a Step], ids: &[String]) -> anyhow::Result<FilterResult<'a>> {
    let map: HashMap<&str, &Step> = steps.iter().copied().map(|s| (s.id.as_str(), s)).collect();

    let unknown_steps: Vec<_> = ids
        .iter()
        .filter(|id| !map.contains_key(id.as_str()))
        .map(|id| id.as_str())
        .collect();

    if !unknown_steps.is_empty() {
        bail!("Unknown steps: {}", unknown_steps.join(", "));
    }

    Ok(FilterResult {
        matching: ids
            .iter()
            .map(|id| *map.get(id.as_str()).unwrap())
            .collect(),
        not_matching: map
            .values()
            .copied()
            .filter(|s| !ids.contains(&s.id))
            .collect(),
    })
}

pub fn filter_by_tags<'a>(steps: &[&'a Step], tags_expr: &str) -> anyhow::Result<FilterResult<'a>> {
    let expr = parse(tags_expr)?;
    check_tags_exist(steps, &expr.vars().into_iter().collect::<Vec<_>>())?;

    let (matching, not_matching): (Vec<_>, Vec<_>) =
        steps.iter().partition(|s| eval_tags_expr(&expr, &s.tags));
    Ok(FilterResult {
        matching,
        not_matching,
    })
}

pub fn filter_steps_start_with_id<'a>(
    steps: &[&'a Step],
    start_step_id: &str,
) -> anyhow::Result<FilterResult<'a>> {
    if let Some(pos) = steps.iter().position(|s| s.id == start_step_id) {
        let (not_matching, matching) = steps.split_at(pos);

        Ok(FilterResult {
            matching: matching.to_vec(),
            not_matching: not_matching.to_vec(),
        })
    } else {
        bail!("Start step '{start_step_id}' not found in file");
    }
}

pub fn filter_by_os<'a>(steps: &[&'a Step], os_info: &OsInfo) -> anyhow::Result<FilterResult<'a>> {
    let (matching, not_matching): (Vec<_>, Vec<_>) = steps.iter().partition(|s| {
        if s.os.is_none() {
            return true;
        }
        if let Some(os_expr) = &s.os
            && eval_os_expr(os_expr, os_info)
        {
            return true;
        }
        false
    });
    Ok(FilterResult {
        matching,
        not_matching,
    })
}

pub fn filter_by_when_script<'a>(steps: &[&'a Step]) -> anyhow::Result<FilterResult<'a>> {
    let mut matching = Vec::new();
    let mut not_matching = Vec::new();

    for s in steps {
        if s.when_script.is_none() {
            matching.push(*s);
            continue;
        }

        let script = runner::Script::from(s.when_script.as_ref().unwrap(), &s.defaults);
        let run =
            run_noninteractive_script(&script, Path::new(&s.source_file).parent().unwrap(), None)
                .context(format!("failed to run when-script for step '{}'", s.id))?;

        match run {
            ScriptResult::Success => matching.push(*s),
            ScriptResult::NotZeroExitStatus(_) => not_matching.push(*s),
        }
    }

    Ok(FilterResult {
        matching,
        not_matching,
    })
}

//filters priority: steps_ids > tags_expr > os > when-script > start_step_id
pub fn filter_steps<'a>(
    steps: &'a [Step],
    os_info: &OsInfo,
    steps_ids: &[String],
    tags_expr: &Option<String>,
    start_step_id: &Option<String>,
) -> anyhow::Result<AllFiltersResult<'a>> {
    let mut res = AllFiltersResult::new();

    let all_steps = steps.iter().collect::<Vec<&Step>>();
    let mut filtered_by_ids: Vec<&Step> = Vec::new();

    if !steps_ids.is_empty() {
        let filter_by_ids = filter_by_ids(&all_steps, steps_ids)?;
        filter_by_ids
            .not_matching
            .iter()
            .for_each(|s| res.add_failed_step(s, StepFilter::ByIds));
        filtered_by_ids = filter_by_ids.matching; //to preserve ids order of selected steps
    }

    if tags_expr.is_some() {
        let filter_by_tags = filter_by_tags(&all_steps, tags_expr.as_ref().unwrap())?;
        filter_by_tags
            .not_matching
            .iter()
            .for_each(|s| res.add_failed_step(s, StepFilter::ByTags));
    }

    let filter_by_os = filter_by_os(&all_steps, os_info)?;
    filter_by_os
        .not_matching
        .iter()
        .for_each(|s| res.add_failed_step(s, StepFilter::ByOs));

    // Run when-scripts only for steps that may participate in execution graph.
    //
    // Rules:
    // - OS-filtered steps are always excluded
    // - If no ids filter is provided, all remaining steps are checked
    // - If ids filter is provided:
    //     * explicitly selected steps are checked
    //     * steps that can be pulled as dependencies (have provides) are also checked
    //
    // This avoids running when-scripts for steps that can never be executed.
    let filter_by_when: FilterResult = filter_by_when_script(
        &all_steps
            .iter()
            .filter(|s| {
                if res.is_excluded_by(s.id.as_str(), ByOs) {
                    return false;
                }

                let has_ids_filter = !steps_ids.is_empty();
                let can_be_dependency = !s.provides.is_empty();
                let explicitly_selected = !res.is_excluded_by(s.id.as_str(), ByIds)
                    && !res.is_excluded_by(s.id.as_str(), ByTags);

                if !has_ids_filter {
                    return true;
                }

                explicitly_selected || can_be_dependency
            })
            .cloned()
            .collect::<Vec<&Step>>(),
    )?;

    filter_by_when
        .not_matching
        .iter()
        .for_each(|s| res.add_failed_step(s, StepFilter::ByWhenScript));

    if let Some(start_step_id) = start_step_id.as_ref() {
        let filter_start_with_id = filter_steps_start_with_id(&all_steps, start_step_id)?;
        filter_start_with_id
            .not_matching
            .iter()
            .for_each(|s| res.add_failed_step(s, StepFilter::ByStartId));
    }

    if filtered_by_ids.is_empty() {
        res.filtered_steps = all_steps;
    } else {
        res.filtered_steps = filtered_by_ids;
    }

    res.filtered_steps = res
        .filtered_steps
        .iter()
        .filter(|s| !res.excluded_steps.contains_key(&s.id))
        .cloned()
        .collect();
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::utils::filters::StepFilter::{ByIds, ByOs, ByStartId, ByTags};
    use crate::config::Script;
    use crate::config::expr::Expr;
    use crate::system::os_info::Platform;
    use tempfile::tempdir;

    fn create_step(id: &str, tags: Vec<&str>, os: Option<Expr>) -> Step {
        Step {
            id: id.to_string(),
            tags: tags.into_iter().map(String::from).collect(),
            os,
            ..Default::default()
        }
    }

    fn create_os_info(platform: Platform, id: Option<&str>, id_like: Vec<&str>) -> OsInfo {
        OsInfo {
            platform,
            id: id.map(String::from),
            id_like: id_like.into_iter().map(String::from).collect(),
        }
    }

    #[test]
    fn test_filter_steps_with_steps_ids() {
        let steps = vec![
            create_step("step1", vec![], None),
            create_step("step2", vec![], None),
            create_step("step3", vec![], None),
        ];
        let os_info = create_os_info(Platform::Linux, None, vec![]);

        let result = filter_steps(
            &steps,
            &os_info,
            &["step1".to_string(), "step3".to_string()],
            &None,
            &None,
        )
        .unwrap();

        assert_eq!(result.filtered_steps.len(), 2);
        assert!(result.filtered_steps.iter().any(|s| s.id == "step1"));
        assert!(result.filtered_steps.iter().any(|s| s.id == "step3"));
        assert!(!result.filtered_steps.iter().any(|s| s.id == "step2"));
        let excluded_by_ids = result.excluded_by(ByIds);
        assert_eq!(excluded_by_ids.len(), 1);
        assert_eq!(excluded_by_ids[0].id, "step2");
    }

    #[test]
    fn test_filter_steps_with_tags_expr() {
        let steps = vec![
            create_step("step1", vec!["linux"], None),
            create_step("step2", vec!["macos"], None),
            create_step("step3", vec!["linux", "dev"], None),
        ];
        let os_info = create_os_info(Platform::Linux, None, vec![]);

        let result =
            filter_steps(&steps, &os_info, &[], &Some("linux".to_string()), &None).unwrap();

        assert_eq!(result.filtered_steps.len(), 2);
        assert_eq!(result.excluded_by(ByTags).len(), 1);
        assert!(result.filtered_steps.iter().any(|s| s.id == "step1"));
        assert!(result.filtered_steps.iter().any(|s| s.id == "step3"));
        assert!(result.excluded_by(ByTags).iter().any(|s| s.id == "step2"));
    }

    #[test]
    fn test_filter_steps_with_tags_unknown() {
        let steps = vec![
            create_step("step1", vec![], None),
            create_step("step2", vec!["tag1"], None),
            create_step("step3", vec!["tag1", "tag2"], None),
            create_step("step3", vec!["tag3"], None),
        ];
        let os_info = create_os_info(Platform::Linux, None, vec![]);

        let result = filter_steps(&steps, &os_info, &[], &Some("tag4".to_string()), &None);
        assert!(result.is_err());

        let result = filter_steps(
            &steps,
            &os_info,
            &[],
            &Some("tag1 || tag4".to_string()),
            &None,
        );
        assert!(result.is_err());

        let result = filter_steps(&steps, &os_info, &[], &Some("!tag4".to_string()), &None);
        assert!(result.is_err());
    }

    #[test]
    fn test_filter_steps_with_start_step_id() {
        let steps = vec![
            create_step("step1", vec![], None),
            create_step("step2", vec![], None),
            create_step("step3", vec![], None),
        ];
        let os_info = create_os_info(Platform::Linux, None, vec![]);

        let result =
            filter_steps(&steps, &os_info, &[], &None, &Some("step2".to_string())).unwrap();

        assert_eq!(result.filtered_steps.len(), 2);
        assert!(result.filtered_steps.iter().any(|s| s.id == "step2"));
        assert!(result.filtered_steps.iter().any(|s| s.id == "step3"));
        assert_eq!(result.excluded_by(ByStartId).len(), 1);
        assert!(
            result
                .excluded_by(ByStartId)
                .iter()
                .any(|s| s.id == "step1")
        );
    }

    #[test]
    fn test_filter_steps_with_os_info() {
        let linux_expr = Expr::Var("linux".to_string());
        let windows_expr = Expr::Var("windows".to_string());

        let steps = vec![
            create_step("step1", vec![], Some(linux_expr.clone())),
            create_step("step2", vec![], Some(windows_expr.clone())),
            create_step("step3", vec![], None),
        ];
        let os_info = create_os_info(Platform::Linux, None, vec![]);

        let result = filter_steps(&steps, &os_info, &[], &None, &None).unwrap();

        assert_eq!(result.filtered_steps.len(), 2);
        assert!(result.filtered_steps.iter().any(|s| s.id == "step1"));
        assert!(result.filtered_steps.iter().any(|s| s.id == "step3"));
        assert_eq!(result.excluded_by(ByOs).len(), 1);
        assert!(result.excluded_by(ByOs).iter().any(|s| s.id == "step2"));
    }

    #[test]
    //step3 is excluded first by steps ids, then step4 by tags (not os by priority), then step1 by start_step_id. only step2 remains
    fn test_filter_steps_combined_all_filters() {
        let steps = vec![
            create_step(
                "step1",
                vec!["tag1"],
                Some(Expr::Var("linux".to_string()).clone()),
            ),
            create_step(
                "step2",
                vec!["tag1"],
                Some(Expr::Var("linux".to_string()).clone()),
            ),
            create_step(
                "step3",
                vec!["tag1"],
                Some(Expr::Var("linux".to_string()).clone()),
            ),
            create_step(
                "step4",
                vec!["tag2"],
                Some(Expr::Var("macos".to_string()).clone()),
            ),
        ];
        let os_info = create_os_info(Platform::Linux, None, vec![]);

        let result = filter_steps(
            &steps,
            &os_info,
            &[
                "step1".to_string(),
                "step2".to_string(),
                "step4".to_string(),
            ],
            &Some("tag1".to_string()),
            &Some("step2".to_string()),
        )
        .unwrap();

        assert_eq!(result.filtered_steps.len(), 1);
        assert!(result.filtered_steps.iter().any(|s| s.id == "step2"));

        assert_eq!(result.excluded_by(ByTags).len(), 1);
        assert!(result.excluded_by(ByTags).iter().any(|s| s.id == "step4"));

        assert_eq!(result.excluded_by(ByStartId).len(), 1);
        assert!(
            result
                .excluded_by(ByStartId)
                .iter()
                .any(|s| s.id == "step1")
        );
    }

    #[test]
    fn test_filter_steps_with_when_script_execution() {
        use crate::config::{Script, Step};
        use std::fs;
        use tempfile::tempdir;

        let dir = tempdir().expect("Failed to create temp dir");
        let log_file = dir.path().join("log.txt");

        let steps = vec![
            Step {
                id: "step1".into(),
                when_script: Some(Script {
                    shell: None,
                    code: format!("echo step1 >> {}", log_file.display()),
                }),
                source_file: "/file.yaml".to_string(),
                provides: vec!["p1".into()],
                ..Default::default()
            },
            Step {
                id: "step2".into(),
                os: Some(Expr::Var("windows".into())),
                when_script: Some(Script {
                    shell: None,
                    code: format!("echo step2 >> {}", log_file.display()),
                }),
                source_file: "/file.yaml".to_string(),
                provides: vec!["p2".into()],
                ..Default::default()
            },
            Step {
                id: "step3".into(),
                when_script: Some(Script {
                    shell: None,
                    code: format!("echo step3 >> {}", log_file.display()),
                }),
                source_file: "/file.yaml".to_string(),
                provides: vec!["dep".into()],
                ..Default::default()
            },
            Step {
                id: "step4".into(),
                when_script: Some(Script {
                    shell: None,
                    code: format!("echo step4 >> {}", log_file.display()),
                }),
                source_file: "/file.yaml".to_string(),
                ..Default::default()
            },
            Step {
                id: "step5".into(),
                tags: vec!["skip".into()],
                when_script: Some(Script {
                    shell: None,
                    code: format!("echo step5 >> {}", log_file.display()),
                }),
                source_file: "/file.yaml".to_string(),
                provides: vec!["dep2".into()],
                ..Default::default()
            },
        ];

        let os_info = create_os_info(Platform::Linux, None, vec![]);

        let steps_ids = vec!["step1".to_string()];
        let result = filter_steps(&steps, &os_info, &steps_ids, &None, &None).unwrap();

        assert!(result.excluded_by(ByOs).iter().any(|s| s.id == "step2"));

        let log_content = fs::read_to_string(log_file).expect("Failed to read log file");
        let executed_steps: Vec<&str> = log_content.lines().collect();

        // step1 should run (explicitly selected)
        assert!(executed_steps.contains(&"step1"));

        // step2 shouldn't run (excluded by os filter)
        assert!(!executed_steps.contains(&"step2"));

        // step3 and step5 should run (dependency)
        assert!(executed_steps.contains(&"step3"));
        assert!(executed_steps.contains(&"step5"));

        // step4 shouldn't run - empty provides and there is ids filter
        assert!(!executed_steps.contains(&"step4"));
    }
}
