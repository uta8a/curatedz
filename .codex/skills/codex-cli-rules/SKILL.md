---
name: codex-cli-rules
description: Create or update Codex CLI rules files for command-prefix policy control with valid syntax and predictable behavior. Use when Codex needs to author, review, or refactor `.rules` files (especially `prefix_rule(...)`) for sandbox prompts, approval decisions, and command safety boundaries.
---

# Codex CLI Rules

## Overview
Author and maintain Codex CLI `.rules` (Starlark) with safe defaults, clear intent, and testable boundaries. Keep policies reviewable and operationally predictable.

## Workflow
1. Collect target commands from user workflow and current friction.
2. Classify each command into `allow`, `prompt`, or `forbidden`.
3. Draft narrow `prefix_rule(...)` blocks with explicit justifications.
4. Add `match` and `not_match` examples for every rule.
5. Validate rule behavior and then return final rule blocks plus rationale.

## Safety Model
- Apply least privilege first.
- Default to `prompt` when risk or intent is unclear.
- Use `allow` only for low-risk, frequent operations.
- Use `forbidden` for destructive, privilege-escalation, or secret-exfiltration paths.
- Resolve overlaps with strictness order:
  `forbidden > prompt > allow`.

## Command Classification
- `allow`:
  Read-only and low-impact operations (for example `git status`, `git diff`, `git log`, `rg`, `ls`).
- `prompt`:
  Valid but impactful operations (for example networked writes, remote updates, deployment actions).
- `forbidden`:
  High-risk operations without safe defaults (for example destructive deletion, raw disk operations, escalation commands, obvious secret exposure paths).

## Rule Authoring Standards
- Use exact argv prefix matching in `pattern`.
- Keep one responsibility per rule.
- Keep prefixes narrow (prefer `["git", "status"]` over `["git"]`).
- Write concise `justification` including risk context.
- Add realistic `match` and `not_match` examples for every rule.
- Prefer separate human-managed files (for example `custom.rules`) over manual edits to `default.rules`.

## Decision Heuristics
- Choose `prompt` when command effects touch external systems, network calls, or repository state changes.
- Choose `forbidden` when blast radius is large or recovery is difficult.
- If a pattern captures unrelated commands, split it into smaller rules.
- If uncertain between `allow` and `prompt`, choose `prompt`.

## Validation
- Ensure every rule has both positive and negative examples.
- Ensure nearby commands that must not match are listed in `not_match`.
- If available in the environment, run the ExecPolicy check flow from Codex Rules docs before finalizing.

## Output Contract
Return:
1. Complete `.rules` block(s) ready to paste.
2. 1-3 lines describing safety boundaries and tradeoffs.
3. A focused clarification question only when ambiguity blocks safe policy selection.

See `references/rules-patterns.md` for templates and anti-patterns.
