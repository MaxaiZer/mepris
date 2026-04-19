use std::collections::HashSet;
use std::io::{BufRead, Write};

use super::logger::Logger;
use crate::runner::{Step, StepCompletedResult};
use anyhow::Result;
use colored::Colorize;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Decision {
    Run,
    Skip,
    Abort,
    LeaveInteractiveMode,
}

pub trait Interactor {
    fn ask_confirmation(
        &mut self,
        step: &Step,
        has_broken_deps: bool,
        completion: &StepCompletedResult,
        logger: &mut Logger,
    ) -> Result<Decision>;
}

pub struct CliInteractor<R: BufRead> {
    input: R,
}

impl<R: BufRead> CliInteractor<R> {
    pub fn new(input: R) -> Self {
        CliInteractor { input }
    }
}

impl<R: BufRead> Interactor for CliInteractor<R> {
    fn ask_confirmation(
        &mut self,
        step: &Step,
        has_broken_deps: bool,
        completion: &StepCompletedResult,
        logger: &mut Logger,
    ) -> Result<Decision> {
        ask_confirmation(step, has_broken_deps, completion, &mut self.input, logger)
    }
}

const MAX_SCRIPT_LINES: usize = 8;

pub fn ask_confirmation(
    step: &Step,
    has_broken_deps: bool,
    completion: &StepCompletedResult,
    read: &mut impl BufRead,
    logger: &mut Logger,
) -> Result<Decision> {
    let mut cmds = vec![
        "r=Run",
        "s=Skip",
        "a=Abort",
        "l=Run and leave interactive mode",
    ];
    if need_truncate_step_output(step) {
        cmds.push("v=View full step");
    }
    if has_broken_deps {
        cmds[0] = "r=Run anyway";
    }

    let letters: Vec<String> = cmds
        .iter()
        .map(|s| s.split('=').next().unwrap().to_string())
        .collect();

    print_step(step, has_broken_deps, completion, &mut logger.out, false)?;
    logger
        .log_with_progress(|p| format!("\n{p} What do you want to do? ({}): ", cmds.join(", ")))?;

    loop {
        logger.out.flush()?;

        let mut input = String::new();
        read.read_line(&mut input).unwrap();

        input = input.trim().to_lowercase();
        if !letters.contains(&input) {
            logger.log("Invalid input, please try again.")?;
            continue;
        }

        match input.as_str() {
            "r" => return Ok(Decision::Run),
            "s" => return Ok(Decision::Skip),
            "a" => return Ok(Decision::Abort),
            "l" => return Ok(Decision::LeaveInteractiveMode),
            "v" => {
                print_step(step, has_broken_deps, completion, &mut logger.out, true)?;
                logger.log_with_progress(|p| {
                    format!("\n{p} What do you want to do? ({}): ", cmds.join(", "))
                })?;
            }
            _ => logger.log("Invalid input, please try again.")?,
        }
    }
}
fn need_truncate_step_output(step: &Step) -> bool {
    fn is_too_long(code: &str) -> bool {
        code.lines().nth(MAX_SCRIPT_LINES).is_some()
    }

    step.pre_script
        .as_ref()
        .is_some_and(|s| is_too_long(&s.code))
        || step.script.as_ref().is_some_and(|s| is_too_long(&s.code))
}

fn print_step(
    step: &Step,
    has_broken_deps: bool,
    completion: &StepCompletedResult,
    out: &mut dyn Write,
    full: bool,
) -> Result<()> {
    let pkg_manager = &step.package_manager;

    writeln!(out, "step {}", step.id.cyan())?;
    let max_script_lines = match full {
        true => usize::MAX,
        false => MAX_SCRIPT_LINES,
    };

    if let Some(pre_script) = &step.pre_script {
        writeln!(out, "pre_script:")?;
        output_script(&pre_script.code, max_script_lines, out)?;
    }
    if !step.packages.is_empty() {
        let not_installed_pkgs: HashSet<String> = match &completion {
            StepCompletedResult::NotInstalledPackages(pkgs) => {
                HashSet::from_iter(pkgs.iter().cloned())
            }
            _ => HashSet::new(),
        };

        let mut installed: Vec<&str> = Vec::new();
        let mut not_installed: Vec<&str> = Vec::new();

        for pkg in &step.packages {
            if *completion != StepCompletedResult::NotInstalledPackageManager
                && not_installed_pkgs.contains(&pkg.name)
            {
                not_installed.push(pkg.name.as_str());
            } else if *completion != StepCompletedResult::NotInstalledPackageManager {
                installed.push(pkg.name.as_str());
            } else {
                not_installed.push(pkg.name.as_str());
            }
        }

        writeln!(out, "packages ({}):", pkg_manager)?;
        if !installed.is_empty() {
            writeln!(
                out,
                "  {}: {}",
                "already installed".green(),
                installed.join(", ")
            )?;
        }
        if !not_installed.is_empty() {
            writeln!(
                out,
                "  {}: {}",
                "would install".yellow(),
                not_installed.join(", ")
            )?;
        }
    }
    if let Some(script) = &step.script {
        writeln!(out, "script:")?;
        output_script(&script.code, max_script_lines, out)?;
    }

    if !step.dependency_of.is_empty() {
        writeln!(out, "dependency of: {}", step.dependency_of.join(", "))?;
    }

    match completion {
        StepCompletedResult::Completed => {
            writeln!(out, "status: {}", "completed".green())?;
        }
        StepCompletedResult::NotInstalledPackageManager => {
            writeln!(out, "status: {}", "package manager not installed".yellow())?;
        }
        StepCompletedResult::NotInstalledPackages(_) => {}
        StepCompletedResult::FailedCheckScript => {
            writeln!(out, "status: {}", "check-script failed".yellow())?;
        }
        StepCompletedResult::HasScriptWithoutCheck if step.packages.is_empty() => {
            writeln!(
                out,
                "status: {}",
                "completion cannot be verified without a check-script".yellow()
            )?;
        }
        StepCompletedResult::HasScriptWithoutCheck => {
            writeln!(out, "status: {}", "all packages are installed, but completion cannot be verified without a check-script".yellow())?;
        }
    }

    if has_broken_deps {
        writeln!(out, "{}", "[has broken dependencies]".yellow())?;
    }

    Ok(())
}

fn output_script(script: &str, max_lines: usize, out: &mut dyn Write) -> Result<()> {
    let mut iter = script.lines();
    for line in iter.by_ref().take(max_lines) {
        writeln!(out, "{}", line.magenta())?;
    }

    if iter.next().is_some() {
        writeln!(out, "...")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{self, StepSelectionReason::MatchedFilter};
    use crate::runner::script::Script;
    use crate::runner::{Package, Step};
    use crate::system::pkg::PackageManager;
    use crate::system::shell::Shell;
    use std::io::Cursor;

    struct MockInteractor {
        decisions: Vec<Decision>,
    }

    impl Interactor for MockInteractor {
        fn ask_confirmation(
            &mut self,
            _: &Step,
            _: bool,
            _: &StepCompletedResult,
            _: &mut Logger,
        ) -> Result<Decision> {
            if self.decisions.is_empty() {
                panic!("not enough decisions");
            }
            Ok(self.decisions.remove(0))
        }
    }

    struct FakeStateSaver;
    impl crate::runner::StateSaver for FakeStateSaver {
        fn save(&self, _: &crate::runner::RunState) -> Result<()> {
            Ok(())
        }
    }

    struct MockScriptChecker;
    impl crate::runner::script_checker::ScriptChecker for MockScriptChecker {
        fn check_script(&mut self, _: &Script, _: bool) -> Result<()> {
            Ok(())
        }
        fn is_checked(&self, _: &Script) -> bool {
            true
        }
    }

    fn make_test_step() -> config::Step {
        config::Step {
            id: "test-step".to_string(),
            script: Some(config::Script {
                shell: Some(Shell::Bash),
                code: "echo test".to_string(),
            }),
            source_file: "/test.yaml".to_string(),
            selection_reason: Some(MatchedFilter),
            ..Default::default()
        }
    }

    #[test]
    fn test_interactive_skip_step_continues_to_next() -> Result<()> {
        let steps = vec![
            make_test_step(),
            config::Step {
                id: "test-step-2".to_string(),
                script: Some(config::Script {
                    shell: Some(Shell::Bash),
                    code: "echo test2".to_string(),
                }),
                source_file: "/test.yaml".to_string(),
                selection_reason: Some(MatchedFilter),
                ..Default::default()
            },
        ];

        let mut interactor = MockInteractor {
            decisions: vec![Decision::Skip, Decision::Run],
        };
        let mut output = Vec::new();

        let result = crate::runner::run(
            &steps,
            &crate::runner::RunParameters {
                dry_run: false,
                source_file_path: std::path::PathBuf::from("/test.yaml"),
            },
            &FakeStateSaver,
            &mut MockScriptChecker,
            Some(&mut interactor),
            &mut output,
        )?;

        assert!(result.is_none());
        let output_str = String::from_utf8(output).unwrap();
        assert!(
            !output_str.contains("Running step 'test-step'"),
            "unexpected output: {}",
            output_str
        );
        assert!(
            output_str.contains("Running step 'test-step-2'"),
            "unexpected output: {}",
            output_str
        );
        Ok(())
    }

    #[test]
    fn test_interactive_abort() -> Result<()> {
        let steps = vec![make_test_step(), make_test_step()];

        let mut interactor = MockInteractor {
            decisions: vec![Decision::Abort],
        };
        let mut output = Vec::new();

        let result = crate::runner::run(
            &steps,
            &crate::runner::RunParameters {
                dry_run: false,
                source_file_path: std::path::PathBuf::from("/test.yaml"),
            },
            &FakeStateSaver,
            &mut MockScriptChecker,
            Some(&mut interactor),
            &mut output,
        )?;

        let output_str = String::from_utf8(output).unwrap();
        assert!(
            !output_str.contains("Running step"),
            "unexpected output: {}",
            output_str
        );
        Ok(())
    }

    #[test]
    fn test_interactive_leave_mode_continues_without_prompt() -> Result<()> {
        let step = make_test_step();
        let steps = vec![step.clone(), step.clone()];

        let mut interactor = MockInteractor {
            decisions: vec![Decision::LeaveInteractiveMode],
        };
        let mut output = Vec::new();

        let result = crate::runner::run(
            &steps,
            &crate::runner::RunParameters {
                dry_run: false,
                source_file_path: std::path::PathBuf::from("/test.yaml"),
            },
            &FakeStateSaver,
            &mut MockScriptChecker,
            Some(&mut interactor),
            &mut output,
        )?;

        assert!(result.is_none());
        Ok(())
    }

    #[test]
    fn test_interactive_run_step_executes() -> Result<()> {
        let steps = vec![make_test_step()];

        let mut interactor = MockInteractor {
            decisions: vec![Decision::Run],
        };
        let mut output = Vec::new();

        let result = crate::runner::run(
            &steps,
            &crate::runner::RunParameters {
                dry_run: false,
                source_file_path: std::path::PathBuf::from("/test.yaml"),
            },
            &FakeStateSaver,
            &mut MockScriptChecker,
            Some(&mut interactor),
            &mut output,
        )?;

        assert!(result.is_none());
        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("Running step"));
        Ok(())
    }

    #[test]
    fn test_interactive_skip_dependency_and_leave_mode() -> Result<()> {
        let steps = vec![
            config::Step {
                id: "test-step-1".to_string(),
                script: Some(config::Script {
                    shell: Some(Shell::Bash),
                    code: "echo test2".to_string(),
                }),
                source_file: "/test.yaml".to_string(),
                selection_reason: Some(MatchedFilter),
                dependency_of: vec!["test-step-3".to_string()],
                ..Default::default()
            },
            config::Step {
                id: "test-step-2".to_string(),
                script: Some(config::Script {
                    shell: Some(Shell::Bash),
                    code: "echo test2".to_string(),
                }),
                source_file: "/test.yaml".to_string(),
                selection_reason: Some(MatchedFilter),
                ..Default::default()
            },
            config::Step {
                id: "test-step-3".to_string(),
                script: Some(config::Script {
                    shell: Some(Shell::Bash),
                    code: "echo test2".to_string(),
                }),
                source_file: "/test.yaml".to_string(),
                selection_reason: Some(MatchedFilter),
                dependencies: vec!["test-step-1".to_string()],
                ..Default::default()
            },
        ];

        let mut interactor = MockInteractor {
            decisions: vec![Decision::Skip, Decision::LeaveInteractiveMode],
        };
        let mut output = Vec::new();

        let result = crate::runner::run(
            &steps,
            &crate::runner::RunParameters {
                dry_run: false,
                source_file_path: std::path::PathBuf::from("/test.yaml"),
            },
            &FakeStateSaver,
            &mut MockScriptChecker,
            Some(&mut interactor),
            &mut output,
        );

        let output_str = String::from_utf8(output).unwrap();
        assert!(
            output_str.contains("Step 'test-step-2' completed"),
            "unexpected output: {}",
            output_str
        );
        assert!(result.is_err());
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("cannot run step with broken dependencies without interactive mode")
        );
        Ok(())
    }

    #[test]
    fn test_interactive_skip_completed_dependency_and_leave_mode() -> Result<()> {
        let steps = vec![
            config::Step {
                id: "test-step-1".to_string(),
                script: Some(config::Script {
                    shell: Some(Shell::Bash),
                    code: "echo test2".to_string(),
                }),
                check_script: Some(config::Script {
                    shell: Some(Shell::Bash),
                    code: "exit 0".to_string(),
                }),
                source_file: "/test.yaml".to_string(),
                selection_reason: Some(MatchedFilter),
                dependency_of: vec!["test-step-3".to_string()],
                ..Default::default()
            },
            config::Step {
                id: "test-step-2".to_string(),
                script: Some(config::Script {
                    shell: Some(Shell::Bash),
                    code: "echo test2".to_string(),
                }),
                source_file: "/test.yaml".to_string(),
                selection_reason: Some(MatchedFilter),
                dependencies: vec!["test-step-1".to_string()],
                ..Default::default()
            },
        ];

        let mut interactor = MockInteractor {
            decisions: vec![Decision::Skip, Decision::LeaveInteractiveMode],
        };
        let mut output = Vec::new();

        let result = crate::runner::run(
            &steps,
            &crate::runner::RunParameters {
                dry_run: false,
                source_file_path: std::path::PathBuf::from("/test.yaml"),
            },
            &FakeStateSaver,
            &mut MockScriptChecker,
            Some(&mut interactor),
            &mut output,
        )?;

        let output_str = String::from_utf8(output).unwrap();
        assert!(
            output_str.contains("Run completed"),
            "unexpected output: {}",
            output_str
        );
        Ok(())
    }

    #[test]
    fn test_no_interactor() -> Result<()> {
        let step = config::Step {
            id: "test-step".to_string(),
            script: Some(config::Script {
                shell: Some(Shell::Bash),
                code: "echo test".to_string(),
            }),
            source_file: "/test.yaml".to_string(),
            selection_reason: Some(MatchedFilter),
            ..Default::default()
        };

        let mut output = Vec::new();

        let result = crate::runner::run(
            &[step],
            &crate::runner::RunParameters {
                dry_run: false,
                source_file_path: std::path::PathBuf::from("/test.yaml"),
            },
            &FakeStateSaver,
            &mut MockScriptChecker,
            None,
            &mut output,
        )?;

        assert!(result.is_none());
        let output_str = String::from_utf8(output).unwrap();
        assert!(
            output_str.contains("Running step 'test-step'") && output_str.contains("Run completed"),
            "unexpected output: {}",
            output_str
        );
        Ok(())
    }

    #[test]
    fn test_skip_dependency() -> Result<()> {
        let step1 = config::Step {
            id: "test-step".to_string(),
            script: Some(config::Script {
                shell: Some(Shell::Bash),
                code: "echo test".to_string(),
            }),
            source_file: "/test.yaml".to_string(),
            selection_reason: Some(MatchedFilter),
            dependency_of: vec!["test-step2".to_string()],
            ..Default::default()
        };
        let step2 = config::Step {
            id: "test-step2".to_string(),
            script: Some(config::Script {
                shell: Some(Shell::Bash),
                code: "echo test".to_string(),
            }),
            source_file: "/test.yaml".to_string(),
            selection_reason: Some(MatchedFilter),
            dependencies: vec!["test-step".to_string()],
            ..Default::default()
        };
        let mut interactor = MockInteractor {
            decisions: vec![Decision::Skip, Decision::Run],
        };

        let mut output = Vec::new();

        let result = crate::runner::run(
            &[step1, step2],
            &crate::runner::RunParameters {
                dry_run: false,
                source_file_path: std::path::PathBuf::from("/test.yaml"),
            },
            &FakeStateSaver,
            &mut MockScriptChecker,
            Some(&mut interactor),
            &mut output,
        )?;

        assert!(result.is_none());
        let output_str = String::from_utf8(output).unwrap();
        assert!(
            output_str.contains("Running step 'test-step2'")
                && output_str.contains("Run completed"),
            "unexpected output: {}",
            output_str
        );
        Ok(())
    }

    #[test]
    fn test_print_step_basic() -> Result<()> {
        let step = Step {
            id: "my-step".to_string(),
            package_manager: PackageManager::Npm,
            packages: vec![Package {
                name: "pkg1".to_string(),
                used_alias: false,
            }],
            script: Some(Script {
                shell: Shell::Bash,
                code: "echo hello".to_string(),
            }),
            source_file: "/test.yaml".to_string(),
            selection_reason: MatchedFilter,
            ..Default::default()
        };

        let mut output = Vec::new();
        print_step(
            &step,
            false,
            &StepCompletedResult::Completed,
            &mut output,
            false,
        )?;

        let clean = strip_ansi_escapes::strip(output);
        let output_str = String::from_utf8(clean)?;
        assert!(output_str.contains("step my-step"));
        assert!(output_str.contains("script:"));
        assert!(output_str.contains("echo hello"));
        assert!(output_str.contains("packages"));
        assert!(output_str.contains("already installed"));
        assert!(output_str.contains("completed"));
        Ok(())
    }

    #[test]
    fn test_print_dependency() -> Result<()> {
        let step = Step {
            id: "my-step".to_string(),
            source_file: "/test.yaml".to_string(),
            selection_reason: MatchedFilter,
            dependency_of: vec!["my-step2".to_string()],
            ..Default::default()
        };

        let mut output = Vec::new();
        print_step(
            &step,
            false,
            &StepCompletedResult::Completed,
            &mut output,
            false,
        )?;

        let clean = strip_ansi_escapes::strip(output);
        let output_str = String::from_utf8(clean)?;
        assert!(output_str.contains("dependency of: my-step2"));
        Ok(())
    }

    #[test]
    fn test_print_step_truncates_long_script() -> Result<()> {
        let long_script = (0..15)
            .map(|i| format!("line {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let step = Step {
            id: "long-step".to_string(),
            package_manager: PackageManager::Npm,
            script: Some(Script {
                shell: Shell::Bash,
                code: long_script,
            }),
            source_file: "/test.yaml".to_string(),
            selection_reason: MatchedFilter,
            ..Default::default()
        };

        let mut output = Vec::new();
        print_step(
            &step,
            false,
            &StepCompletedResult::Completed,
            &mut output,
            false,
        )?;

        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("line 0"));
        assert!(output_str.contains("line 7"));
        assert!(output_str.contains("..."));
        assert!(!output_str.contains("line 8"));
        Ok(())
    }

    #[test]
    fn test_print_step_full_output() -> Result<()> {
        let long_script = (0..15)
            .map(|i| format!("line {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let step = Step {
            id: "long-step".to_string(),
            package_manager: PackageManager::Npm,
            script: Some(Script {
                shell: Shell::Bash,
                code: long_script,
            }),
            source_file: "/test.yaml".to_string(),
            selection_reason: MatchedFilter,
            ..Default::default()
        };

        let mut output = Vec::new();
        print_step(
            &step,
            false,
            &StepCompletedResult::Completed,
            &mut output,
            true,
        )?;

        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("line 14"));
        assert!(!output_str.contains("..."));
        Ok(())
    }

    #[test]
    fn test_print_step_with_pre_script() -> Result<()> {
        let step = Step {
            id: "step-with-pre".to_string(),
            package_manager: PackageManager::Npm,
            pre_script: Some(Script {
                shell: Shell::Bash,
                code: "pre-install".to_string(),
            }),
            script: Some(Script {
                shell: Shell::Bash,
                code: "main-install".to_string(),
            }),
            source_file: "/test.yaml".to_string(),
            selection_reason: MatchedFilter,
            ..Default::default()
        };

        let mut output = Vec::new();
        print_step(
            &step,
            false,
            &StepCompletedResult::Completed,
            &mut output,
            false,
        )?;

        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("pre_script:"));
        assert!(output_str.contains("pre-install"));
        assert!(output_str.contains("script:"));
        assert!(output_str.contains("main-install"));
        Ok(())
    }

    #[test]
    fn test_print_step_with_not_installed_packages() -> Result<()> {
        let step = Step {
            id: "step-with-pkgs".to_string(),
            package_manager: PackageManager::Npm,
            packages: vec![
                Package {
                    name: "installed-pkg".to_string(),
                    used_alias: false,
                },
                Package {
                    name: "not-installed-pkg".to_string(),
                    used_alias: false,
                },
            ],
            source_file: "/test.yaml".to_string(),
            selection_reason: MatchedFilter,
            ..Default::default()
        };

        let mut output = Vec::new();
        print_step(
            &step,
            false,
            &StepCompletedResult::NotInstalledPackages(vec!["not-installed-pkg".to_string()]),
            &mut output,
            false,
        )?;

        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("already installed"));
        assert!(output_str.contains("installed-pkg"));
        assert!(output_str.contains("would install"));
        assert!(output_str.contains("not-installed-pkg"));
        Ok(())
    }

    #[test]
    fn test_print_step_not_installed_package_manager() -> Result<()> {
        let step = Step {
            id: "test-step".to_string(),
            package_manager: PackageManager::Npm,
            source_file: "/test.yaml".to_string(),
            selection_reason: MatchedFilter,
            ..Default::default()
        };

        let mut output = Vec::new();
        print_step(
            &step,
            false,
            &StepCompletedResult::NotInstalledPackageManager,
            &mut output,
            false,
        )?;

        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("package manager not installed"));
        Ok(())
    }

    #[test]
    fn test_ask_confirmation_run() -> Result<()> {
        let step = Step {
            id: "test-step".to_string(),
            package_manager: PackageManager::Npm,
            source_file: "/test.yaml".to_string(),
            selection_reason: MatchedFilter,
            ..Default::default()
        };

        let mut input = Cursor::new("r\n");
        let mut output = Vec::new();
        let mut logger = Logger::new(1, &mut output);

        let decision = ask_confirmation(
            &step,
            false,
            &StepCompletedResult::Completed,
            &mut input,
            &mut logger,
        )?;

        assert_eq!(decision, Decision::Run);
        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("What do you want to do?"));
        Ok(())
    }

    #[test]
    fn test_ask_confirmation_skip() -> Result<()> {
        let step = Step {
            id: "test-step".to_string(),
            package_manager: PackageManager::Npm,
            source_file: "/test.yaml".to_string(),
            selection_reason: MatchedFilter,
            ..Default::default()
        };

        let mut input = Cursor::new("s\n");
        let mut output = Vec::new();
        let mut logger = Logger::new(1, &mut output);

        let decision = ask_confirmation(
            &step,
            false,
            &StepCompletedResult::Completed,
            &mut input,
            &mut logger,
        )?;

        assert_eq!(decision, Decision::Skip);
        Ok(())
    }

    #[test]
    fn test_ask_confirmation_abort() -> Result<()> {
        let step = Step {
            id: "test-step".to_string(),
            package_manager: PackageManager::Npm,
            source_file: "/test.yaml".to_string(),
            selection_reason: MatchedFilter,
            ..Default::default()
        };

        let mut input = Cursor::new("a\n");
        let mut output = Vec::new();
        let mut logger = Logger::new(1, &mut output);

        let decision = ask_confirmation(
            &step,
            false,
            &StepCompletedResult::Completed,
            &mut input,
            &mut logger,
        )?;

        assert_eq!(decision, Decision::Abort);
        Ok(())
    }

    #[test]
    fn test_ask_confirmation_leave_mode() -> Result<()> {
        let step = Step {
            id: "test-step".to_string(),
            package_manager: PackageManager::Npm,
            source_file: "/test.yaml".to_string(),
            selection_reason: MatchedFilter,
            ..Default::default()
        };

        let mut input = Cursor::new("l\n");
        let mut output = Vec::new();
        let mut logger = Logger::new(1, &mut output);

        let decision = ask_confirmation(
            &step,
            false,
            &StepCompletedResult::Completed,
            &mut input,
            &mut logger,
        )?;

        assert_eq!(decision, Decision::LeaveInteractiveMode);
        Ok(())
    }

    #[test]
    fn test_ask_confirmation_invalid_input() -> Result<()> {
        let step = Step {
            id: "test-step".to_string(),
            package_manager: PackageManager::Npm,
            source_file: "/test.yaml".to_string(),
            selection_reason: MatchedFilter,
            ..Default::default()
        };

        let mut input = Cursor::new("invalid\nr\n");
        let mut output = Vec::new();
        let mut logger = Logger::new(1, &mut output);

        let decision = ask_confirmation(
            &step,
            false,
            &StepCompletedResult::Completed,
            &mut input,
            &mut logger,
        )?;

        assert_eq!(decision, Decision::Run);
        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("Invalid input"));
        Ok(())
    }

    #[test]
    fn test_ask_confirmation_view_full_output() -> Result<()> {
        let long_script = (0..15)
            .map(|i| format!("line {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let step = Step {
            id: "long-step".to_string(),
            package_manager: PackageManager::Npm,
            script: Some(Script {
                shell: Shell::Bash,
                code: long_script,
            }),
            source_file: "/test.yaml".to_string(),
            selection_reason: MatchedFilter,
            ..Default::default()
        };

        let mut input = Cursor::new("v\nr\n");
        let mut output = Vec::new();
        let mut logger = Logger::new(1, &mut output);

        let decision = ask_confirmation(
            &step,
            false,
            &StepCompletedResult::Completed,
            &mut input,
            &mut logger,
        )?;

        assert_eq!(decision, Decision::Run);
        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("What do you want to do?"));
        assert!(output_str.contains("line 14"));
        Ok(())
    }

    #[test]
    fn test_ask_confirmation_shows_step_output() -> Result<()> {
        let step = Step {
            id: "test-step".to_string(),
            package_manager: PackageManager::Npm,
            script: Some(Script {
                shell: Shell::Bash,
                code: "echo test".to_string(),
            }),
            packages: vec![Package {
                name: "mypackage".to_string(),
                used_alias: false,
            }],
            source_file: "/test.yaml".to_string(),
            selection_reason: MatchedFilter,
            ..Default::default()
        };

        let mut input = Cursor::new("r\n");
        let mut output = Vec::new();
        let mut logger = Logger::new(1, &mut output);

        ask_confirmation(
            &step,
            false,
            &StepCompletedResult::Completed,
            &mut input,
            &mut logger,
        )?;

        let clean = strip_ansi_escapes::strip(output);
        let output_str = String::from_utf8(clean)?;
        assert!(output_str.contains("step test-step"));
        assert!(output_str.contains("script:"));
        assert!(output_str.contains("echo test"));
        assert!(output_str.contains("packages"));
        assert!(output_str.contains("mypackage"));
        Ok(())
    }

    #[test]
    fn test_ask_confirmation_broken_dependencies() -> Result<()> {
        let step = Step {
            id: "test-step".to_string(),
            package_manager: PackageManager::Npm,
            script: Some(Script {
                shell: Shell::Bash,
                code: "echo test".to_string(),
            }),
            source_file: "/test.yaml".to_string(),
            selection_reason: MatchedFilter,
            ..Default::default()
        };

        let mut input = Cursor::new("r\n");
        let mut output = Vec::new();
        let mut logger = Logger::new(1, &mut output);

        ask_confirmation(
            &step,
            true,
            &StepCompletedResult::Completed,
            &mut input,
            &mut logger,
        )?;

        let clean = strip_ansi_escapes::strip(output);
        let output_str = String::from_utf8(clean)?;
        assert!(
            output_str.contains("Run anyway"),
            "unexpected output: {}",
            output_str
        );
        assert!(
            output_str.contains("has broken dependencies"),
            "unexpected output: {}",
            output_str
        );
        Ok(())
    }
}
