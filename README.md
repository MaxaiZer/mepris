# Mepris

Cross-platform declarative system setup tool.

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

Override default values for your setup, such as:

```yaml
defaults:
  windows_package_manager: scoop # or choco; default is winget
```
Defaults are propagated to all included configuration files, unless they explicitly override the same keys.  

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
- `pre_script`: A script that runs before installing packages or the main script.
- `when`: A shell command/script used as a condition check; if it exits with 0, the step will run, otherwise it will be skipped.  
- `tags`: List of tags to categorize steps.
- `package_source`: Overrides the default package manager for this step. Possible package managers: `apt`, `dnf`, `pacman`, `flatpak`, `zypper`, `brew`, `scoop`, `choco`, `winget`. If `aur` is specified, program will use `yay` or `paru` (whichever is available)
- `packages`: List of packages to install via the system or overridden package manager.  
**Note:** If no aliases are defined in `pkg_aliases.yaml`, Mepris passes the package names from `step.packages` directly to the specified package manager, without any automatic translation.
- `script`: The main shell script to execute.  

### Scripts

Default shell for running scripts is bash.

```yaml
script: |
  echo "bash" # will use bash
```

To use different shell, specify it explicitly with the syntax:

```yaml
script:
  shell: pwsh
  run: |
    echo "pwsh" # will use pwsh
```
**Note**: Only two shells are supported: `bash` and `pwsh` (PowerShell Core).  
All scripts (`when`, `pre_script`, `script`) are executed with their working directory set to the folder where their YAML file resides.

### Execution order
- when — check condition (skip step if fails)
- pre_script — run preliminary commands
- Install packages via the appropriate package manager
- Run the main script

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

  - id: yazi-common
    os: "!%debian"
    tags: ["terminal"]
    packages: ["yazi"]

  - id: yazi-debian
    os: "%debian"
    tags: ["terminal"]
    when: |
      if command -v yazi >/dev/null 2>&1; then exit 1; else exit 0; fi
    script: |
      wget -qO yazi.zip https://github.com/sxyazi/yazi/releases/latest/download/yazi-x86_64-unknown-linux-gnu.zip
      unzip -q yazi.zip -d yazi-temp
      sudo mv yazi-temp/*/{ya,yazi} /usr/local/bin
      rm -rf yazi-temp yazi.zip

  - id: nerd-fonts-windows
    os: "windows"
    tags: ["terminal", "fonts"]
    pre_script:
      shell: pwsh
      run: scoop bucket add nerd-fonts
    packages: ["JetBrainsMono-NF"]

  - id: setup-git
    tags: ["git"]
    env: ["GIT_EMAIL", "GIT_NAME"]
    script: |
      git config --global user.email "$GIT_EMAIL"
      git config --global user.name "$GIT_NAME"

  - id: install-anki-arch
    os: "%arch"
    packages: ["anki"]
    package_source: aur
```
## CLI Usage

```bash
mepris run --file config.yaml [--tag TAGS_EXPRESSION] [--step STEP] [--interactive] [--dry-run]  
# Execute steps from configuration file  

mepris resume [--interactive] [--dry-run]  
# Resume previously failed run

mepris list-steps --file config.yaml [--tag TAG_EXPRESSION]  
# List steps from config file  

mepris completion <SHELL>
# Generate completion for shell
```
