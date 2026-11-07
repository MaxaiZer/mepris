use mepris::{EnvGuard, cli::RunArgs, commands::run::handle};
use serial_test::serial;
use std::fs;
use tempfile::tempdir;

#[cfg(unix)]
fn write_fake_command(dir: &std::path::Path, name: &str, output: &str) {
    use std::os::unix::fs::PermissionsExt;

    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\necho \"{output}\"\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
fn run_dry(config: &std::path::Path) -> String {
    let mut output = Vec::new();
    handle(
        RunArgs {
            file: config.to_str().unwrap().to_string(),
            dry_run: true,
            ..Default::default()
        },
        &mut output,
    )
    .unwrap();
    let output = strip_ansi_escapes::strip(&output);
    String::from_utf8_lossy(&output).to_string()
}

#[test]
#[cfg(unix)]
#[serial]
fn test_npm_version_spec_matches_installed_package() {
    let dir = tempdir().unwrap();
    let bin_dir = dir.path().join("bin");
    fs::create_dir(&bin_dir).unwrap();
    write_fake_command(&bin_dir, "npm", "/usr/lib/node_modules/typescript");

    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        r#"
        steps:
          - id: with_version
            package_source: npm
            packages: ["typescript@5"]
            script:
              shell: bash
              run: "echo hi"
"#,
    )
    .unwrap();

    let old_path = std::env::var("PATH").unwrap_or_default();
    let _path_guard = EnvGuard::new("PATH", &format!("{}:{old_path}", bin_dir.display()));
    let state_path = dir.path().join("state.json");
    let _state_guard = EnvGuard::new("MEPRIS_STATE_PATH", state_path.to_str().unwrap());

    let output = run_dry(&config);

    assert!(
        output.contains("all packages installed"),
        "unexpected output: {output}"
    );
    assert!(
        !output.contains("Would install packages"),
        "unexpected output: {output}"
    );
}

#[test]
#[cfg(unix)]
#[serial]
fn test_cargo_version_spec_matches_only_exact_package_name() {
    let dir = tempdir().unwrap();
    let bin_dir = dir.path().join("bin");
    fs::create_dir(&bin_dir).unwrap();
    write_fake_command(&bin_dir, "cargo", "ripgrep v14.1.0:");

    let config = dir.path().join("config.yaml");
    let state_path = dir.path().join("state.json");
    let old_path = std::env::var("PATH").unwrap_or_default();
    let _path_guard = EnvGuard::new("PATH", &format!("{}:{old_path}", bin_dir.display()));
    let _state_guard = EnvGuard::new("MEPRIS_STATE_PATH", state_path.to_str().unwrap());

    fs::write(
        &config,
        r#"
        steps:
          - id: cargo_pinned
            package_source: cargo
            packages: ["ripgrep@14"]
            script:
              shell: bash
              run: "echo hi"
"#,
    )
    .unwrap();

    let output = run_dry(&config);
    assert!(
        output.contains("all packages installed"),
        "unexpected output: {output}"
    );

    write_fake_command(&bin_dir, "cargo", "foobar v1.0.0:");
    fs::write(
        &config,
        r#"
        steps:
          - id: cargo_prefix
            package_source: cargo
            packages: ["foo@1"]
            script:
              shell: bash
              run: "echo hi"
"#,
    )
    .unwrap();

    let output = run_dry(&config);
    assert!(
        output.contains("Would install packages foo@1"),
        "unexpected output: {output}"
    );
}
