---
name: check-ci-results
description: Skill for checking PR-related CI results with `gh pr checks` and diagnosing failures via `gh run view --log/--log-failed`.
---

# check-ci-results

## Overview
Check the latest workflow run for a PR and diagnose failures from logs with actionable fix suggestions.

## Workflow
1. Run `gh pr checks <pr-number>` (or `gh pr checks <pr-number> --json`) to get CI/checks for the PR.
2. Determine overall CI status from checks (pass/fail/pending/etc.).
3. If there are failures, try to obtain a GitHub Actions run ID:
	- Prefer extracting it from the `link` field in `gh pr checks --json` output when it points to `/actions/runs/<run-id>`.
	- If no run link is available, fall back to listing runs for the PR head branch with `gh run list --branch <headRefName> --event pull_request`.
4. On failure (and when you have a run ID), run `gh run view <run-id> --log-failed` (or `--log`) to inspect logs.
5. Summarize root cause and propose concrete fixes.
6. Return any follow-up checks needed before rerun.

## Command Notes
- Checks for a PR: `gh pr checks <number> | <url> | <branch> [--watch]`
- Recommended by `gh run list` manual for PR association: use `gh pr checks`
- Head branch name (for fallback): `gh pr view <pr-number> --json headRefName --jq .headRefName`
- Latest run (fallback): `gh run list --branch <headRefName> --event pull_request --limit 1 --json databaseId,status,conclusion,url,workflowName --jq '.[0]'`
- Run logs: `gh run view <run-id> --log-failed` (or `gh run view <run-id> --log`)

## Output Contract
Return:
1. PR number and run ID
2. CI status
3. For failures: concise "cause", "impact", and "fix"
4. Responses should be in Japanese
