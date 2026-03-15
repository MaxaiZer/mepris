# Mepris

Declarative environment bootstrapper.

## Config Structure

### includes (optional)

You can split your configuration into multiple YAML files using the `includes` field. For example:

```yaml
includes:
  - terminal.yaml
  - vpn.yaml
```
The steps from included files are loaded in order before the steps from the current file.

### defaults (optional)

You can override the default settings:

```yaml
defaults:
  windows_package_manager: winget # or scoop / choco
  windows_shell: powershell # or pwsh
  linux_shell: bash # or pwsh
  macos_shell: bash # or pwsh
```
These defaults apply to all included config files, unless overridden.

### Step Fields

Every step must contain a unique id field.

Each step supports the following optional fields:  
- `os`: Filters step execution by operating system.
  - `!` — negation
  - `%` — "based on" check (matches both the ID and any entries in ID_LIKE in /etc/os-release on Linux).
  - A distribution name **without** `%` matches only the ID field in /etc/os-release
  - You can combine expressions with `&&` (AND) and `||` (OR).
  - Examples:
    - `%debian` — runs on Debian and Debian-based distributions
    - `!windows && !macos` — skip on Windows and macOS
    - `!%arch || manjaro` — runs on non Arch-based distributions or on Manjaro
- `env`: A list of required environment variables.
Mepris validates that all required variables are set before starting the run (including .env if present).  
- `pre_script`: A script that runs before installing packages or the main script. Typically used to prepare the environment (for example, adding repositories or package sources).
- `when`: A script used as a condition check; if it exits with 0, the step will run, otherwise it will be skipped. This acts as a filter and does not determine whether the step is completed.
- `tags`: List of tags to categorize steps.
- `package_source`: Overrides the default package manager for this step. Possible package managers: `apt`, `dnf`, `pacman`, `flatpak`, `snap`, `zypper`, `brew`, `scoop`, `choco`, `winget`, `cargo`, `npm`. If `aur` is specified, program will use `yay` or `paru` (whichever is available)
- `packages`: List of packages to install via the system or overridden package manager.  
**Note:** If no aliases are defined in `pkg_aliases.yaml`, Mepris passes the package names from `step.packages` directly to the specified package manager, without any automatic translation.
- `script`: The main shell script to execute.  
- `check`: A verification script used to determine whether the step is completed (exit code 0). If a step defines a `script`, it is recommended to provide a `check` script.  
  Without it, program cannot determine the completion state, so the step will not marked as completed in `dry-run` or interactive mode.

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
**Note**: Only three shells are supported: `bash`, `powershell` (legacy), `pwsh` (cross-platform).  
All scripts (`when`, `pre_script`, `script`, `check`) are executed with their working directory set to the folder where their YAML file resides.

### Execution order
- when — check condition (skip step if fails)
- pre_script — run preparation commands before installing packages
- Install packages via the appropriate package manager
- Run the main script
- Run the check script

## .env support

If a .env file exists in the working directory **alongside the main YAML config file**, its variables are automatically loaded (override existing ones).

## Package aliases

Different package managers may use different names for the same package.  
For example, fd is called fd-find in apt, but just fd in pacman.

Instead of cluttering your config with OS checks, define aliases once in a separate file - pkg_aliases.yaml.

**Location**

- **Global:**
  - `~/.config/mepris/pkg_aliases.yaml` (Linux)
  - `~/Library/Application Support/mepris/pkg_aliases.yaml` (macOS)
  - `C:\Users\<User>\AppData\Roaming\mepris\pkg_aliases.yaml`  
  (shared across all configs)
- **Local:** next to your main config file (pkg_aliases.yaml)

If both exist, **local aliases override global ones**.

**Example**
```yaml
# <package_default_name>:
#   <package_source>: <overridden_name_for_this_source>
# See `Step Fields -> package_source` for valid values

fd:
  apt: fd-find
  zypper: fd-find
  dnf: fd-find
```

Now your config stays clean:

```yaml
steps:
  - id: fd
    packages: ["fd"]
```

When you run Mepris on Debian, it will automatically resolve it as:

`apt install fd-find`

## Example Config

```yaml
includes:
  - vpn.yaml

defaults:
  windows_package_manager: scoop

steps:
  - id: terminal-core
    tags: ["terminal"]
    packages: ["ripgrep", "neovim"]

  - id: install-nvidia-pascal-drivers
    os: "%arch"
    when: |  #only if nvidia pascal card
      lspci | grep -i 'NVIDIA.*GP10' >/dev/null 2>&1
    packages:
      [
        "nvidia-580xx-dkms",
        "nvidia-580xx-utils",
        "lib32-nvidia-580xx-utils",
        "nvidia-580xx-settings",
        "opencl-nvidia-580xx",
      ]
    package_source: aur

  - id: install-cron-arch
    os: "%arch"
    packages: ["cronie"]
    script: sudo systemctl enable --now cronie.service
    check: systemctl is-active --quiet cronie.service

  - id: nerd-fonts-windows
    os: "windows"
    tags: ["terminal", "fonts"]
    pre_script: scoop bucket add nerd-fonts
    packages: ["JetBrainsMono-NF"]

  - id: setup-git
    tags: ["git"]
    env: ["GIT_EMAIL", "GIT_NAME"]
    script: |
      git config --global user.email "$GIT_EMAIL"
      git config --global user.name "$GIT_NAME"
```
## CLI Usage

```bash
# Execute steps from configuration file  
mepris run --file config.yaml [--tag TAGS_EXPRESSION] [--step STEP] [--interactive] [--dry-run]

# Resume previously failed run
mepris resume [--interactive] [--dry-run]  

# List steps from config file  
mepris list-steps --file config.yaml [--tag TAG_EXPRESSION]  

# Generate completion for shell
mepris completion <SHELL>
e.g. mepris completion fish > ~/.config/fish/completions/mepris.fish
```
## Installation

```bash
curl -L https://github.com/MaxaiZer/mepris/releases/latest/download/mepris-x86_64-unknown-linux-gnu.zip \
  -o /tmp/mepris.zip \
&& unzip -p /tmp/mepris.zip mepris | sudo tee /usr/local/bin/mepris > /dev/null \
&& sudo chmod +x /usr/local/bin/mepris \
&& rm /tmp/mepris.zip
```
