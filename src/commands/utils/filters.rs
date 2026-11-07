use crate::commands::utils::{check_env, check_tags_exist};
use crate::config::Step;
use crate::config::expr::os::eval_os_expr;
use crate::config::expr::parse;
use crate::config::expr::tags::eval_tags_expr;
use crate::logging::{EventType, SpanType};
use crate::runner;
use crate::runner::script::{ScriptStatus, run_noninteractive_script};
use crate::system::os_info::OsInfo;
use anyhow::{Context, bail};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use tracing::{debug, debug_span};

#[derive(Default)]
pub struct FilterConfig {
    apply_ids: Vec<String>,
    apply_previously_run_ids: Vec<String>,
    apply_tags: Option<String>,
    apply_os: Option<OsInfo>,
    apply_when: bool,
}

impl FilterConfig {
    pub fn new() -> FilterConfig {
        FilterConfig::default()
    }

    pub fn apply_ids(mut self, ids: Vec<String>) -> FilterConfig {
        self.apply_ids = ids;
        self
    }

    pub fn apply_previously_run_ids(mut self, ids: Vec<String>) -> FilterConfig {
        self.apply_previously_run_ids = ids;
        self
    }

    pub fn apply_tags(mut self, tags: String) -> FilterConfig {
        self.apply_tags = Some(tags);
        self
    }

    pub fn apply_os(mut self, os: OsInfo) -> FilterConfig {
        self.apply_os = Some(os);
        self
    }

    pub fn apply_when(mut self) -> FilterConfig {
        self.apply_when = true;
        self
    }
}

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

    pub fn add_not_matching(&mut self, filter_result: &FilterResult<'a>) {
        filter_result
            .not_matching
            .iter()
            .for_each(|s| self.add_failed_step(s, filter_result.filter));
    }

    #[cfg(test)]
    pub fn excluded_by(&self, filter: StepFilter) -> Vec<&'a Step> {
        self.excluded_steps
            .values()
            .filter(|v| v.failed_filters.contains(&filter))
            .map(|v| v.step)
            .collect()
    }

    pub fn is_excluded_by(&self, step_id: &str, filter: StepFilter) -> bool {
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
    ByPreviousRunState,
}

pub struct FilterResult<'a> {
    pub filter: StepFilter,
    pub matching: Vec<&'a Step>,
    pub not_matching: Vec<&'a Step>,
}

fn filter_by_ids<'a>(steps: &[&'a Step], ids: &[String]) -> anyhow::Result<FilterResult<'a>> {
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
        filter: StepFilter::ByIds,
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
        filter: StepFilter::ByTags,
        matching,
        not_matching,
    })
}

pub fn filter_by_os<'a>(steps: &[&'a Step], os_info: &OsInfo) -> FilterResult<'a> {
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
    FilterResult {
        filter: StepFilter::ByOs,
        matching,
        not_matching,
    }
}

pub fn filter_by_when_script<'a>(steps: &[&'a Step]) -> anyhow::Result<FilterResult<'a>> {
    let mut matching = Vec::new();
    let mut not_matching = Vec::new();

    check_env(
        &steps
            .iter()
            .filter(|s| s.when_script.is_some())
            .cloned()
            .collect::<Vec<&Step>>(),
    )?;

    for s in steps {
        if s.when_script.is_none() {
            matching.push(*s);
            continue;
        }

        let _span = debug_span!("when-script check", step_id = s.id).entered();

        let script = runner::Script::from_step(
            s.when_script.as_ref().unwrap(),
            &s.defaults,
            &s.id,
            &s.with,
        )?;
        let run =
            run_noninteractive_script(&script, Path::new(&s.source_file).parent().unwrap(), None)
                .context(format!("failed to run when-script for step '{}'", s.id))?;

        debug!(
            event_type = %EventType::ScriptCompleted.as_str(),
            code = run.status.code(),
            elapsed_secs = run.time.as_secs_f64(),
            kind = "when-script",
        );
        match run.status {
            ScriptStatus::Success => matching.push(*s),
            ScriptStatus::Failed(_) => not_matching.push(*s),
        }
    }

    Ok(FilterResult {
        filter: StepFilter::ByWhenScript,
        matching,
        not_matching,
    })
}

fn filter_by_previously_run<'a>(
    steps: &[&'a Step],
    ids: &[String],
) -> anyhow::Result<FilterResult<'a>> {
    let map: HashMap<&str, &Step> = steps.iter().copied().map(|s| (s.id.as_str(), s)).collect();

    Ok(FilterResult {
        filter: StepFilter::ByPreviousRunState,
        matching: map
            .values()
            .copied()
            .filter(|s| !ids.contains(&s.id))
            .collect(),
        not_matching: ids
            .iter()
            .filter_map(|id| map.get(id.as_str()).copied())
            .collect(),
    })
}

fn collect_when_candidates<'a>(
    selected_steps: &[&'a Step],
    all_steps: &[&'a Step],
    filters: &AllFiltersResult<'a>,
) -> Vec<&'a Step> {
    let mut providers: HashMap<&'a str, Vec<&'a Step>> = HashMap::new();

    for &step in all_steps {
        if filters.is_excluded_by(&step.id, StepFilter::ByOs)
            || filters.is_excluded_by(&step.id, StepFilter::ByPreviousRunState)
        {
            continue;
        }

        for provide in &step.provides {
            providers.entry(provide.as_str()).or_default().push(step);
        }
    }

    let mut needed: HashSet<&'a str> = selected_steps.iter().map(|s| s.id.as_str()).collect();
    let mut queue: Vec<&'a Step> = selected_steps.to_vec();

    while let Some(step) = queue.pop() {
        for require in &step.requires {
            if let Some(provider_steps) = providers.get(require.id.as_str()) {
                for &provider in provider_steps {
                    if needed.insert(provider.id.as_str()) {
                        queue.push(provider);
                    }
                }
            }
        }
    }

    all_steps
        .iter()
        .copied()
        .filter(|step| needed.contains(step.id.as_str()))
        .collect()
}

pub fn filter_steps<'a>(
    steps: &'a [Step],
    config: &FilterConfig,
) -> anyhow::Result<AllFiltersResult<'a>> {
    debug!("Filtering steps...");
    let _span = debug_span!(SpanType::Filter.as_str()).entered();

    let mut res = AllFiltersResult::new();

    let all_steps = steps.iter().collect::<Vec<&Step>>();
    let mut filtered_by_ids: Vec<&Step> = Vec::new();

    if !config.apply_ids.is_empty() {
        let filter_by_ids = filter_by_ids(&all_steps, &config.apply_ids)?;
        res.add_not_matching(&filter_by_ids);
        filtered_by_ids = filter_by_ids.matching;
    }

    if let Some(tags_expr) = config.apply_tags.as_ref() {
        let filter_by_tags = filter_by_tags(&all_steps, tags_expr)?;
        res.add_not_matching(&filter_by_tags);
    }

    if let Some(os_info) = config.apply_os.as_ref() {
        let filter_by_os = filter_by_os(&all_steps, os_info);
        res.add_not_matching(&filter_by_os);
    }

    if !config.apply_previously_run_ids.is_empty() {
        let filter_by_previous_run =
            filter_by_previously_run(&all_steps, &config.apply_previously_run_ids)?;
        res.add_not_matching(&filter_by_previous_run);
    }

    let selected_steps: Vec<&Step> = if filtered_by_ids.is_empty() {
        all_steps
            .iter()
            .copied()
            .filter(|s| !res.excluded_steps.contains_key(&s.id))
            .collect()
    } else {
        //to preserve ids order of selected steps
        filtered_by_ids
            .iter()
            .copied()
            .filter(|s| !res.excluded_steps.contains_key(&s.id))
            .collect()
    };

    // Run when-scripts only for selected steps and steps statically reachable
    // as their dependencies. OS-filtered and previously run steps are never
    // candidates, but unselected and tag-filtered-out steps can become
    // dependencies and are kept in the provider map.
    if config.apply_when {
        let candidates = collect_when_candidates(&selected_steps, &all_steps, &res);
        let filter_by_when = filter_by_when_script(&candidates)?;
        res.add_not_matching(&filter_by_when);
    }

    res.filtered_steps = selected_steps
        .into_iter()
        .filter(|s| !res.excluded_steps.contains_key(&s.id))
        .collect();

    debug!(event_type=%EventType::FilterCompleted);
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::utils::filters::StepFilter::{ByIds, ByOs, ByPreviousRunState, ByTags};
    use crate::config::expr::Expr;
    use crate::system::os_info::Platform;

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
            &FilterConfig::new()
                .apply_os(os_info)
                .apply_when()
                .apply_ids(vec!["step1".to_string(), "step3".to_string()]),
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

        let result = filter_steps(
            &steps,
            &FilterConfig::new()
                .apply_os(os_info)
                .apply_when()
                .apply_tags("linux".to_string()),
        )
        .unwrap();

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

        let result = filter_steps(
            &steps,
            &FilterConfig::new()
                .apply_os(os_info.clone())
                .apply_when()
                .apply_tags("tag4".to_string()),
        );
        assert!(result.is_err());

        let result = filter_steps(
            &steps,
            &FilterConfig::new()
                .apply_os(os_info.clone())
                .apply_when()
                .apply_tags("tag1 || tag4".to_string()),
        );
        assert!(result.is_err());

        let result = filter_steps(
            &steps,
            &FilterConfig::new()
                .apply_os(os_info.clone())
                .apply_when()
                .apply_tags("!tag4".to_string()),
        );
        assert!(result.is_err());
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

        let result =
            filter_steps(&steps, &FilterConfig::new().apply_os(os_info).apply_when()).unwrap();

        assert_eq!(result.filtered_steps.len(), 2);
        assert!(result.filtered_steps.iter().any(|s| s.id == "step1"));
        assert!(result.filtered_steps.iter().any(|s| s.id == "step3"));
        assert_eq!(result.excluded_by(ByOs).len(), 1);
        assert!(result.excluded_by(ByOs).iter().any(|s| s.id == "step2"));
    }

    #[test]
    //step3 is excluded by steps ids, step4 by tags, step2 as previously run,  step1 remains
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
            &FilterConfig::new()
                .apply_os(os_info)
                .apply_when()
                .apply_ids(vec![
                    "step1".to_string(),
                    "step2".to_string(),
                    "step4".to_string(),
                ])
                .apply_tags("tag1".to_string())
                .apply_previously_run_ids(vec!["step2".to_string()]),
        )
        .unwrap();

        assert_eq!(result.filtered_steps.len(), 1);
        assert!(result.filtered_steps.iter().any(|s| s.id == "step1"));

        assert_eq!(result.excluded_by(ByTags).len(), 1);
        assert!(result.excluded_by(ByTags).iter().any(|s| s.id == "step4"));
        assert_eq!(result.excluded_by(ByIds).len(), 1);
        assert!(result.excluded_by(ByIds).iter().any(|s| s.id == "step3"));
        assert_eq!(result.excluded_by(ByPreviousRunState).len(), 1);
        assert!(
            result
                .excluded_by(ByPreviousRunState)
                .iter()
                .any(|s| s.id == "step2")
        );
    }

    #[test]
    fn test_filter_steps_with_when_script_execution() {
        use crate::config::{Require, Script, Step};
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
                requires: vec![Require {
                    id: "dep".into(),
                    os: None,
                    when_script: None,
                }],
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
        let result = filter_steps(
            &steps,
            &FilterConfig::new()
                .apply_os(os_info)
                .apply_when()
                .apply_ids(steps_ids),
        )
        .unwrap();

        assert!(result.excluded_by(ByOs).iter().any(|s| s.id == "step2"));

        let log_content = fs::read_to_string(log_file).expect("Failed to read log file");
        let executed_steps: Vec<&str> = log_content.lines().collect();

        // step1 should run (explicitly selected)
        assert!(executed_steps.contains(&"step1"));

        // step2 shouldn't run (excluded by os filter)
        assert!(!executed_steps.contains(&"step2"));

        // step3 should run because step1 requires its provide
        assert!(executed_steps.contains(&"step3"));

        // step5 shouldn't run - it has provides, but is not reachable from step1
        assert!(!executed_steps.contains(&"step5"));

        // step4 shouldn't run - empty provides and there is ids filter
        assert!(!executed_steps.contains(&"step4"));

        assert_eq!(executed_steps.len(), 2);
    }

    #[test]
    fn test_filter_steps_runs_when_only_for_reachable_dependencies() {
        use crate::config::{Require, Script, Step};
        use std::fs;
        use tempfile::tempdir;

        let dir = tempdir().expect("Failed to create temp dir");
        let log_file = dir.path().join("log.txt");

        let steps = vec![
            Step {
                id: "selected".into(),
                tags: vec!["go".into()],
                requires: vec![Require {
                    id: "dep".into(),
                    os: None,
                    when_script: None,
                }],
                source_file: "/file.yaml".to_string(),
                ..Default::default()
            },
            Step {
                id: "dep".into(),
                tags: vec!["other".into()],
                provides: vec!["dep".into()],
                when_script: Some(Script {
                    shell: None,
                    code: format!("echo dep >> {}", log_file.display()),
                }),
                source_file: "/file.yaml".to_string(),
                ..Default::default()
            },
            Step {
                id: "unrelated".into(),
                tags: vec!["other".into()],
                env: vec!["MEPRIS_TAG_EXCLUDED_ENV".into()],
                provides: vec!["unrelated-artifact".into()],
                when_script: Some(Script {
                    shell: None,
                    code: format!("echo unrelated >> {}", log_file.display()),
                }),
                source_file: "/file.yaml".to_string(),
                ..Default::default()
            },
        ];

        let os_info = create_os_info(Platform::Linux, None, vec![]);

        let result = filter_steps(
            &steps,
            &FilterConfig::new()
                .apply_os(os_info)
                .apply_when()
                .apply_tags("go".to_string()),
        )
        .unwrap();

        assert_eq!(result.filtered_steps.len(), 1);
        assert_eq!(result.filtered_steps[0].id, "selected");

        let log_content = fs::read_to_string(log_file).expect("Failed to read log file");
        let executed_steps: Vec<&str> = log_content.lines().collect();
        assert_eq!(executed_steps, vec!["dep"]);
    }

    #[test]
    fn test_filter_steps_runs_when_for_transitive_dependencies() {
        use crate::config::{Require, Script, Step};
        use std::fs;
        use tempfile::tempdir;

        let dir = tempdir().expect("Failed to create temp dir");
        let log_file = dir.path().join("log.txt");

        let steps = vec![
            Step {
                id: "selected".into(),
                requires: vec![Require {
                    id: "a".into(),
                    os: None,
                    when_script: None,
                }],
                source_file: "/file.yaml".to_string(),
                ..Default::default()
            },
            Step {
                id: "dep1".into(),
                provides: vec!["a".into()],
                requires: vec![Require {
                    id: "b".into(),
                    os: None,
                    when_script: None,
                }],
                when_script: Some(Script {
                    shell: None,
                    code: format!("echo dep1 >> {}", log_file.display()),
                }),
                source_file: "/file.yaml".to_string(),
                ..Default::default()
            },
            Step {
                id: "dep2".into(),
                provides: vec!["b".into()],
                when_script: Some(Script {
                    shell: None,
                    code: format!("echo dep2 >> {}", log_file.display()),
                }),
                source_file: "/file.yaml".to_string(),
                ..Default::default()
            },
            Step {
                id: "unrelated".into(),
                provides: vec!["c".into()],
                when_script: Some(Script {
                    shell: None,
                    code: format!("echo unrelated >> {}", log_file.display()),
                }),
                source_file: "/file.yaml".to_string(),
                ..Default::default()
            },
        ];

        let os_info = create_os_info(Platform::Linux, None, vec![]);

        filter_steps(
            &steps,
            &FilterConfig::new()
                .apply_os(os_info)
                .apply_when()
                .apply_ids(vec!["selected".to_string()]),
        )
        .unwrap();

        let log_content = fs::read_to_string(log_file).expect("Failed to read log file");
        let executed_steps: Vec<&str> = log_content.lines().collect();
        assert_eq!(executed_steps, vec!["dep1", "dep2"]);
    }
}
