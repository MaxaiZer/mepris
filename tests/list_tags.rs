use std::fs;

use mepris::{cli::ListTagsArgs, commands::list_tags::handle};
use tempfile::tempdir;

#[test]
#[cfg(unix)]
fn test_list_tags_only_matching_os() {
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
        ListTagsArgs {
            file: file_path.to_str().unwrap().to_string(),
            tags: vec![],
            all: false,
        },
        &mut output,
    );
    dbg!(&res);
    let output = String::from_utf8_lossy(&output);
    assert!(res.is_ok());
    assert!(!output.contains("tag1"));
    assert!(!output.contains("tag2"));
    assert!(output.contains("tag3"));
    assert!(output.contains("tag4"));
}

#[test]
#[cfg(unix)]
fn test_list_tags_all() {
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
        ListTagsArgs {
            file: file_path.to_str().unwrap().to_string(),
            tags: vec![],
            all: true,
        },
        &mut output,
    );
    let output = String::from_utf8_lossy(&output);
    assert!(res.is_ok());
    assert!(output.contains("tag1"));
    assert!(output.contains("tag2"));
    assert!(output.contains("tag3"));
    assert!(output.contains("tag4"));
}

#[test]
fn test_list_tags_unknown_tag() {
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
        ListTagsArgs {
            file: file_path.to_str().unwrap().to_string(),
            tags: vec!["tag5".to_string()],
            all: true,
        },
        &mut output,
    );
    assert!(res.is_err());
    assert!(res.unwrap_err().to_string().contains("tag5"));
}

#[test]
fn test_list_tags_selected() {
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
        ListTagsArgs {
            file: file_path.to_str().unwrap().to_string(),
            tags: vec!["tag1".to_string()],
            all: true,
        },
        &mut output,
    );
    let output = String::from_utf8_lossy(&output);
    assert!(res.is_ok());
    assert!(output.contains("tag1"));
    assert!(output.contains("step1"));
    assert!(output.contains("step2"));
    assert!(!output.contains("tag2"));
    assert!(!output.contains("tag4"));
    assert!(!output.contains("tag5"));
    assert!(!output.contains("step3"));
}
