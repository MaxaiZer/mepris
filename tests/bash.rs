use mepris::EnvGuard;
use mepris::cli::{RunArgs};
use std::fs;
use tempfile::tempdir;

#[test]
#[cfg(unix)]
fn test_bash_error_default_stops_on_failure() {
    let _guard = EnvGuard::new("MEPRIS_TEST_SCRIPT_OUTPUT", "1");
    let dir = tempdir().expect("Failed to create temp dir");
    let file_path = dir.path().join("file.yaml");
    let mut output = Vec::new();

    fs::write(
        &file_path,
        r#"
        steps:
          - id: step
            script:
              shell: bash
              run: |
                echo start
                cat NOT_EXISTING_FILE.yaml
                echo should-not-reach
        "#,
    )
    .expect("Failed to write file.yaml");

    let res = mepris::commands::run::handle(
        RunArgs {
            file: file_path.to_str().unwrap().to_string(),
            ..Default::default()
        },
        &mut output,
    );
    let output = String::from_utf8_lossy(&output);
    assert!(res.is_err());
    assert!(output.contains("start"), "unexpected output: {}", output);
    assert!(
        !output.contains("should-not-reach"),
        "unexpected output: {}",
        output
    );
}

#[test]
#[cfg(unix)]
fn test_bash_run_script_from_file_dir() {
    let _guard = EnvGuard::new("MEPRIS_TEST_SCRIPT_OUTPUT", "1");
    let dir = tempdir().expect("Failed to create temp dir");
    let file_path = dir.path().join("file.yaml");
    let mut output = Vec::new();

    fs::write(dir.path().join("file.txt"), "temp file").expect("Failed to write temp file");

    fs::write(
        &file_path,
        r#"
        steps:
          - id: parent
            pre_script:
              shell: bash
              run: cat file.txt
            script:
              shell: bash
              run: cat file.txt
        "#,
    )
    .expect("Failed to write file.yaml");

    let res = mepris::commands::run::handle(
        RunArgs {
            file: file_path.to_str().unwrap().to_string(),
            ..Default::default()
        },
        &mut output,
    );
    let output = String::from_utf8_lossy(&output);
    assert!(res.is_ok(), "run failed: {:?}", res.err());
    assert!(output.contains("temp file"), "unexpected output: {}", output);
}
