use std::fs;

use mepris::{cli::ListStepsArgs, commands::list_steps::handle};
use tempfile::tempdir;

#[test]
#[cfg(unix)]
fn test_list_steps_only_matching_os() {
    let dir = tempdir().expect("Failed to create temp dir");
    let file_path = dir.path().join("file.yaml");
    let mut output = Vec::new();

    fs::write(
        &file_path,
        r#"
steps:
  - id: "step-windows"
    os: "windows"
    tags: ["tag-windows"]

  - id: "step-unix"
    tags: ["tag-unix"]
"#,
    )
    .expect("Failed to write file.yaml");

    let res = handle(
        ListStepsArgs {
            file: file_path.to_str().unwrap().to_string(),
            tags_expr: None,
            plain: false,
            all: false,
        },
        &mut output,
    );
    dbg!(&res);
    let output = String::from_utf8_lossy(&output);
    assert!(res.is_ok());
    assert!(!output.contains("step-windows"));
    assert!(!output.contains("tag-windows"));
    assert!(output.contains("step-unix"));
    assert!(output.contains("tag-unix"));
}

#[test]
#[cfg(unix)]
fn test_list_steps_all() {
    let dir = tempdir().expect("Failed to create temp dir");
    let file_path = dir.path().join("file.yaml");
    let mut output = Vec::new();

    fs::write(
        &file_path,
        r#"
steps:
  - id: "step-windows"
    os: "windows"
    tags: ["tag-windows"]

  - id: "step-unix"
    tags: ["tag-unix"]
"#,
    )
    .expect("Failed to write file.yaml");

    let res = handle(
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
    assert!(output.contains("step-windows"));
    assert!(output.contains("step-unix"));
    assert!(output.contains("step-windows"));
    assert!(output.contains("step-unix"));
}

#[test]
fn test_list_steps_unknown_tag() {
    let dir = tempdir().expect("Failed to create temp dir");
    let file_path = dir.path().join("file.yaml");
    let mut output = Vec::new();

    fs::write(
        &file_path,
        r#"
steps:
  - id: "step1"
    os: "windows"
    tags: ["tag1", "tag2"]

  - id: "step2"
    tags: [tag3", "tag4"]
"#,
    )
    .expect("Failed to write file.yaml");

    let res = handle(
        ListStepsArgs {
            file: file_path.to_str().unwrap().to_string(),
            tags_expr: Some("tag5".to_string()),
            plain: false,
            all: false,
        },
        &mut output,
    );
    assert!(res.is_err());
    assert!(res.unwrap_err().to_string().contains("tag5"));
}

#[test]
fn test_list_steps_no_selected_tags() {
    let dir = tempdir().expect("Failed to create temp dir");
    let file_path = dir.path().join("file.yaml");
    let mut output = Vec::new();

    fs::write(
        &file_path,
        r#"
steps:
  - id: "step1"
    tags: ["tag1", "tag2"]

  - id: "step2"
    tags: ["tag1", "tag4"]

  - id: "step3"
    tags: ["tag5"]
"#,
    )
    .expect("Failed to write file.yaml");

    let res = handle(
        ListStepsArgs {
            file: file_path.to_str().unwrap().to_string(),
            tags_expr: None,
            plain: false,
            all: false,
        },
        &mut output,
    );
    let output = String::from_utf8_lossy(&output);
    assert!(res.is_ok());
    assert!(output.contains("all tags:"));
}

#[test]
fn test_list_steps_with_selected_tags() {
    let dir = tempdir().expect("Failed to create temp dir");
    let file_path = dir.path().join("file.yaml");
    let mut output = Vec::new();

    fs::write(
        &file_path,
        r#"
steps:
  - id: "step1"
    tags: ["tag1", "tag2"]

  - id: "step2"
    tags: ["tag1", "tag4"]

  - id: "step3"
    tags: ["tag5"]
"#,
    )
    .expect("Failed to write file.yaml");

    let res = handle(
        ListStepsArgs {
            file: file_path.to_str().unwrap().to_string(),
            tags_expr: Some("tag1".to_string()),
            plain: false,
            all: false,
        },
        &mut output,
    );
    let output = String::from_utf8_lossy(&output);
    assert!(res.is_ok());
    assert!(!output.contains("all tags:"));
    assert!(output.contains("tag1"));
    assert!(output.contains("step1"));
    assert!(output.contains("step2"));
    assert!(!output.contains("step3"));
}

#[test]
fn test_list_steps_one_file_no_file_header() {
    let dir = tempdir().expect("Failed to create temp dir");
    let file_path = dir.path().join("file.yaml");
    let mut output = Vec::new();

    fs::write(
        &file_path,
        r#"
steps:
  - id: "step1"
    tags: ["tag1", "tag2"]
"#,
    )
    .expect("Failed to write file.yaml");

    let res = handle(
        ListStepsArgs {
            file: file_path.to_str().unwrap().to_string(),
            tags_expr: Some("tag1".to_string()),
            plain: false,
            all: false,
        },
        &mut output,
    );
    let output = String::from_utf8_lossy(&output);
    assert!(res.is_ok());
    assert!(!output.contains("file"));
}

#[test]
fn test_list_steps_multiple_files_has_file_header() {
    let dir = tempdir().expect("Failed to create temp dir");
    let file_path = dir.path().join("file.yaml");
    let child_path = dir.path().join("child.yaml");
    let mut output = Vec::new();

    fs::write(
        &file_path,
        r#"
includes:
  - child.yaml
steps:
  - id: "step1"
    tags: ["tag1", "tag2"]
"#,
    )
    .expect("Failed to write file.yaml");

    fs::write(
        &child_path,
        r#"
steps:
  - id: "step2"
    tags: ["tag1", "tag2"]
"#,
    )
    .expect("Failed to write child.yaml");

    let res = handle(
        ListStepsArgs {
            file: file_path.to_str().unwrap().to_string(),
            tags_expr: None,
            plain: false,
            all: false,
        },
        &mut output,
    );
    let output = String::from_utf8_lossy(&output);
    assert!(res.is_ok());
    assert!(output.contains("file"));
    assert!(output.contains("file.yaml"));
    assert!(output.contains("child.yaml"));
}

#[test]
fn test_list_steps_multiple_files_has_file_header2() {
    let dir = tempdir().expect("Failed to create temp dir");
    let file_path = dir.path().join("file.yaml");
    let child_path = dir.path().join("child.yaml");
    let mut output = Vec::new();

    fs::write(
        &file_path,
        r#"
includes:
  - child.yaml
"#,
    )
    .expect("Failed to write file.yaml");

    fs::write(
        &child_path,
        r#"
steps:
  - id: "step2"
    tags: ["tag1", "tag2"]
"#,
    )
    .expect("Failed to write child.yaml");

    let res = handle(
        ListStepsArgs {
            file: file_path.to_str().unwrap().to_string(),
            tags_expr: None,
            plain: false,
            all: false,
        },
        &mut output,
    );
    let output = String::from_utf8_lossy(&output);
    assert!(res.is_ok());
    assert!(output.contains("file"));
    assert!(output.contains("child.yaml"));
}
