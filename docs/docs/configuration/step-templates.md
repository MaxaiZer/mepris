---
sidebar_position: 5
---

# Step templates

Any regular step can serve as a template for other steps via `extends`.

## Example

```yaml
steps:
  - id: start-process
    env: [EXE_PATH, PROCESS_NAME]
    script: |
      wt nt --title "$PROCESS_NAME" cmd /c start "" "$EXE_PATH" --name "$WT_TITLE"

  - id: start-host
    extends: start-process
    env: [REPO_PATH, PROCESS_NAME]
    with:
      EXE_PATH: "${REPO_PATH}/Host.exe"
      WT_TITLE: "MyTitle"
```

`start-process` is a regular step that doubles as a template. `start-process` inherits its script and satisfies the variables it declares:

- `EXE_PATH` — supplied via `with`.
- `PROCESS_NAME` — not in `with`, but declared in `start-host`'s own `env`, so its value is taken from your environment at runtime.
- `REPO_PATH` — a regular runtime dependency of the consumer step; used to resolve `${REPO_PATH}` inside the `with` value.
- `WT_TITLE` — an extra variable: not declared in the template's `env`, but still injected into the scripts because it is passed via `with`.

## Variable contract

The referenced step's `env:` lists the variables its script logic requires. For each of them, every consumer must explicitly supply a value:

1. If the variable name appears in the consumer's `with` — the `with` value is used.
2. Otherwise the consumer must declare the variable in its own `env` — then the value comes from your environment at runtime.
3. If neither — that's a config error.

As usual, every variable declared in a step's own `env:` must be present in your environment before the run starts.

## Inherited fields

Only the four script-related fields are inherited: `pre_script`, `script`, `when` and `check`. All other fields belong to each step individually — they are never copied from the referenced step.

For each script field, if **both** steps define it that's an error (inheritance never silently overrides); otherwise whichever side defines it is used. So a shared step can provide just `script` while each consumer adds its own `pre_script`, `when` or `check`.

## Value interpolation

`with` values are plain strings. To pull a value from an environment variable use the braced form `${NAME}` — mepris resolves it from your process environment before any script starts:

```yaml
  - id: start-host
    extends: start-process
    with:
      EXE_PATH: "${ROOT_PATH}/Host.exe" # resolved by mepris from the environment
      WT_TITLE: "$HOME/Host"            # passed through literally — no braces, no interpolation
```

Rules:

- Only `${NAME}` (braces) is interpolated; a bare `$NAME` stays literal.
- If a referenced variable is not set (or the name inside the braces is invalid), the run fails with an error.
- A step that uses `extends` itself cannot be used as a template: inheritance chains, self-reference and cycles are all rejected.
- The referenced step and its consumers may live in different included files; visibility is global after parsing.

:::info
Interpolation happens in mepris itself, not in your shell — it works identically for `bash`, `powershell` and `pwsh`. There is no escape sequence: a literal `${` inside a `with` value will always be treated as an environment reference.
:::