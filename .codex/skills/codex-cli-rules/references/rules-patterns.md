# Codex CLI Rules Patterns

Use this reference when authoring or reviewing Codex CLI `.rules` files.

## Core model
- Place rules under `~/.codex/rules/` (for example `default.rules`).
- Main primitive: `prefix_rule(...)`.
- `decision` values: `allow`, `prompt`, `forbidden`.
- If multiple rules match, the strictest decision wins:
  `forbidden > prompt > allow`.
- `match` and `not_match` act as inline tests and should be provided for every rule.

Official reference:
- https://developers.openai.com/codex/rules/

## Design principles
1. Follow least privilege.
2. Default to `prompt`; use `allow` only for clearly safe, high-frequency commands.
3. Use `forbidden` for clearly dangerous operations.
4. Keep patterns narrow and task-specific.

## Dangerous operations to forbid
Common examples:
- Destructive disk/file operations: `rm -rf`, `dd`, `mkfs`
- System control commands: `shutdown`, `reboot`
- Privilege escalation: `sudo`, `su`
- Commands likely to expose secrets from sensitive paths

## Keep pattern boundaries narrow
Avoid broad patterns such as `pattern = ["git"]`.
Prefer scoped prefixes like:
- `pattern = ["git", "status"]`
- `pattern = ["git", "diff"]`
- `pattern = ["git", "log"]`

## Baseline template
```starlark
prefix_rule(
    pattern = ["gh", "pr", "view"],
    decision = "prompt",
    justification = "Viewing PR details may access remote data",
    match = [
        "gh pr view 123",
        "gh pr view --repo org/repo",
    ],
    not_match = [
        "gh pr list",
        "gh repo view org/repo",
    ],
)
```

## Read-only allow pattern
Use `allow` for low-risk inspection commands only.

```starlark
prefix_rule(
    pattern = ["git", ["status", "diff", "log"]],
    decision = "allow",
    justification = "Read-only git inspection",
    match = [
        "git status",
        "git diff --stat",
        "git log -n 20",
    ],
    not_match = [
        "git commit -m test",
    ],
)
```

## Quality checklist
- `pattern` is as specific as possible.
- `justification` explains risk and intent in one short sentence.
- `match` examples are realistic and must pass.
- `not_match` includes nearby commands that must not match.
- Broad or overlapping rules are avoided unless intentional and documented.
