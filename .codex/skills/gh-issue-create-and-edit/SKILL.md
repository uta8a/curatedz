---
name: gh-issue-create-and-edit
description: Skill for creating and editing GitHub issues with `gh issue create` and `gh issue edit` under Japanese writing constraints.
---

# gh-issue-create-and-edit

## Overview
Create new issues with `gh issue create` and update existing ones with `gh issue edit <issue-number>`.

## Workflow
1. Confirm the current repository owner is under `uta8a/`.
2. Draft issue title and body in Japanese.
3. Create the issue with `gh issue create`.
4. If updates are requested, edit with `gh issue edit <issue-number>`.
5. Return final issue number and URL.

## Safety Rules
- Do not create or edit issues outside `uta8a/` repositories.
- Always write issue title and body in Japanese.

## Output Contract
Return:
1. Operation performed (create or edit)
2. Target issue number and URL
3. Short change summary when edited
