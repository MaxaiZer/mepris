---
sidebar_position: 5
---

# Examples

You can see all of this functionality used in my own [dotfiles repository](https://github.com/MaxaiZer/dotfiles/tree/main/mepris).

## Example config

```yaml
includes:
  - vpn.yaml

defaults:
  windows_package_manager: scoop

steps:
  - id: terminal-core
    tags: ["terminal"]
    packages: ["ripgrep", "neovim"]

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
        "opencl-nvidia-580xx",
      ]
    package_source: aur
    provides: ["videodrivers"]

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
        "opencl-nvidia",
      ]
    provides: ["videodrivers"]

  - id: install-gaming-stuff
    os: "linux"
    packages: ["steam","lutris","wine"]
    requires: ["videodrivers"]

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