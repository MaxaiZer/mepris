use mepris::{
    EnvGuard,
    cli::{ResumeArgs, RunArgs},
    commands::{resume, run::handle},
    run_with_cwd,
};
use std::{fs, io};
use tempfile::tempdir;

#[test]
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
