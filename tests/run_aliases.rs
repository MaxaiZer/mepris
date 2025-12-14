use std::{env, fs};

use mepris::{cli::RunArgs, commands::run::handle};
use serial_test::serial;
use tempfile::tempdir;

#[test]
#[serial]
fn test_run_local_aliases() {
    let dir = tempdir().expect("Failed to create temp dir");
    let file_path = dir.path().join("file.yaml");
    let aliases_path = dir.path().join("pkg_aliases.yaml");
    let mut output = Vec::new();
    unsafe { env::set_var("MEPRIS_FAKE_PACKAGE_MANAGER", "Apt"); }

    fs::write(
        &file_path,
        r#"
        steps:
          - id: "step1"
            packages: ["git"]
        "#,
    )
    .expect("Failed to write file.yaml");

    fs::write(
        &aliases_path,
        r#"
        git:
          apt: git-local
        "#,
    )
    .expect("Failed to write pkg_aliases.yaml");

    let res = handle(
        RunArgs {
            file: file_path.to_str().unwrap().to_string(),
            tags_expr: None,
            steps: vec![],
            start_step_id: None,
            interactive: false,
            dry_run: true,
        },
        &mut output,
    );
    let output = String::from_utf8_lossy(&output);
    unsafe { env::remove_var("MEPRIS_FAKE_PACKAGE_MANAGER"); }

    assert!(res.is_ok());
    assert!(
        output.contains("git-local (using alias)"),
        "output doesn't contain 'git-local (using alias)': {output}"
    );
}

#[test]
#[serial]
fn test_run_local_aliases_wrong_file_name() {
    let dir = tempdir().expect("Failed to create temp dir");
    let file_path = dir.path().join("file.yaml");
    let aliases_path = dir.path().join("aliases.yaml");
    let mut output = Vec::new();
    unsafe { env::set_var("MEPRIS_FAKE_PACKAGE_MANAGER", "Apt"); }

    fs::write(
        &file_path,
        r#"
         steps:
           - id: "step1"
             packages: ["git"]
         "#,
    )
    .expect("Failed to write file.yaml");

    fs::write(
        &aliases_path,
        r#"
         git:
           apt: git-local
         "#,
    )
    .expect("Failed to write aliases.yaml");

    let res = handle(
        RunArgs {
            file: file_path.to_str().unwrap().to_string(),
            tags_expr: None,
            steps: vec![],
            start_step_id: None,
            interactive: false,
            dry_run: true,
        },
        &mut output,
    );
    let output = String::from_utf8_lossy(&output);
    unsafe { env::remove_var("MEPRIS_FAKE_PACKAGE_MANAGER"); }

    assert!(res.is_ok());
    assert!(
        output.contains("git (apt-get)"),
        "output doesn't contain 'git (apt-get)': {output}"
    );
}

#[test]
#[serial]
fn test_run_global_aliases() {
    let dir = tempdir().expect("Failed to create temp dir");
    let file_path = dir.path().join("file.yaml");
    let aliases_path = dir.path().join("folder/aliases.yaml");
    fs::create_dir_all(aliases_path.parent().unwrap())
        .expect("Failed to create folder for aliases.yaml");
    let mut output = Vec::new();
    unsafe {
        env::set_var("MEPRIS_GLOBAL_ALIASES_PATH", aliases_path.to_str().unwrap());
        env::set_var("MEPRIS_FAKE_PACKAGE_MANAGER", "Apt");
    }

    fs::write(
        &file_path,
        r#"
        steps:
          - id: "step1"
            packages: ["git"]
        "#,
    )
    .expect("Failed to write file.yaml");

    fs::write(
        &aliases_path,
        r#"
        git:
          apt: git-global
        "#,
    )
    .expect("Failed to write aliases.yaml");

    let res = handle(
        RunArgs {
            file: file_path.to_str().unwrap().to_string(),
            tags_expr: None,
            steps: vec![],
            start_step_id: None,
            interactive: false,
            dry_run: true,
        },
        &mut output,
    );
    let output = String::from_utf8_lossy(&output);
    unsafe {
        env::remove_var("MEPRIS_GLOBAL_ALIASES_PATH");
        env::remove_var("MEPRIS_FAKE_PACKAGE_MANAGER");
    };

    assert!(res.is_ok());
    assert!(
        output.contains("git-global (using alias)"),
        "output doesn't contain 'git-global (using alias)': {output}"
    );
}

#[test]
#[serial]
fn test_run_local_aliases_override_global() {
    let dir = tempdir().expect("Failed to create temp dir");
    let file_path = dir.path().join("file.yaml");
    let local_aliases_path = dir.path().join("pkg_aliases.yaml");
    let global_aliases_path = dir.path().join("folder/aliases.yaml");
    fs::create_dir_all(global_aliases_path.parent().unwrap())
        .expect("Failed to create folder for aliases.yaml");
    let mut output = Vec::new();
    unsafe {
        env::set_var("MEPRIS_GLOBAL_ALIASES_PATH", global_aliases_path.to_str().unwrap());
        env::set_var("MEPRIS_FAKE_PACKAGE_MANAGER", "Apt");
    };

    fs::write(
        &file_path,
        r#"
        steps:
          - id: "step1"
            packages: ["git"]
        "#,
    )
    .expect("Failed to write file.yaml");

    fs::write(
        &global_aliases_path,
        r#"
        git:
          apt: git-global
        "#,
    )
    .expect("Failed to write aliases.yaml");

    fs::write(
        &local_aliases_path,
        r#"
        git:
          apt: git-local
        "#,
    )
    .expect("Failed to write pkg_aliases.yaml");

    let res = handle(
        RunArgs {
            file: file_path.to_str().unwrap().to_string(),
            tags_expr: None,
            steps: vec![],
            start_step_id: None,
            interactive: false,
            dry_run: true,
        },
        &mut output,
    );
    let output = String::from_utf8_lossy(&output);
    unsafe {
        env::remove_var("MEPRIS_GLOBAL_ALIASES_PATH");
        env::remove_var("MEPRIS_FAKE_PACKAGE_MANAGER");
    };
    
    assert!(res.is_ok());
    assert!(
        output.contains("git-local (using alias)"),
        "output doesn't contain 'git-local (using alias)': {output}"
    );
}

#[test]
#[serial]
fn test_run_aliases_manager_overridden() {
    let dir = tempdir().expect("Failed to create temp dir");
    let file_path = dir.path().join("file.yaml");
    let local_aliases_path = dir.path().join("pkg_aliases.yaml");
    let mut output = Vec::new();
    unsafe {
        env::set_var("MEPRIS_FAKE_PACKAGE_MANAGER", "Apt");
    };

    fs::write(
        &file_path,
        r#"
        steps:
          - id: "step1"
            packages: ["git"]
            package_source: pacman
        "#,
    )
        .expect("Failed to write file.yaml");

    fs::write(
        &local_aliases_path,
        r#"
        git:
          apt: git-apt
          pacman: git-pacman
        "#,
    )
        .expect("Failed to write pkg_aliases.yaml");

    let res = handle(
        RunArgs {
            file: file_path.to_str().unwrap().to_string(),
            tags_expr: None,
            steps: vec![],
            start_step_id: None,
            interactive: false,
            dry_run: true,
        },
        &mut output,
    );
    let output = String::from_utf8_lossy(&output);
    unsafe {
        env::remove_var("MEPRIS_FAKE_PACKAGE_MANAGER");
    };

    assert!(res.is_ok());
    assert!(
        output.contains("git-pacman (using alias)"),
        "output doesn't contain 'git-pacman (using alias)': {output}"
    );
}