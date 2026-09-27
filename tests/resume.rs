use mepris::{
    EnvGuard,
    cli::{ResumeArgs, RunArgs},
    commands::{resume, run::handle},
    run_with_cwd, run_with_tracing,
};
use serial_test::serial;
use std::{fs, io};
use tempfile::tempdir;

#[test]
#[serial]
fn test_resume_uses_absolute_path() {
    let dir = tempdir().expect("Failed to create temp dir");
    let file_path = dir.path().join("file.yaml");
    let state_file_path = dir.path().join("state.json");

    fs::write(
        &file_path,
        r#"
        steps:
          - id: "step"
            script: |
              echo "didn't clone bracket
        "#,
    )
    .expect("Failed to write file.yaml");

    let _guard = EnvGuard::new("MEPRIS_STATE_PATH", state_file_path.to_str().unwrap());
    let _guard2 = EnvGuard::new("MEPRIS_TEST_SCRIPT_OUTPUT", "1");
    run_with_cwd(dir.path(), || {
        let res = handle(
            RunArgs {
                file: "./file.yaml".to_string(),
                ..Default::default()
            },
            &mut io::sink(),
        );

        assert!(res.is_err());
    });

    fs::write(
        &file_path,
        r#"
        steps:
          - id: "step"
            script: |
              echo "fixed bracket"
        "#,
    )
    .expect("Failed to write file.yaml");

    let mut output = Vec::new();
    let res = resume::handle(
        ResumeArgs {
            interactive: false,
            dry_run: false,
            show_skipped: false,
            debug: false,
        },
        &mut output,
    );
    let output = String::from_utf8_lossy(&output);

    assert!(
        res.is_ok(),
        "mepris run failed: {}",
        res.as_ref().unwrap_err()
    );
    assert!(
        output.contains("fixed bracket"),
        "output doesn't contain 'fixed bracket': {output}"
    );
}

#[test]
#[serial]
#[cfg(unix)]
fn test_resume_failed_dependency_not_last_step() {
    let dir = tempdir().expect("Failed to create temp dir");
    let parent_path = dir.path().join("parent.yaml");
    let child_path = dir.path().join("child.yaml");
    let state_file_path = dir.path().join("state.json");

    // parser.rs does `steps.extend(own_steps)`, so the child's steps come FIRST
    // and the parent's own steps come LAST. File order: [main, dep].
    fs::write(
        &child_path,
        r#"
        steps:
          - id: "main"
            requires:
              - id: x
            script:
              shell: bash
              run: echo "main-ran"
        "#,
    )
    .expect("Failed to write child.yaml");

    fs::write(
        &parent_path,
        r#"
        includes:
          - child.yaml
        steps:
          - id: "dep"
            provides: [x]
            script:
              shell: bash
              run: exit 1
        "#,
    )
    .expect("Failed to write parent.yaml");

    let _guard = EnvGuard::new("MEPRIS_STATE_PATH", state_file_path.to_str().unwrap());
    let _guard2 = EnvGuard::new("MEPRIS_TEST_SCRIPT_OUTPUT", "1");

    // original run: only "main" is explicitly selected, but "dep" (its provider)
    // is pulled in as a dependency and runs first. "dep" is not the last step
    // overall, so the run order is [dep, main].
    let res = handle(
        RunArgs {
            file: parent_path.to_str().unwrap().to_string(),
            steps: vec!["main".to_string()],
            ..Default::default()
        },
        &mut io::sink(),
    );
    assert!(
        res.is_err(),
        "expected run to fail at step 'dep' (pulled-in dependency)"
    );

    // fix the failing step, then resume
    fs::write(
        &parent_path,
        r#"
        includes:
          - child.yaml
        steps:
          - id: "dep"
            provides: [x]
            script:
              shell: bash
              run: echo "dep-fixed"
        "#,
    )
    .expect("Failed to write parent.yaml");

    let mut output = Vec::new();
    let res = resume::handle(
        ResumeArgs {
            interactive: false,
            dry_run: false,
            show_skipped: false,
            debug: false,
        },
        &mut output,
    );
    let output = String::from_utf8_lossy(&output);

    assert!(
        res.is_ok(),
        "resume failed: {:?}",
        res.as_ref().unwrap_err()
    );
    assert!(
        output.contains("dep-fixed"),
        "expected resume to re-run the failed step 'dep', but nothing ran. \
         Output was: {output}"
    );
    assert!(
        output.contains("main-ran"),
        "expected resume to continue with remaining step 'main' after 'dep', \
         but got output: {output}"
    );
}

#[test]
#[serial]
#[cfg(unix)]
fn test_resume_skips_completed_steps() {
    let dir = tempdir().expect("Failed to create temp dir");
    let file_path = dir.path().join("file.yaml");
    let state_file_path = dir.path().join("state.json");

    fs::write(
        &file_path,
        r#"
        steps:
          - id: "first"
            script:
              shell: bash
              run: echo "first-ran"
          - id: "second"
            script:
              shell: bash
              run: exit 1
        "#,
    )
    .expect("Failed to write file.yaml");

    let _guard = EnvGuard::new("MEPRIS_STATE_PATH", state_file_path.to_str().unwrap());
    let _guard2 = EnvGuard::new("MEPRIS_TEST_SCRIPT_OUTPUT", "1");

    let mut output = Vec::new();
    let res = handle(
        RunArgs {
            file: file_path.to_str().unwrap().to_string(),
            ..Default::default()
        },
        &mut output,
    );
    let output = String::from_utf8_lossy(&output);
    assert!(res.is_err(), "expected run to fail at step 'second'");
    assert_eq!(
        output.matches("first-ran").count(),
        1,
        "'first' should have run exactly once: {output}"
    );

    fs::write(
        &file_path,
        r#"
        steps:
          - id: "first"
            script:
              shell: bash
              run: echo "first-ran"
          - id: "second"
            script:
              shell: bash
              run: echo "second-ran"
        "#,
    )
    .expect("Failed to write file.yaml");

    let mut output = Vec::new();
    let mut res: anyhow::Result<()> = Ok(());
    let trace_output = run_with_tracing(false, || {
        res = resume::handle(
            ResumeArgs {
                interactive: false,
                dry_run: false,
                show_skipped: false,
                debug: false,
            },
            &mut output,
        );
    });
    let output = String::from_utf8_lossy(&output);

    assert!(
        res.is_ok(),
        "resume failed: {:?}",
        res.as_ref().unwrap_err()
    );
    assert!(
        output.contains("second-ran"),
        "expected resume to re-run the failed step 'second', but got output: {output}"
    );
    assert!(
        !output.contains("first-ran"),
        "expected resume to skip the completed step 'first' (no check-script), \
         but it ran again: {output}"
    );
    assert!(
        trace_output
            .as_string()
            .contains("[1/1] Running step 'second'"),
        "expected resume to count only pending steps in progress ('[1/1] Running step 'second''), \
         but got trace output: {}",
        trace_output.as_string()
    );
}

#[test]
#[serial]
#[cfg(unix)]
fn test_resume_does_not_run_completed_dependency() {
    let dir = tempdir().expect("Failed to create temp dir");
    let file_path = dir.path().join("file.yaml");
    let state_file_path = dir.path().join("state.json");

    fs::write(
        &file_path,
        r#"
        steps:
          - id: "dep"
            provides: [x]
            script:
              shell: bash
              run: echo "dep-ran"
          - id: "main"
            requires:
              - id: x
            script:
              shell: bash
              run: exit 1
        "#,
    )
    .expect("Failed to write file.yaml");

    let _guard = EnvGuard::new("MEPRIS_STATE_PATH", state_file_path.to_str().unwrap());
    let _guard2 = EnvGuard::new("MEPRIS_TEST_SCRIPT_OUTPUT", "1");

    // original run: 'dep' (pulled in as a dependency) completes, then 'main' fails
    let mut output = Vec::new();
    let res = handle(
        RunArgs {
            file: file_path.to_str().unwrap().to_string(),
            ..Default::default()
        },
        &mut output,
    );
    let output = String::from_utf8_lossy(&output);
    assert!(res.is_err(), "expected run to fail at step 'main'");
    assert!(
        output.contains("dep-ran"),
        "expected 'dep' to run: {output}"
    );

    // fix the failing step, then resume
    fs::write(
        &file_path,
        r#"
        steps:
          - id: "dep"
            provides: [x]
            script:
              shell: bash
              run: echo "dep-ran"
          - id: "main"
            requires:
              - id: x
            script:
              shell: bash
              run: echo "main-ran"
        "#,
    )
    .expect("Failed to write file.yaml");

    let mut output = Vec::new();
    let mut res: anyhow::Result<()> = Ok(());
    let trace_output = run_with_tracing(false, || {
        res = resume::handle(
            ResumeArgs {
                interactive: false,
                dry_run: false,
                show_skipped: false,
                debug: false,
            },
            &mut output,
        );
    });
    let output = String::from_utf8_lossy(&output);

    assert!(
        res.is_ok(),
        "resume failed: {:?}",
        res.as_ref().unwrap_err()
    );
    assert!(
        output.contains("main-ran"),
        "expected resume to run 'main', but got output: {output}"
    );
    assert!(
        !output.contains("dep-ran"),
        "expected resume to skip the completed dependency 'dep' even though \
         'main' requires it, but it ran again: {output}"
    );
    assert!(
        trace_output
            .as_string()
            .contains("[1/1] Running step 'main'"),
        "expected resume to count only pending steps in progress ('[1/1] Running step 'main''), \
         but got trace output: {}",
        trace_output.as_string()
    );
}

#[test]
#[serial]
#[cfg(unix)]
fn test_resume_does_not_rerun_completed_dependency_with_selected_step() {
    let dir = tempdir().expect("Failed to create temp dir");
    let file_path = dir.path().join("file.yaml");
    let state_file_path = dir.path().join("state.json");

    fs::write(
        &file_path,
        r#"
        steps:
          - id: "dep"
            provides: [x]
            script:
              shell: bash
              run: echo "dep-ran"
          - id: "main"
            requires:
              - id: x
            script:
              shell: bash
              run: exit 1
        "#,
    )
    .expect("Failed to write file.yaml");

    let _guard = EnvGuard::new("MEPRIS_STATE_PATH", state_file_path.to_str().unwrap());
    let _guard2 = EnvGuard::new("MEPRIS_TEST_SCRIPT_OUTPUT", "1");

    // original run: only "main" is explicitly selected via --step, but "dep"
    // (its provider) is pulled in as a dependency, completes, then "main" fails
    let mut output = Vec::new();
    let res = handle(
        RunArgs {
            file: file_path.to_str().unwrap().to_string(),
            steps: vec!["main".to_string()],
            ..Default::default()
        },
        &mut output,
    );
    let output = String::from_utf8_lossy(&output);
    assert!(res.is_err(), "expected run to fail at selected step 'main'");
    assert!(
        output.contains("dep-ran"),
        "expected pulled-in dependency 'dep' to run: {output}"
    );

    // fix the failing selected step, then resume
    fs::write(
        &file_path,
        r#"
        steps:
          - id: "dep"
            provides: [x]
            script:
              shell: bash
              run: echo "dep-ran"
          - id: "main"
            requires:
              - id: x
            script:
              shell: bash
              run: echo "main-ran"
        "#,
    )
    .expect("Failed to write file.yaml");

    let mut output = Vec::new();
    let mut res: anyhow::Result<()> = Ok(());
    let trace_output = run_with_tracing(false, || {
        res = resume::handle(
            ResumeArgs {
                interactive: false,
                dry_run: false,
                show_skipped: false,
                debug: false,
            },
            &mut output,
        );
    });
    let output = String::from_utf8_lossy(&output);

    assert!(
        res.is_ok(),
        "resume failed: {:?}",
        res.as_ref().unwrap_err()
    );
    assert!(
        output.contains("main-ran"),
        "expected resume to run the selected step 'main', but got output: {output}"
    );
    assert!(
        !output.contains("dep-ran"),
        "expected resume to skip the completed pulled-in dependency 'dep' even though \
         'main' requires it, but it ran again: {output}"
    );
    assert!(
        trace_output
            .as_string()
            .contains("[1/1] Running step 'main'"),
        "expected resume to count only pending steps in progress ('[1/1] Running step 'main''), \
         but got trace output: {}",
        trace_output.as_string()
    );
}

#[test]
#[serial]
#[cfg(unix)]
fn test_resume_does_not_run_skipped_dependency() {
    let dir = tempdir().expect("Failed to create temp dir");
    let file_path = dir.path().join("file.yaml");
    let state_file_path = dir.path().join("state.json");

    fs::write(
        &file_path,
        r#"
        steps:
          - id: "dep"
            provides: [x]
            script:
              shell: bash
              run: echo "dep-ran"
          - id: "main"
            requires:
              - id: x
            script:
              shell: bash
              run: exit 1
        "#,
    )
    .expect("Failed to write file.yaml");

    // interactive run: skip 'dep', run 'main' (which fails)
    let (success, output) = run_mepris(
        &["run", "-f", "file.yaml", "-i"],
        "s\nr\n",
        dir.path(),
        &state_file_path,
    );
    assert!(
        !success,
        "expected interactive run to fail at 'main', but it succeeded: {output}"
    );
    assert!(
        !output.contains("Running step 'dep'"),
        "expected 'dep' to be skipped in the interactive run: {output}"
    );

    // fix the failing step, then resume
    fs::write(
        &file_path,
        r#"
        steps:
          - id: "dep"
            provides: [x]
            script:
              shell: bash
              run: echo "dep-ran"
          - id: "main"
            requires:
              - id: x
            script:
              shell: bash
              run: echo "main-ran"
        "#,
    )
    .expect("Failed to write file.yaml");

    // original run was interactive, so resume is interactive too; answer Run
    // for every step it asks about
    let (success, output) = run_mepris(&["resume"], "r\nr\nr\nr\n", dir.path(), &state_file_path);
    assert!(
        success,
        "expected resume to succeed, but it failed: {output}"
    );
    assert!(
        output.contains("main-ran"),
        "expected resume to run 'main', but got output: {output}"
    );
    assert!(
        !output.contains("Running step 'dep'"),
        "expected resume to skip the skipped dependency 'dep' even though \
         'main' requires it, but it ran again: {output}"
    );
    assert!(
        output.contains("[1/1] What do you want to do?"),
        "expected resume to count only pending steps in progress ('[1/1] What do you want to do?'), \
         but got output: {output}"
    );
}

#[test]
#[serial]
#[cfg(unix)]
fn test_resume_accumulates_completed_steps_across_attempts() {
    let dir = tempdir().expect("Failed to create temp dir");
    let file_path = dir.path().join("file.yaml");
    let state_file_path = dir.path().join("state.json");

    let yaml = |b_run: &str, c_run: &str| {
        format!(
            r#"
        steps:
          - id: "a"
            script:
              shell: bash
              run: echo "a-ran"
          - id: "b"
            script:
              shell: bash
              run: {b_run}
          - id: "c"
            script:
              shell: bash
              run: {c_run}
        "#
        )
    };

    let _guard = EnvGuard::new("MEPRIS_STATE_PATH", state_file_path.to_str().unwrap());
    let _guard2 = EnvGuard::new("MEPRIS_TEST_SCRIPT_OUTPUT", "1");

    // attempt 1: 'a' completes, 'b' fails
    fs::write(&file_path, yaml("exit 1", "echo \"c-ran\"")).expect("Failed to write file.yaml");
    let res = handle(
        RunArgs {
            file: file_path.to_str().unwrap().to_string(),
            ..Default::default()
        },
        &mut io::sink(),
    );
    assert!(res.is_err(), "expected run 1 to fail at step 'b'");

    // resume 1: fix 'b', but 'c' fails
    fs::write(&file_path, yaml("echo \"b-ran\"", "exit 1")).expect("Failed to write file.yaml");
    let res = resume::handle(
        ResumeArgs {
            interactive: false,
            dry_run: false,
            show_skipped: false,
            debug: false,
        },
        &mut io::sink(),
    );
    assert!(res.is_err(), "expected resume 1 to fail at step 'c'");

    // resume 2: fix 'c'. 'a' and 'b' completed in previous attempts and
    // must not run again.
    fs::write(&file_path, yaml("echo \"b-ran\"", "echo \"c-ran\""))
        .expect("Failed to write file.yaml");
    let mut output = Vec::new();
    let mut res: anyhow::Result<()> = Ok(());
    let trace_output = run_with_tracing(false, || {
        res = resume::handle(
            ResumeArgs {
                interactive: false,
                dry_run: false,
                show_skipped: false,
                debug: false,
            },
            &mut output,
        );
    });
    let output = String::from_utf8_lossy(&output);

    assert!(
        res.is_ok(),
        "resume failed: {:?}",
        res.as_ref().unwrap_err()
    );
    assert!(
        output.contains("c-ran"),
        "expected resume to run 'c', but got output: {output}"
    );
    assert!(
        !output.contains("a-ran"),
        "expected resume to skip 'a' completed in attempt 1, but it ran again: {output}"
    );
    assert!(
        !output.contains("b-ran"),
        "expected resume to skip 'b' completed in resume 1, but it ran again: {output}"
    );
    assert!(
        trace_output.as_string().contains("[1/1] Running step 'c'"),
        "expected resume to count only pending steps in progress ('[1/1] Running step 'c''), \
         but got trace output: {}",
        trace_output.as_string()
    );
}

#[test]
#[serial]
#[cfg(unix)]
fn test_resume_after_successful_run_does_nothing() {
    let dir = tempdir().expect("Failed to create temp dir");
    let file_path = dir.path().join("file.yaml");
    let state_file_path = dir.path().join("state.json");

    fs::write(
        &file_path,
        r#"
        steps:
          - id: "first"
            script:
              shell: bash
              run: echo "first-ran"
        "#,
    )
    .expect("Failed to write file.yaml");

    let _guard = EnvGuard::new("MEPRIS_STATE_PATH", state_file_path.to_str().unwrap());
    let _guard2 = EnvGuard::new("MEPRIS_TEST_SCRIPT_OUTPUT", "1");

    // original run: succeeds, so the final state must be marked as completed
    let mut output = Vec::new();
    let res = handle(
        RunArgs {
            file: file_path.to_str().unwrap().to_string(),
            ..Default::default()
        },
        &mut output,
    );
    assert!(res.is_ok(), "expected run to succeed");

    let state = fs::read_to_string(&state_file_path).expect("Failed to read state.json");
    assert!(
        state.contains("\"completed\":true"),
        "expected completed state after successful run: {state}"
    );

    // resume after a successful run must do nothing
    let mut output = Vec::new();
    let res = resume::handle(
        ResumeArgs {
            interactive: false,
            dry_run: false,
            show_skipped: false,
            debug: false,
        },
        &mut output,
    );
    let output = String::from_utf8_lossy(&output);

    assert!(
        res.is_ok(),
        "resume failed: {:?}",
        res.as_ref().unwrap_err()
    );
    assert!(
        output.contains("Nothing to resume: last run was successful"),
        "expected 'last run was successful' message, but got output: {output}"
    );
    assert!(
        !output.contains("first-ran"),
        "expected nothing to be re-run after a successful run: {output}"
    );
}

#[test]
#[serial]
#[cfg(unix)]
fn test_resume_dry_run_shows_completed_dependency_as_skipped() {
    let dir = tempdir().expect("Failed to create temp dir");
    let file_path = dir.path().join("file.yaml");
    let state_file_path = dir.path().join("state.json");

    // step3 requires y, step2 provides y and requires x, step1 provides x.
    // Selecting only "step3" pulls in both dependencies; run order is [step1, step2, step3].
    fs::write(
        &file_path,
        r#"
        steps:
          - id: "step1"
            provides: [x]
            script:
              shell: bash
              run: exit 1
          - id: "step2"
            requires:
              - id: x
            provides: [y]
            script:
              shell: bash
              run: exit 1
          - id: "step3"
            requires:
              - id: y
            script:
              shell: bash
              run: echo "step3-ran"
        "#,
    )
    .expect("Failed to write file.yaml");

    let _guard = EnvGuard::new("MEPRIS_STATE_PATH", state_file_path.to_str().unwrap());
    let _guard2 = EnvGuard::new("MEPRIS_TEST_SCRIPT_OUTPUT", "1");

    // original run: only "step3" is selected, both dependencies are pulled in;
    // the run fails at "step1".
    let res = handle(
        RunArgs {
            file: file_path.to_str().unwrap().to_string(),
            steps: vec!["step3".to_string()],
            ..Default::default()
        },
        &mut io::sink(),
    );
    assert!(res.is_err(), "expected run to fail at step 'step1'");

    // fix "step1" and resume: it runs (becoming handled), then the run fails at "step2".
    fs::write(
        &file_path,
        r#"
        steps:
          - id: "step1"
            provides: [x]
            script:
              shell: bash
              run: echo "step1-ran"
          - id: "step2"
            requires:
              - id: x
            provides: [y]
            script:
              shell: bash
              run: exit 1
          - id: "step3"
            requires:
              - id: y
            script:
              shell: bash
              run: echo "step3-ran"
        "#,
    )
    .expect("Failed to write file.yaml");

    let mut output = Vec::new();
    let res = resume::handle(
        ResumeArgs {
            interactive: false,
            dry_run: false,
            show_skipped: false,
            debug: false,
        },
        &mut output,
    );
    let output = String::from_utf8_lossy(&output);
    assert!(res.is_err(), "expected resume to fail at step 'step2'");
    assert!(
        output.contains("step1-ran"),
        "expected resumed run to execute the fixed step 'step1': {output}"
    );

    // dry-run resume: the completed dependency must be listed under the previous-run
    // section and NOT re-listed as a pending step.
    let mut output = Vec::new();
    let res = resume::handle(
        ResumeArgs {
            interactive: false,
            dry_run: true,
            show_skipped: true,
            debug: false,
        },
        &mut output,
    );
    assert!(
        res.is_ok(),
        "dry run failed: {:?}",
        res.as_ref().unwrap_err()
    );
    let output = String::from_utf8_lossy(&output);

    assert!(
        output.contains("⏭️ Skipped steps: already completed in previous run"),
        "expected 'already completed in previous run' section, got output: {output}"
    );
    // 'step1' must appear exactly once — inside the skipped section. If it were
    // re-listed as pending, its step line would add another occurrence.
    assert_eq!(
        output.matches("step1").count(),
        1,
        "expected 'step1' to be listed only in the previous-run skipped section: {output}"
    );
    assert!(
        output.contains("[PULLED DEPENDENCIES]"),
        "expected pending dependency 'step2' to still be planned: {output}"
    );
    assert!(
        output.contains("step2"),
        "expected pending step 'step2' to still be planned: {output}"
    );

    // finally fix "step2" as well; a plain resume completes the whole chain.
    fs::write(
        &file_path,
        r#"
        steps:
          - id: "step1"
            provides: [x]
            script:
              shell: bash
              run: echo "step1-ran"
          - id: "step2"
            requires:
              - id: x
            provides: [y]
            script:
              shell: bash
              run: echo "step2-ran"
          - id: "step3"
            requires:
              - id: y
            script:
              shell: bash
              run: echo "step3-ran"
        "#,
    )
    .expect("Failed to write file.yaml");

    let mut output = Vec::new();
    let res = resume::handle(
        ResumeArgs {
            interactive: false,
            dry_run: false,
            show_skipped: false,
            debug: false,
        },
        &mut output,
    );
    assert!(
        res.is_ok(),
        "expected final resume to succeed after fixing all steps"
    );
    let output = String::from_utf8_lossy(&output);
    assert!(
        output.contains("step2-ran"),
        "expected final resume to run step 'step2': {output}"
    );
    assert!(
        output.contains("step3-ran"),
        "expected final resume to finish with the selected step 'step3': {output}"
    );
}

fn run_mepris(
    args: &[&str],
    stdin_input: &str,
    dir: &std::path::Path,
    state_file: &std::path::Path,
) -> (bool, String) {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_mepris"));
    cmd.args(args)
        .current_dir(dir)
        .env("MEPRIS_STATE_PATH", state_file)
        .env("MEPRIS_TEST_SCRIPT_OUTPUT", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("failed to spawn mepris binary");
    child
        .stdin
        .take()
        .expect("failed to get child stdin")
        .write_all(stdin_input.as_bytes())
        .expect("failed to write to child stdin");
    let out = child.wait_with_output().expect("failed to wait for mepris");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    (out.status.success(), format!("{stdout}{stderr}"))
}
