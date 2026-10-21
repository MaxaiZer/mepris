use mepris::{
    EnvGuard,
    cli::{ListStepsArgs, RunArgs},
    commands::{list_steps::handle as list_steps_handle, run::handle as run_handle},
};
use serial_test::serial;
use std::{fs, io};
use tempfile::tempdir;

#[test]
#[serial]
fn test_extends_env_injection() {
    let dir = tempdir().expect("Failed to create temp dir");
    let file_path = dir.path().join("file.yaml");
    let state_file_path = dir.path().join("state.json");
    let mut output = Vec::new();

    fs::write(
        &file_path,
        r#"
        steps:
          - id: "echo-step"
            env: [A, B]
            script: |
              echo "A=$A B=$B"

          - id: "consumer1"
            extends: echo-step
            env: [B]
            with:
              A: value_a
        "#,
    )
    .expect("Failed to write file.yaml");

    let _guard = EnvGuard::new("MEPRIS_TEST_SCRIPT_OUTPUT", "1");
    let _guard2 = EnvGuard::new("MEPRIS_STATE_PATH", state_file_path.to_str().unwrap());
    let _guard3 = EnvGuard::new("B", "value_b");

    let res = run_handle(
        RunArgs {
            file: file_path.to_str().unwrap().to_string(),
            steps: vec!["consumer1".to_string()],
            ..Default::default()
        },
        &mut output,
    );
    let output = String::from_utf8_lossy(&output);
    assert!(res.is_ok(), "{}", res.as_ref().unwrap_err());
    // A comes from 'with' (injected), B comes from the runtime environment
    // because it is declared in the consumer's own env.
    assert!(output.contains("A=value_a B=value_b"));
    // The referenced step itself is not selected, so its script never runs.
    assert!(!output.contains("echo-step"));
}

#[test]
fn test_extends_missing_with_var_error() {
    let dir = tempdir().expect("Failed to create temp dir");
    let file_path = dir.path().join("file.yaml");

    fs::write(
        &file_path,
        r#"
        steps:
          - id: "echo-step"
            env: [A]
            script: |
              echo "$A"

          - id: "consumer1"
            extends: echo-step
            with:
              A: "${MEPRIS_TPL_UNSET_12345}"
        "#,
    )
    .expect("Failed to write file.yaml");

    let res = run_handle(
        RunArgs {
            file: file_path.to_str().unwrap().to_string(),
            dry_run: true,
            steps: vec!["consumer1".to_string()],
            ..Default::default()
        },
        &mut io::sink(),
    );
    assert!(res.is_err_and(|e| {
        let str = e.to_string();
        str.contains("MEPRIS_TPL_UNSET_12345") && str.contains("consumer1")
    }));
}

#[test]
fn test_extends_visible_in_list_steps() {
    let dir = tempdir().expect("Failed to create temp dir");
    let file_path = dir.path().join("file.yaml");
    let mut output = Vec::new();

    fs::write(
        &file_path,
        r#"
        steps:
          - id: "echo-step"
            env: [A]
            script: |
              echo "$A"

          - id: "consumer1"
            extends: echo-step
            with:
              A: value_a
        "#,
    )
    .expect("Failed to write file.yaml");

    let res = list_steps_handle(
        ListStepsArgs {
            file: file_path.to_str().unwrap().to_string(),
            tags_expr: None,
            plain: false,
            all: true,
        },
        &mut output,
    );
    let output = String::from_utf8_lossy(&output);
    assert!(res.is_ok());
    // A referenced step is a regular step: it stays in the list alongside its consumer.
    assert!(output.contains("echo-step"));
    assert!(output.contains("consumer1"));
}

#[test]
#[serial]
fn test_extends_when_script_receives_vars() {
    let dir = tempdir().expect("Failed to create temp dir");
    let file_path = dir.path().join("file.yaml");
    let state_file_path = dir.path().join("state.json");
    let mut output = Vec::new();

    fs::write(
        &file_path,
        r#"
        steps:
          - id: "when-base"
            env: [A]
            script: |
              echo "when-ok-$A"

          - id: "consumer-when"
            extends: when-base
            with:
              A: whenvalue
            when: "[[ \"$A\" == \"whenvalue\" ]]"
        "#,
    )
    .expect("Failed to write file.yaml");

    let _guard = EnvGuard::new("MEPRIS_TEST_SCRIPT_OUTPUT", "1");
    let _guard2 = EnvGuard::new("MEPRIS_STATE_PATH", state_file_path.to_str().unwrap());

    // No process-env guard for A: if the when-script (or the main script) did not
    // receive A via 'with', $A would be empty and the step would be filtered out.
    let res = run_handle(
        RunArgs {
            file: file_path.to_str().unwrap().to_string(),
            steps: vec!["consumer-when".to_string()],
            ..Default::default()
        },
        &mut output,
    );
    let output = String::from_utf8_lossy(&output);
    assert!(res.is_ok(), "{}", res.as_ref().unwrap_err());
    assert!(output.contains("when-ok-whenvalue"));
}
