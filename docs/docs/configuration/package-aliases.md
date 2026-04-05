---
sidebar_position: 4
---

# Package aliases

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
# See `Step fields -> package_source` for valid values

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

`apt install -y fd-find`