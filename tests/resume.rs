use mepris::{
    cli::{ResumeArgs, RunArgs},
    commands::{resume, run::handle},
};
use std::{env, fs, io};
use tempfile::tempdir;

#[test]
fn test_resume_uses_absolute_path() {
    let dir = tempdir().expect("Failed to create temp dir");
    let file_path = dir.path().join("file.yaml");
    let state_file_path = dir.path().join("state.json");

    let original_dir = env::current_dir().expect("Failed to get current dir");
    env::set_current_dir(&dir).expect("Failed to change directory");

    unsafe { env::set_var("STATE_PATH", state_file_path.to_str().unwrap()) };

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

    let res = handle(
        RunArgs {
            file: "./file.yaml".to_string(),
            tags_expr: None,
            steps: vec![],
            start_step_id: None,
            interactive: false,
            dry_run: false,
        },
        &mut io::sink(),
    );

    assert!(res.is_err());

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

    env::set_current_dir(original_dir).expect("Failed to restore directory");

    let mut output = Vec::new();
    let res = resume::handle(
        ResumeArgs {
            interactive: false,
            dry_run: false,
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

    unsafe { env::remove_var("STATE_PATH") };
}
