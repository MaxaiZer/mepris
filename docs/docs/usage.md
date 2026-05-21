---
sidebar_position: 2
---

# Usage

Use `mepris --help` to see all available commands and options.

## Core commands

### Run

```bash
mepris run -f config.yaml
```

Executes steps from the configuration file.

#### Useful options

```bash
mepris run -f config.yaml -t "backend && !docker"
mepris run -f config.yaml -s step1 -s step2
mepris run -f config.yaml -i
mepris run -f config.yaml --dry-run --show-skipped
```

* `-t, --tag` — filter steps using a tag expression
* `-s, --step` — run only specific steps by ID
* `-i, --interactive` — confirm each step before execution
* `-d, --dry-run` — show execution plan without running anything
* `--show-skipped` — show steps that would be skipped (requires `--dry-run`)
* `--debug` — enable debug output (shows script execution time, exit codes, etc.)

Dry-run output example:
```
[PULLED DEPENDENCIES]
🚀 Would run step install_postgres (dependency of setup_db)
📦 Would install packages postgres (pacman)

[SELECTED STEPS]
🚀 Would run step setup_db (pending steps: install_postgres)
✅ Step install-rust completed
```
---

### Resume

```bash
mepris resume
```

Resumes the last failed run.

```bash
mepris resume -i
mepris resume --dry-run --show-skipped
```

* `-i` — resume in interactive mode (for example, to skip failed step)
* `-d, --dry-run` — show execution plan without running anything
* `--show-skipped` — show steps that would be skipped (requires `--dry-run`)
* `--debug` — enable debug output (shows script execution time, exit codes, etc.)

---

## Discovery & tooling

### List steps

```bash
mepris list-steps -f config.yaml
```

Shows available steps.

```bash
mepris list-steps -f config.yaml -t "backend && !docker"
mepris list-steps -f config.yaml --plain
mepris list-steps -f config.yaml --all
```

* `--plain` — output only step IDs
* `--all` — include steps that don’t match current OS

Primarily intended for use in shell completions.

---

### List tags

Shows all tags.

```bash
mepris list-tags -f config.yaml
```

Primarily intended for use in shell completions.

---

### Validate

```bash
mepris validate -f config.yaml
```

Validate configuration and script syntax.  
If no steps are selected via filters, scripts of all steps are validated. Otherwise, only scripts of selected steps are validated.

```bash
mepris validate -f config.yaml -t "backend && !docker"
mepris validate -f config.yaml -s step1 -s step2
```

Filtering works the same as in the run command.

---

## Shell completion

```bash
mepris completion bash
mepris completion zsh
mepris completion fish
mepris completion powershell
```

Generates shell completion scripts.  
Supports advanced completion for step IDs and tags (**only available for Fish and PowerShell**)