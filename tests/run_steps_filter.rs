use std::{fs, io};

use mepris::{cli::RunArgs, commands::run::handle};
use tempfile::tempdir;

#[test]
fn test_run_steps_filter_check_exist() {
    let dir = tempdir().expect("Failed to create temp dir");
    let file_path = dir.path().join("file.yaml");

    fs::write(
        &file_path,
        r#"
steps:
  - id: "step1"
  - id: "step2"
"#,
    )
    .expect("Failed to write file.yaml");

    let res = handle(
        RunArgs {
            file: file_path.to_str().unwrap().to_string(),
            tags_expr: None,
            steps: vec!["step3".to_string()],
            start_step_id: None,
            interactive: false,
            dry_run: true,
        },
        &mut io::sink(),
    );
    assert!(res.is_err());
    assert!(res.unwrap_err().to_string().contains("Unknown steps"));
}

#[test]
fn test_run_steps_filter_preserve_order() {
    let dir = tempdir().expect("Failed to create temp dir");
    let file_path = dir.path().join("file.yaml");
    let mut output = Vec::new();

    fs::write(
        &file_path,
        r#"
steps:
  - id: "step1"
  - id: "step2"
"#,
    )
    .expect("Failed to write file.yaml");

    let res = handle(
        RunArgs {
            file: file_path.to_str().unwrap().to_string(),
            tags_expr: None,
            steps: vec!["step2".to_string(), "step1".to_string()],
            start_step_id: None,
            interactive: false,
            dry_run: true,
        },
        &mut output,
    );
    let output = String::from_utf8_lossy(&output);
    assert!(res.is_ok());
    let step2_pos = output.find("Would run step 'step2'").unwrap();
    let step1_pos = output.find("Would run step 'step1'").unwrap();
    assert!(step2_pos < step1_pos, "Expected step2 before step1");
}
