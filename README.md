# Mepris

Declarative environment bootstrapper.

## [Documentation](https://maxaizer.github.io/mepris/)

## Quick example сonfig

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

## Installation

```bash
curl -L https://github.com/MaxaiZer/mepris/releases/latest/download/mepris-x86_64-unknown-linux-gnu.zip \
  -o /tmp/mepris.zip \
&& unzip -p /tmp/mepris.zip mepris | sudo tee /usr/local/bin/mepris > /dev/null \
&& sudo chmod +x /usr/local/bin/mepris \
&& rm /tmp/mepris.zip
```
