#![cfg(target_os = "linux")]

use std::fs;

use mepris::{EnvGuard, cli::RunArgs, commands::run::handle};
use serial_test::serial;
use tempfile::tempdir;

#[test]
#[serial]
fn script_only_config_does_not_require_package_manager() {
    let config_dir = tempdir().expect("Failed to create config temp dir");
    let empty_path_dir = tempdir().expect("Failed to create empty PATH temp dir");
    let file_path = config_dir.path().join("file.yaml");

    fs::write(
        &file_path,
        r#"
        steps:
          - id: "script-only"
            script: "echo ok"
        "#,
    )
    .expect("Failed to write file.yaml");

    let _path_guard = EnvGuard::new("PATH", empty_path_dir.path().to_str().unwrap());
    let mut output = Vec::new();

    let res = handle(
        RunArgs {
            file: file_path.to_str().unwrap().to_string(),
            dry_run: true,
            ..Default::default()
        },
        &mut output,
    );

    if let Err(e) = res {
        panic!("script-only config failed without package managers: {e:#}");
    }
    assert!(String::from_utf8_lossy(&output).contains("script-only"));
}

#[test]
#[serial]
fn script_only_config_does_not_use_fake_package_manager_override() {
    let config_dir = tempdir().expect("Failed to create config temp dir");
    let empty_path_dir = tempdir().expect("Failed to create empty PATH temp dir");
    let file_path = config_dir.path().join("file.yaml");

    fs::write(
        &file_path,
        r#"
        steps:
          - id: "script-only"
            script: "echo ok"
        "#,
    )
    .expect("Failed to write file.yaml");

    let _path_guard = EnvGuard::new("PATH", empty_path_dir.path().to_str().unwrap());
    let _pm_guard = EnvGuard::new("MEPRIS_DEFAULT_PACKAGE_MANAGER", "invalid");
    let mut output = Vec::new();

    let res = handle(
        RunArgs {
            file: file_path.to_str().unwrap().to_string(),
            dry_run: true,
            ..Default::default()
        },
        &mut output,
    );

    if let Err(e) = res {
        panic!("script-only config should not parse the package manager override: {e:#}");
    }
}

#[test]
#[cfg(unix)]
#[serial]
fn check_only_step_does_not_require_package_manager() {
    let config_dir = tempdir().expect("Failed to create config temp dir");
    let file_path = config_dir.path().join("file.yaml");
    let marker_path = config_dir.path().join("checked");

    fs::write(
        &file_path,
        format!(
            r#"
        steps:
          - id: "check-only"
            package_source: scoop
            check:
              shell: bash
              run: "echo checked > '{}'"
        "#,
            marker_path.display()
        ),
    )
    .expect("Failed to write file.yaml");

    let mut output = Vec::new();
    let res = handle(
        RunArgs {
            file: file_path.to_str().unwrap().to_string(),
            dry_run: true,
            ..Default::default()
        },
        &mut output,
    );

    if let Err(e) = res {
        panic!("check-only step failed without a package manager: {e:#}");
    }

    let plain_output = strip_ansi_escapes::strip(&output);
    let output = String::from_utf8_lossy(&plain_output);
    assert!(marker_path.exists(), "check script did not run: {output}");
    assert!(
        output.contains("Step check-only completed"),
        "unexpected output: {output}"
    );
}
