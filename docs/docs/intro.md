---
slug: /
sidebar_position: 1
---

# Getting started

## Overview

Mepris features:
- **Package Manager Support**: apt, dnf, pacman, flatpak, snap, zypper, brew, scoop, choco, winget, cargo, npm, and AUR
- **Conditional Execution**: Run steps based on OS and custom conditions
- **Modular Configs**: Split your configuration into multiple files
- **Package Aliases**: Define package name mappings for different package managers
- **Dependencies**: Define step dependencies via requires/provides fields.

## Installation

```bash
curl -L https://github.com/MaxaiZer/mepris/releases/latest/download/mepris-x86_64-unknown-linux-gnu.zip \
  -o /tmp/mepris.zip \
&& unzip -p /tmp/mepris.zip mepris | sudo tee /usr/local/bin/mepris > /dev/null \
&& sudo chmod +x /usr/local/bin/mepris \
&& rm /tmp/mepris.zip
```

or something similar on Windows (🤮)
