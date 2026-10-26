use crate::commands::utils::check_step_env;
use crate::commands::utils::filters::{AllFiltersResult, StepFilter};
use crate::config::expr::os::eval_os_expr;
use crate::config::{Require, Step, StepSelectionReason};
use crate::runner::run_noninteractive_script;
use crate::runner::script::ScriptStatus;
use crate::system::os_info::OsInfo;
use crate::{runner, utils};
use anyhow::{Context, bail};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;

pub fn toposort_steps(
    filter_result: &AllFiltersResult,
    os_info: &OsInfo,
) -> anyhow::Result<Vec<Step>> {
    let filtered_steps: Vec<Step> = filter_result
        .filtered_steps
        .iter()
        .copied()
        .cloned()
        .map(|mut s| {
            s.selection_reason = Some(StepSelectionReason::MatchedFilter);
            s
        })
        .collect();
    let excluded_steps: Vec<&Step> = filter_result
        .excluded_steps()
        .iter()
        .filter(|v| {
            !v.failed_filters.contains(&StepFilter::ByOs)
                && !v.failed_filters.contains(&StepFilter::ByWhenScript)
        })
        .map(|v| v.step)
        .collect();

    let excluded_steps: Vec<Step> = excluded_steps.into_iter().cloned().collect();

    let mut steps = filtered_steps;
    steps.extend(excluded_steps); // filtered steps + steps that failed non-strict filters (can be dependencies of filtered steps)

    let completed_steps_idx: HashSet<usize> = steps
        .iter()
        .enumerate()
        .filter(|(_, s)| filter_result.is_excluded_by(&s.id, StepFilter::ByPreviousRunState))
        .map(|(idx, _)| idx)
        .collect();

    let mut providers: HashMap<String, Vec<ProviderRef>> = HashMap::new();
    let mut unknown_requires: HashMap<String, Vec<String>> = HashMap::new();

    fill_providers(&steps, &mut providers)?;
    add_dependencies(
        &mut steps,
        &completed_steps_idx,
        &mut providers,
        &mut unknown_requires,
        os_info,
    )?;
    check_for_errors(&unknown_requires)?;

    let mut graph: utils::graph::Graph<usize> = utils::graph::Graph::new();

    for i in 0..steps.len() {
        if steps[i].selection_reason.is_none() {
            continue;
        }

        graph.add_vertex(i);
        let step_id = steps[i].id.clone();
        let requires = steps[i].requires.clone();

        for require in &requires {
            if let Some(provider_indices) = providers.get(&require.id) {
                for provider in provider_indices {
                    let provider_idx = provider.step_idx;
                    let provider_id = steps[provider_idx].id.clone();

                    steps[provider_idx].dependency_of.push(step_id.clone());
                    steps[i].dependencies.push(provider_id.clone());

                    graph.add_edge(i, provider_idx);
                }
            }
        }
    }

    match graph.stable_toposort() {
        Ok(steps_idx) => Ok(steps_idx.iter().map(|idx| steps[*idx].clone()).collect()),
        Err(cycle_idx) => {
            let cycle = cycle_idx
                .iter()
                .map(|idx| steps[*idx].id.clone())
                .collect::<Vec<String>>();
            bail!(format!(
                "cyclic dependency detected: {}",
                cycle.join(" -> ")
            ));
        }
    }
}

struct ProviderRef {
    step_idx: usize,
    step_id: String,
}

fn fill_providers(
    steps: &[Step],
    providers: &mut HashMap<String, Vec<ProviderRef>>,
) -> anyhow::Result<()> {
    for (i, step) in steps.iter().enumerate() {
        let mut checked_step_providers: HashSet<String> = HashSet::new();

        for provide in &step.provides {
            if checked_step_providers.contains(provide) {
                bail!("duplicated provide '{}' in step '{}'", provide, step.id);
            }

            providers
                .entry(provide.clone())
                .or_default()
                .push(ProviderRef {
                    step_idx: i,
                    step_id: step.id.clone(),
                });
            checked_step_providers.insert(provide.clone());
        }
    }
    Ok(())
}

// to add dependencies of matched filters steps
fn add_dependencies(
    steps: &mut [Step],
    completed_steps_idx: &HashSet<usize>,
    providers: &mut HashMap<String, Vec<ProviderRef>>,
    unknown_requires: &mut HashMap<String, Vec<String>>,
    os_info: &OsInfo,
) -> anyhow::Result<()> {
    let mut stack: Vec<usize> = Vec::new();
    let mut scheduled_steps: HashSet<usize> = HashSet::new();

    for (idx, step) in steps.iter().enumerate() {
        if step.selection_reason == Some(StepSelectionReason::MatchedFilter) {
            scheduled_steps.insert(idx);
        }
    }

    for step in steps.iter_mut() {
        // we need only matched steps, and they are added before skipped - can safely break
        if step.selection_reason != Some(StepSelectionReason::MatchedFilter) {
            break;
        }

        if step.requires.is_empty() {
            continue;
        }

        push_step_dependencies(
            step,
            completed_steps_idx,
            providers,
            unknown_requires,
            &mut stack,
            &mut scheduled_steps,
            os_info,
        )?;
    }

    // iteratively add dependencies of added dependencies
    while let Some(step_idx) = stack.pop() {
        if steps[step_idx].selection_reason.is_none() {
            steps[step_idx].selection_reason = Some(StepSelectionReason::Dependency);
            check_step_env(&steps[step_idx])?;
        }

        push_step_dependencies(
            &mut steps[step_idx],
            completed_steps_idx,
            providers,
            unknown_requires,
            &mut stack,
            &mut scheduled_steps,
            os_info,
        )?;
    }
    Ok(())
}

fn push_step_dependencies(
    step: &mut Step,
    completed_steps_idx: &HashSet<usize>,
    providers: &HashMap<String, Vec<ProviderRef>>,
    unknown_requires: &mut HashMap<String, Vec<String>>,
    stack: &mut Vec<usize>,
    scheduled_steps: &mut HashSet<usize>,
    os_info: &OsInfo,
) -> anyhow::Result<()> {
    // requires whose provider was already handled in a previous run are considered satisfied
    step.requires.retain(|r| match providers.get(&r.id) {
        Some(indexes) => {
            if indexes.len() == 1 {
                !completed_steps_idx.contains(&indexes[0].step_idx)
            } else {
                true
            }
        }
        None => true,
    });

    // the current step is guaranteed to be executed, so we can filter its requires (possible os / when script checks).
    filter_requires(step, os_info)?;
    for require in &step.requires {
        if let Some(provider_steps) = providers.get(&require.id) {
            if provider_steps.len() > 1 {
                let provider_ids: BTreeSet<&str> = provider_steps
                    .iter()
                    .map(|provider| provider.step_id.as_str())
                    .collect();

                if provider_ids.len() == 1 {
                    bail!(
                        "step '{}' is selected {} times, but provides '{}' required by step '{}'; select it only once",
                        provider_steps[0].step_id,
                        provider_steps.len(),
                        require.id,
                        step.id
                    );
                }

                bail!(
                    "multiple filtered steps have the same provide: {}",
                    require.id
                );
            }

            let provider_step_idx = provider_steps[0].step_idx;

            if !scheduled_steps.contains(&provider_step_idx) {
                stack.push(provider_step_idx);
                scheduled_steps.insert(provider_step_idx);
            }
        } else {
            unknown_requires
                .entry(step.id.clone())
                .or_default()
                .push(require.id.clone());
        }
    }
    Ok(())
}

fn filter_requires(step: &mut Step, os_info: &OsInfo) -> anyhow::Result<()> {
    let mut filtered_required: Vec<Require> = Vec::new();

    for require in &step.requires {
        if let Some(os_expr) = &require.os
            && !eval_os_expr(os_expr, os_info)
        {
            continue;
        }

        if require.when_script.is_some() {
            check_step_env(step)?;
            let script = runner::Script::from_step(
                require.when_script.as_ref().unwrap(),
                &step.defaults,
                &step.id,
                &step.with,
            )?;
            let run = run_noninteractive_script(
                &script,
                Path::new(&step.source_file).parent().unwrap(),
                None,
            )
            .context(format!(
                "failed to run require '{}' when-script for step '{}'",
                require.id, step.id
            ))?;

            match run.status {
                ScriptStatus::Success => {}
                ScriptStatus::Failed(_) => continue,
            }
        }

        filtered_required.push(require.clone());
    }

    step.requires = filtered_required;
    Ok(())
}

fn check_for_errors(unknown_requires: &HashMap<String, Vec<String>>) -> anyhow::Result<()> {
    let mut errors: Vec<String> = Vec::new();
    if !unknown_requires.is_empty() {
        let mut unknown_require_errs: Vec<String> = Vec::new();
        for (step_id, requires) in unknown_requires {
            let deps_str = requires.join(", ");
            unknown_require_errs.push(format!("{} -> {}", step_id, deps_str));
        }
        errors.push("unknown requires: ".to_string() + &unknown_require_errs.join(", "));
    }

    if !errors.is_empty() {
        bail!(errors.join(", "));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::utils::filters::StepFilter::{
        ByIds, ByOs, ByPreviousRunState, ByTags, ByWhenScript,
    };
    use crate::commands::utils::filters::{AllFiltersResult, StepFilter};
    use crate::config::expr::Expr;
    use crate::config::{Require, Script, Step, StepSelectionReason};
    use crate::system::os_info::Platform;
    use std::collections::BTreeMap;

    fn make_step(id: &str, requires: Vec<&str>, provides: Vec<&str>) -> Step {
        Step {
            id: id.to_string(),
            requires: requires
                .into_iter()
                .map(|s| Require {
                    id: s.to_string().clone(),
                    os: None,
                    when_script: None,
                })
                .collect(),
            provides: provides.into_iter().map(String::from).collect(),
            ..Default::default()
        }
    }

    fn make_filters_result<'a>(
        filtered_steps: Vec<&'a Step>,
        excluded_steps: Vec<(StepFilter, &'a Step)>,
    ) -> AllFiltersResult<'a> {
        let mut res = AllFiltersResult::new();
        res.filtered_steps = filtered_steps;

        for (filter, step) in excluded_steps {
            res.add_failed_step(step, filter);
        }

        res
    }

    fn make_os_info() -> OsInfo {
        OsInfo {
            platform: Platform::Linux,
            id: None,
            id_like: vec![],
        }
    }

    fn test_selection_reason(
        steps: &[Step],
        filter: Option<fn(&Step) -> bool>,
        target_reason: &StepSelectionReason,
    ) -> anyhow::Result<()> {
        for step in steps {
            if let Some(filter) = filter {
                if !filter(step) {
                    continue;
                }
            }

            if step.selection_reason != Some(target_reason.clone()) {
                bail!(
                    "step {} has selection reason {}, but target is {}",
                    step.id,
                    step.selection_reason.clone().unwrap(),
                    target_reason
                );
            }
        }
        Ok(())
    }

    #[test]
    fn test_single_step() {
        let step = make_step("step1", vec![], vec![]);
        let filter_res = make_filters_result(vec![&step], vec![]);

        let result = toposort_steps(&filter_res, &make_os_info()).unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "step1");
        assert_eq!(
            result[0].selection_reason,
            Some(StepSelectionReason::MatchedFilter)
        );
        test_selection_reason(&result, None, &StepSelectionReason::MatchedFilter).unwrap();
    }

    #[test]
    fn test_with_excluded_steps() {
        let step1 = make_step("step1", vec![], vec![]);
        let step2 = make_step("step2", vec![], vec![]);
        let step3 = make_step("step3", vec![], vec![]);
        let step4 = make_step("step4", vec![], vec![]);
        let filter_res = make_filters_result(
            vec![&step1],
            vec![
                (ByTags, &step2),
                (ByOs, &step3),
                (ByPreviousRunState, &step4),
            ],
        );

        let result = toposort_steps(&filter_res, &make_os_info()).unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "step1");
        assert_eq!(
            result[0].selection_reason,
            Some(StepSelectionReason::MatchedFilter)
        );
        test_selection_reason(&result, None, &StepSelectionReason::MatchedFilter).unwrap();
    }

    #[test]
    fn test_no_dependencies() {
        let step1 = make_step("step1", vec![], vec![]);
        let step2 = make_step("step2", vec![], vec![]);
        let step3 = make_step("step3", vec![], vec![]);
        let filter_res = make_filters_result(vec![&step1, &step2, &step3], vec![]);

        let result = toposort_steps(&filter_res, &make_os_info()).unwrap();

        assert_eq!(result.len(), 3);
        assert_eq!(result[0].id, "step1");
        assert_eq!(result[1].id, "step2");
        assert_eq!(result[2].id, "step3");
        test_selection_reason(&result, None, &StepSelectionReason::MatchedFilter).unwrap();
    }

    #[test]
    //3 depends on 2, 2 depends on 1
    fn test_linear_dependency() {
        let step3 = make_step("step3", vec!["step2_completed"], vec![]);
        let step2 = make_step("step2", vec!["step1_completed"], vec!["step2_completed"]);
        let step1 = make_step("step1", vec![], vec!["step1_completed"]);
        let filter_res = make_filters_result(vec![&step1, &step2, &step3], vec![]);

        let result = toposort_steps(&filter_res, &make_os_info()).unwrap();

        assert_eq!(result[0].id, "step1");
        assert_eq!(result[0].dependency_of.len(), 1);
        assert_eq!(result[0].dependency_of[0], "step2");

        assert_eq!(result[1].id, "step2");
        assert_eq!(result[1].dependency_of.len(), 1);
        assert_eq!(result[1].dependency_of[0], "step3");
        assert_eq!(result[1].dependencies.len(), 1);
        assert_eq!(result[1].dependencies[0], "step1");

        assert_eq!(result[2].id, "step3");
        assert_eq!(result[2].dependency_of.len(), 0);
        assert_eq!(result[2].dependencies.len(), 1);
        assert_eq!(result[2].dependencies[0], "step2");

        test_selection_reason(&result, None, &StepSelectionReason::MatchedFilter).unwrap();
    }

    #[test]
    //3 depends on 2, 2 depends on 1. 1 and 2 was excluded by non-strict filters
    fn test_linear_dependency_with_excluded_steps() {
        let step3 = make_step("step3", vec!["step2_completed"], vec![]);
        let step2 = make_step("step2", vec!["step1_completed"], vec!["step2_completed"]);
        let step1 = make_step("step1", vec![], vec!["step1_completed"]);
        let filter_res = make_filters_result(vec![&step3], vec![(ByTags, &step1), (ByIds, &step2)]);

        let result = toposort_steps(&filter_res, &make_os_info()).unwrap();

        assert_eq!(result[0].id, "step1");
        assert_eq!(result[0].dependency_of.len(), 1);
        assert_eq!(result[0].dependency_of[0], "step2");

        assert_eq!(result[1].id, "step2");
        assert_eq!(result[1].dependency_of.len(), 1);
        assert_eq!(result[1].dependency_of[0], "step3");
        assert_eq!(result[1].dependencies.len(), 1);
        assert_eq!(result[1].dependencies[0], "step1");

        assert_eq!(result[2].id, "step3");
        assert_eq!(result[2].dependency_of.len(), 0);
        assert_eq!(result[2].dependencies.len(), 1);
        assert_eq!(result[2].dependencies[0], "step2");

        test_selection_reason(
            &result,
            Some(|s| s.id == "step3"),
            &StepSelectionReason::MatchedFilter,
        )
        .unwrap();
        test_selection_reason(
            &result,
            Some(|s| s.id != "step3"),
            &StepSelectionReason::Dependency,
        )
        .unwrap();
    }

    #[test]
    //3 depends on 2, 2 depends on 1. 1 was excluded by os filter
    fn test_linear_dependency_with_excluded_by_os_step() {
        let step3 = make_step("step3", vec!["step2_completed"], vec![]);
        let step2 = make_step("step2", vec!["step1_completed"], vec!["step2_completed"]);
        let mut step1 = make_step("step1", vec![], vec!["step1_completed"]);
        step1.os = Some(Expr::Var("windows".to_string()));
        let filter_res = make_filters_result(vec![&step3], vec![(ByOs, &step1), (ByTags, &step2)]);

        let result = toposort_steps(&filter_res, &make_os_info());
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string()
                .contains("unknown requires: step2 -> step1_completed"),
            "unexpected err: {}",
            err.to_string()
        );
    }

    #[test]
    //3 depends on 2, 2 depends on 1. 1 was excluded by when filter
    fn test_linear_dependency_with_excluded_by_when_script_step() {
        let step3 = make_step("step3", vec!["step2_completed"], vec![]);
        let step2 = make_step("step2", vec!["step1_completed"], vec!["step2_completed"]);
        let step1 = make_step("step1", vec![], vec!["step1_completed"]);
        let filter_res =
            make_filters_result(vec![&step3], vec![(ByWhenScript, &step1), (ByTags, &step2)]);

        let result = toposort_steps(&filter_res, &make_os_info());
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string()
                .contains("unknown requires: step2 -> step1_completed"),
            "unexpected err: {}",
            err.to_string()
        );
    }

    #[test]
    //2 depends on 3, 3 depends on 4, 1 and 5 don't require anything and must preserve order
    fn test_dependency_preserve_order() {
        let step5 = make_step("step5", vec![], vec![]);
        let step4 = make_step("step4", vec![], vec!["step4_completed"]);
        let step3 = make_step("step3", vec!["step4_completed"], vec!["step3_completed"]);
        let step2 = make_step("step2", vec!["step3_completed"], vec![]);
        let step1 = make_step("step1", vec![], vec![]);
        let filter_res = make_filters_result(vec![&step1, &step2, &step3, &step4, &step5], vec![]);

        let result = toposort_steps(&filter_res, &make_os_info()).unwrap();

        assert_eq!(result[0].id, "step1");
        assert_eq!(result[1].id, "step4");
        assert_eq!(result[2].id, "step3");
        assert_eq!(result[3].id, "step2");
        assert_eq!(result[4].id, "step5");

        test_selection_reason(&result, None, &StepSelectionReason::MatchedFilter).unwrap();
    }

    #[test]
    fn test_multiple_providers_one_excluded_by_os() {
        let step3 = make_step("step3", vec!["step_completed"], vec![]);
        let step2 = make_step("step2", vec![], vec!["step_completed"]);
        let mut step1 = make_step("step1", vec![], vec!["step_completed"]);
        step1.os = Some(Expr::Var("windows".to_string()));
        let filter_res = make_filters_result(vec![&step2, &step3], vec![(ByOs, &step1)]);

        let result = toposort_steps(&filter_res, &make_os_info()).unwrap();

        assert_eq!(result[1].id, "step3");
        assert_eq!(result[0].dependency_of[0], "step3");

        test_selection_reason(&result, None, &StepSelectionReason::MatchedFilter).unwrap();
    }

    #[test]
    fn test_unknown_require() {
        let step1 = make_step("step1", vec!["unknown"], vec![]);
        let filter_res = make_filters_result(vec![&step1], vec![]);

        let result = toposort_steps(&filter_res, &make_os_info());

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown requires"));
    }

    #[test]
    fn test_cycle_detection() {
        let step3 = make_step("step3", vec!["step2_completed"], vec!["step3_completed"]);
        let step2 = make_step("step2", vec!["step1_completed"], vec!["step2_completed"]);
        let step1 = make_step("step1", vec!["step3_completed"], vec!["step1_completed"]);
        let filter_res = make_filters_result(vec![&step1, &step2, &step3], vec![]);

        let result = toposort_steps(&filter_res, &make_os_info());

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("cyclic dependency"));
    }

    #[test]
    fn test_multiple_steps_with_same_provide() {
        let step1 = make_step("step1", vec!["step_completed"], vec![]);
        let step3 = make_step("step3", vec![], vec!["step_completed"]);
        let step2 = make_step("step2", vec![], vec!["step_completed"]);

        let filter_res = make_filters_result(vec![&step1, &step2, &step3], vec![]);

        let result = toposort_steps(&filter_res, &make_os_info());

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("multiple filtered steps have the same provide: step_completed"));
    }

    #[test]
    fn test_duplicate_required_provider_is_rejected() {
        let provider = make_step("provider", vec![], vec!["artifact"]);
        let consumer = make_step("consumer", vec!["artifact"], vec![]);
        let filter_res = make_filters_result(vec![&provider, &provider, &consumer], vec![]);

        let result = toposort_steps(&filter_res, &make_os_info());

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("step 'provider' is selected 2 times")
                && err.contains("provides 'artifact'")
                && err.contains("required by step 'consumer'"),
            "unexpected err: {err}"
        );
    }

    #[test]
    fn test_duplicate_selected_step_runs_twice() {
        let step = make_step("step", vec![], vec![]);
        let filter_res = make_filters_result(vec![&step, &step], vec![]);

        let result = toposort_steps(&filter_res, &make_os_info()).unwrap();

        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|step| step.id == "step"));
    }

    #[test]
    fn test_duplicate_provider_not_required_runs_twice() {
        let provider = make_step("provider", vec![], vec!["artifact"]);
        let filter_res = make_filters_result(vec![&provider, &provider], vec![]);

        let result = toposort_steps(&filter_res, &make_os_info()).unwrap();

        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|step| step.id == "provider"));
    }

    #[test]
    fn test_requires_when_runs_once_when_provider_is_selected() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("when.log");

        let dependency = Step {
            id: "dependency".to_string(),
            provides: vec!["dependency".to_string()],
            source_file: "/test.yaml".to_string(),
            ..Default::default()
        };
        let provider = Step {
            id: "provider".to_string(),
            provides: vec!["provider".to_string()],
            requires: vec![Require {
                id: "dependency".to_string(),
                os: None,
                when_script: Some(Script {
                    shell: None,
                    code: format!("echo hit >> '{}'", marker.display()),
                }),
            }],
            source_file: "/test.yaml".to_string(),
            ..Default::default()
        };
        let consumer = Step {
            id: "consumer".to_string(),
            requires: vec![Require {
                id: "provider".to_string(),
                os: None,
                when_script: None,
            }],
            source_file: "/test.yaml".to_string(),
            ..Default::default()
        };

        let filter_res =
            make_filters_result(vec![&consumer, &provider], vec![(ByTags, &dependency)]);
        let steps = toposort_steps(&filter_res, &make_os_info()).unwrap();

        let marker_content = std::fs::read_to_string(&marker).unwrap();
        assert_eq!(marker_content, "hit\n");
        assert_eq!(
            steps
                .iter()
                .map(|step| step.id.as_str())
                .collect::<Vec<_>>(),
            vec!["dependency", "provider", "consumer"]
        );
    }

    #[test]
    fn test_filter_requires_when_script_receives_with_vars() {
        // Regression: `requires[].when` scripts are executed by filter_requires and must
        // receive the template variables injected from the step's `with`, exactly like
        // step-level `when` scripts do.
        let mut step = Step {
            id: "consumer".to_string(),
            with: Some(BTreeMap::from([(
                "MEPRIS_REQ_WHEN_VAR_12345".to_string(),
                "foo".to_string(),
            )])),
            requires: vec![Require {
                id: "provider".to_string(),
                os: None,
                when_script: Some(Script {
                    shell: None,
                    code: "[[ \"$MEPRIS_REQ_WHEN_VAR_12345\" == \"foo\" ]]".to_string(),
                }),
            }],
            source_file: "/test.yaml".to_string(),
            ..Default::default()
        };

        filter_requires(&mut step, &make_os_info()).unwrap();

        // The when-script passes only if $MEPRIS_REQ_WHEN_VAR_12345 == "foo", i.e. only
        // if the value from `with` was injected into the script's environment.
        assert_eq!(step.requires.len(), 1);
    }

    #[test]
    fn test_filter_requires() {
        let step1 = Step {
            id: "step1".to_string(),
            requires: vec![],
            provides: vec!["step_completed".to_string()],
            source_file: "/test.yaml".to_string(),
            ..Default::default()
        };
        let step2 = Step {
            id: "step2".to_string(),
            requires: vec![Require {
                id: "step_completed".to_string(),
                os: Some(Expr::Var("windows".to_string())),
                when_script: None,
            }],
            provides: vec![],
            source_file: "/test.yaml".to_string(),
            ..Default::default()
        };
        let step3 = Step {
            id: "step3".to_string(),
            requires: vec![Require {
                id: "step_completed".to_string(),
                os: None,
                when_script: Some(Script {
                    shell: None,
                    code: "exit 1".to_string(),
                }),
            }],
            provides: vec![],
            source_file: "/test.yaml".to_string(),
            ..Default::default()
        };
        let step4 = Step {
            id: "step4".to_string(),
            requires: vec![Require {
                id: "step_completed".to_string(),
                os: None,
                when_script: None,
            }],
            provides: vec![],
            source_file: "/test.yaml".to_string(),
            ..Default::default()
        };

        let filter_res = make_filters_result(vec![&step1, &step2, &step3, &step4], vec![]);

        let steps = toposort_steps(&filter_res, &make_os_info()).unwrap();

        assert_eq!(steps[1].id, "step2");
        assert!(steps[1].requires.is_empty());
        assert!(steps[1].dependencies.is_empty());

        assert_eq!(steps[2].id, "step3");
        assert!(steps[2].requires.is_empty());
        assert!(steps[2].dependencies.is_empty());

        assert_eq!(steps[3].id, "step4");
        assert_eq!(steps[3].requires.len(), 1);
        assert_eq!(steps[3].dependencies, vec!["step1"]);
    }
}
