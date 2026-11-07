use mepris::{EnvGuard, cli::RunArgs, commands::run::handle};
use serial_test::serial;
use std::fs;
use std::io;
use tempfile::tempdir;

#[cfg(unix)]
fn write_script(path: &std::path::Path, content: &str) {
    use std::os::unix::fs::PermissionsExt;

    fs::write(path, content).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
#[cfg(unix)]
#[serial]
fn test_install_only_missing_packages() {
    let dir = tempdir().unwrap();
    let bin_dir = dir.path().join("bin");
    fs::create_dir(&bin_dir).unwrap();

    write_script(
        &bin_dir.join("cargo"),
        "#!/bin/sh\nprintf 'installed-pkg v1.0.0:\\n    installed-pkg\\n'\n",
    );

    let install_log = dir.path().join("install.log");
    write_script(
        &dir.path().join("record-install"),
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"{}\"\n",
            install_log.display()
        ),
    );

    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        r#"
        steps:
          - id: install
            package_source: cargo
            packages: ["installed-pkg", "missing-pkg"]
            script:
              shell: bash
              run: "echo hi"
        "#,
    )
    .unwrap();

    let old_path = std::env::var("PATH").unwrap_or_default();
    let _path_guard = EnvGuard::new("PATH", &format!("{}:{old_path}", bin_dir.display()));
    let _state_guard = EnvGuard::new(
        "MEPRIS_STATE_PATH",
        dir.path().join("state.json").to_str().unwrap(),
    );
    let _install_guard = EnvGuard::new(
        "MEPRIS_INSTALL_COMMAND",
        dir.path().join("record-install").to_str().unwrap(),
    );

    handle(
        RunArgs {
            file: config.to_str().unwrap().to_string(),
            ..Default::default()
        },
        &mut io::sink(),
    )
    .unwrap();

    let installed = fs::read_to_string(&install_log).unwrap_or_default();
    assert_eq!(
        installed, "missing-pkg\n",
        "install command should only receive missing packages"
    );
}
