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
  - id: neovim-linux-helpers
    os: "linux"
    packages: ["wl-clipboard"]
    provides: ["neovim-linux-helpers"]

  - id: neovim
    tags: ["terminal"]
    packages: ["neovim"]
    requires:
      - id: neovim-linux-helpers
        os: "linux"
      - nerd-fonts
    provides: ["nvim"]

  - id: nerd-fonts-windows
    os: "windows"
    tags: ["terminal", "fonts"]
    pre_script: scoop bucket add nerd-fonts
    packages: ["JetBrainsMono-NF"]
    provides: ["nerd-fonts"]

  - id: install-cron-arch
    os: "%arch"
    packages: ["cronie"]
    script: sudo systemctl enable --now cronie.service
    check: systemctl is-active --quiet cronie.service

  - id: setup-git
    tags: ["git"]
    env: ["GIT_EMAIL", "GIT_NAME"]
    script: |
      git config --global user.email "$GIT_EMAIL"
      git config --global user.name "$GIT_NAME"
```

## Installation

🐧
```bash
curl -L https://github.com/MaxaiZer/mepris/releases/latest/download/mepris-x86_64-unknown-linux-gnu.zip \
  -o /tmp/mepris.zip \
&& unzip -p /tmp/mepris.zip mepris | sudo tee /usr/local/bin/mepris > /dev/null \
&& sudo chmod +x /usr/local/bin/mepris \
&& rm /tmp/mepris.zip
```
🪟  
**Requires installed Microsoft Visual C++ Redistributable** (can be installed with, for example, `winget install Microsoft.VCRedist.2015+.x64`)

Using [Scoop](https://scoop.sh):
```
scoop bucket add maxaizer-bucket https://github.com/MaxaiZer/scoop-bucket
scoop install mepris
```
