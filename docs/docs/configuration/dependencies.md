---
sidebar_position: 3
---

# Dependencies

## step completion

In configuration, each step can declare **what it provides** and **what it requires**.  
The program resolves the graph automatically: a step will only run after all steps that provide its `requires` are **completed**.

:::info
By **completion**, I mean that all the step’s packages (if any) are installed and the `check` script (if needed) executes successfully.  

`check` script is needed to verify completion only if step has `script`.  
Without it, program cannot determine the completion state, so the step will not be marked as completed in `dry-run` or interactive mode.  
`pre_script` doesn't require `check` because its main purpose to prepare environment for packages installation. If packages are installed, then it's reasonable to assume that `pre_script` was successful.
:::

:::warning
There will be an error if more than one step that provide needed `requires` are passed OS / when-script filters.
:::

## `provides`

The `provides` field lists **artifacts or capabilities that a step produces**. Other steps can depend on these artifacts by using `requires`.

**Example:**

```yaml
- id: install_postgres
  provides:
    - postgres_installed

- id: setup_database
  requires:
    - postgres_installed
```

Here, `setup_database` depends on `postgres_installed`, which is produced by `install_postgres`. The program ensures `install_postgres` runs first.

---

## `requires`

The `requires` field lists **dependencies a step needs** before it can run. These dependencies declare:

* Needed artifacts provided by other steps (`provides`)
* Conditional requirements based on OS (`os`)
* Conditional requirements based on `when` scripts

### Basic example

```yaml
- id: install_app
  requires:
    - postgres_installed
```

### Example with OS-specific requirement

```yaml

- id: install-nvim-linux-helpers
  os: "linux"
  packages: ["wl-clipboard"]
  provides: ["nvim-linux-helpers"]

- id: install_nvim
  packages: ["nvim"]
  requires:
    - id: nvim-linux-helpers
      os: linux
```

Step `install_nvim` will only require `nvim-linux-helpers` if the current OS matches Linux.

### Example with `when`-script requirement

```yaml
- id: setup-init-system
  requires:
    - id: setup-systemd
      when: "pidof systemd >/dev/null 2>&1"
```

Here, the dependency `setup-systemd` is only required if the `when` script succeeds - current init system is systemd.

## Behavior in interactive mode

Interactive mode allows you to skip step dependencies or rerun already completed steps, giving more control over execution flow.