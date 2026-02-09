---
name: gh-pr-creator
description: Skill for creating GitHub pull requests with `gh pr create`, following repository PR templates and Japanese authoring constraints.
---

# gh-pr-creator

## Overview
Create pull requests with `gh pr create`. The PR body must follow pull request templates under `.github/` when present.

## Workflow
1. Confirm the current repository owner is under `uta8a/`.
2. Detect and read the PR template under `.github/`.
3. Draft a Japanese PR title and body that reflect the actual changes.
4. Run `gh pr create` with explicit title/body and base/head branches when needed.
5. Return the created PR number and URL.

## Safety Rules
- Do not create PRs for repositories outside `uta8a/`.
- Always write PR title and body in Japanese.
- If a template exists, fill all required sections.

## Output Contract
Return:
1. Execution summary (template used, branch choices)
2. Created PR number and URL
3. One follow-up check only if required
