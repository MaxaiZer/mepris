---
sidebar_position: 1
---

# Config structure

### Includes (optional)

You can split your configuration into multiple YAML files using the `includes` field. For example:

```yaml
includes:
  - terminal.yaml
  - vpn.yaml
```

### Defaults (optional)

You can override the default settings:

```yaml
defaults:
  windows_package_manager: winget # or scoop / choco
  windows_shell: powershell # or pwsh
  linux_shell: bash # or pwsh
  macos_shell: bash # or pwsh
```
These defaults apply to all included config files, unless overridden.

### Steps

Every step must contain a unique `id` field.

Each step supports the following **optional** fields:
- `os`: Filters step execution by operating system (see [Filtering by os](filtering.md#by-os)).
- `env`: A list of required environment variables. Program validates that all required environment variables are set before starting the run.
- `pre_script`: A script that runs before installing packages or the main script. Purpose: prepare the environment for installing packages (for example, adding repositories or package sources).
- `when`: An arbitrary script-filter (see [Filtering by script](filtering.md#by-script))
- `tags`: List of tags to categorize steps.
- `package_source`: Overrides the default package manager for this step. Possible package managers: `apt`, `dnf`, `pacman`, `flatpak`, `snap`, `zypper`, `brew`, `scoop`, `choco`, `winget`, `cargo`, `npm`. If `aur` is specified, program will use `yay` or `paru` (whichever is available)
- `packages`: List of packages to install via the system or overridden package manager. Can use [Package aliases](package-aliases.md)
- `script`: The main shell script to execute.
- `check`: A verification script used to determine whether the step is completed. (see [Step completion](dependencies.md#step-completion))

### Scripts

Default shell for running scripts is bash for Linux/macOS and powershell (the built-in legacy one) for Windows.

```yaml
script: |
  echo "bash" # shell will depend on current OS
```

To use different shell, specify it explicitly with the syntax:

```yaml
script:
  shell: pwsh
  run: |
    echo "pwsh" # will use pwsh
```
:::note
Only three shells are supported: `bash`, `powershell` (legacy), `pwsh` (cross-platform).  
:::
All scripts (`when`, `pre_script`, `script`, `check`) are executed with their working directory set to the folder where their YAML file resides.

### Execution order

After step is filtered by tags / OS / when-script, it executes like this:

- Run the pre-script
- Install packages via the appropriate package manager
- Run the main script
- Run the check-script

## .env support

If a .env file exists in the working directory **alongside the main YAML config file**, its variables are automatically loaded (override existing ones).