use mepris::{cli::RunArgs, commands::run::handle};
use std::{env, fs, io};
use tempfile::tempdir;

#[test]
fn test_env_not_exists() {
    let dir = tempdir().expect("Failed to create temp dir");
    let file_path = dir.path().join("file.yaml");

    fs::write(
        &file_path,
        r#"
steps:
  - id: "step1"
    env: ["PATH1"]
    script: |
      end=$((SECONDS+10))
      i=1

      while [ $SECONDS -lt $end ]; do
        i=$((i+1))
        echo $PATH1
        sleep 1
      done
"#,
    )
    .expect("Failed to write file.yaml");

    let res = handle(
        RunArgs {
            file: file_path.to_str().unwrap().to_string(),
            tags_expr: None,
            steps: vec![],
            start_step_id: None,
            interactive: false,
            dry_run: true,
        },
        &mut io::sink(),
    );
    assert!(res.is_err_and(|e| {
        let str = e.to_string();
        str.contains("PATH1") && str.contains("step1")
    }));
}

#[test]
fn test_env_exists() {
    let dir = tempdir().expect("Failed to create temp dir");
    let file_path = dir.path().join("file.yaml");

    fs::write(
        &file_path,
        r#"
steps:
  - id: "step1"
    env: ["PATH2"]
    script: |
      end=$((SECONDS+10))
      i=1

      while [ $SECONDS -lt $end ]; do
        i=$((i+1))
        echo $PATH2
        sleep 1
      done
"#,
    )
    .expect("Failed to write file.yaml");

    unsafe { env::set_var("PATH2", "something") };

    let res = handle(
        RunArgs {
            file: file_path.to_str().unwrap().to_string(),
            tags_expr: None,
            steps: vec![],
            start_step_id: None,
            interactive: false,
            dry_run: true,
        },
        &mut io::sink(),
    );
    assert!(res.is_ok());
}

#[test]
fn test_env_from_file() {
    let dir = tempdir().expect("Failed to create temp dir");
    let file_path = dir.path().join("file.yaml");
    let env_file_path = dir.path().join(".env");

    fs::write(
        &file_path,
        r#"
steps:
  - id: "step1"
    env: ["PATH3"]
    script: |
      end=$((SECONDS+10))
      i=1

      while [ $SECONDS -lt $end ]; do
        i=$((i+1))
        echo $PATH3
        sleep 1
      done
"#,
    )
    .expect("Failed to write file.yaml");

    fs::write(&env_file_path, "PATH3=what").expect("Failed to write .env");

    let res = handle(
        RunArgs {
            file: file_path.to_str().unwrap().to_string(),
            tags_expr: None,
            steps: vec![],
            start_step_id: None,
            interactive: false,
            dry_run: true,
        },
        &mut io::sink(),
    );
    assert!(res.is_ok());
}

#[test]
fn test_env_from_file_overrides_existing() {
    let dir = tempdir().expect("Failed to create temp dir");
    let file_path = dir.path().join("file.yaml");
    let env_file_path = dir.path().join(".env");
    let state_file_path = dir.path().join("state.json");
    let mut output = Vec::new();

    fs::write(
        &file_path,
        r#"
steps:
  - id: "step1"
    env: ["PATH4"]
    script: |
        echo "PATH4=$PATH4"
"#,
    )
    .expect("Failed to write file.yaml");

    unsafe {
        env::set_var("MEPRIS_STATE_PATH", state_file_path.to_str().unwrap());
        env::set_var("PATH4", "old_value")
    };
    fs::write(&env_file_path, "PATH4=new_value").expect("Failed to write .env");

    let res = handle(
        RunArgs {
            file: file_path.to_str().unwrap().to_string(),
            tags_expr: None,
            steps: vec![],
            start_step_id: None,
            interactive: false,
            dry_run: false,
        },
        &mut output,
    );
    let output = String::from_utf8_lossy(&output);
    assert!(res.is_ok());
    assert!(output.contains("PATH4=new_value"));
}

#[test]
fn test_invalid_env_file() {
    let dir = tempdir().expect("Failed to create temp dir");
    let file_path = dir.path().join("file.yaml");
    let env_file_path = dir.path().join(".env");

    fs::write(
        &file_path,
        r#"
steps:
  - id: "step1"
    env: ["PATH5"]
    script: |
        echo "PATH5=$PATH5"
"#,
    )
    .expect("Failed to write file.yaml");

    fs::write(&env_file_path, "PATH5=\"new value").expect("Failed to write .env");

    let res = handle(
        RunArgs {
            file: file_path.to_str().unwrap().to_string(),
            tags_expr: None,
            steps: vec![],
            start_step_id: None,
            interactive: false,
            dry_run: false,
        },
        &mut io::sink(),
    );
    assert!(res.is_err());
    assert!(res.unwrap_err().to_string().contains("Failed to load .env"));
}
