---
name: gh-issue-view
description: Skill for listing and inspecting GitHub issues using `gh issue list` and `gh issue view`.
---

# gh-issue-view

## Overview
Inspect GitHub issue details by listing issues first and then viewing a target issue.

## Workflow
1. Run `gh issue list` to fetch issue candidates.
2. Identify the target issue number.
3. Run `gh issue view <issue-number>` to get details.
4. Extract and return the issue title and body.

## Command Notes
- List issues: `gh issue list`
- View issue: `gh issue view <issue-number>`

## Output Contract
Return:
1. Target issue number
2. Issue title
3. Issue body (or concise summary when long)
