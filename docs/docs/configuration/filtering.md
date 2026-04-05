---
sidebar_position: 2
---

# Filtering

## By OS
Step has optional `os` field that filters execution by operating system
  - `!` — negation
  - `%` — "based on" check (matches both the ID and any entries in ID_LIKE in /etc/os-release on Linux).
  - A distribution name **without** `%` matches only the ID field in /etc/os-release
  - You can combine expressions with `&&` (AND) and `||` (OR).
  - Examples:
      - `%debian` — runs on Debian and Debian-based distributions
      - `!windows && !macos` — skip on Windows and macOS
      - `!%arch || manjaro` — runs on non Arch-based distributions or on Manjaro

## By script
Step has optional `when` field to define script that used as a condition check; if it exits with 0, the step will run, otherwise it will be skipped.  
See [scripts](config-structure.md#scripts) for possible fields
:::warning
All when-scripts are executed at the start of run. So:
- If they use a non-default system shell, make sure it is installed first.  
- You can't use when-script to depend on another step result. Use [requires/provides](dependencies.md)  

They are also executed in dry-run, so they **must not** create side effects.
:::

Examples:

```
- id: install-nvidia-pascal-drivers #for 10xx series
  os: "arch || endeavouros"
  when: |
    lspci | grep -i 'NVIDIA.*GP10' >/dev/null 2>&1
  packages:
    [
      "nvidia-580xx-dkms",
      "nvidia-580xx-utils",
      "lib32-nvidia-580xx-utils",
      "nvidia-580xx-settings",
      "opencl-nvidia-580xx"
    ]
  package_source: aur

- id: install-nvidia-drivers
  os: "linux"
  when: |
    lspci | grep -i 'nvidia' >/dev/null 2>&1 && \
      ! lspci | grep -i 'NVIDIA.*GP10' >/dev/null 2>&1
  packages:
    [
      "nvidia-dkms",
      "nvidia-utils",
      "lib32-nvidia-utils",
      "nvidia-settings",
      "opencl-nvidia"
    ]
```