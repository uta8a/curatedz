---
name: check-ci-results
description: Skill for checking PR-related CI results with `gh run list --pr` and diagnosing failures via `gh run view --log`.
---

# check-ci-results

## Overview
Check the latest workflow run for a PR and diagnose failures from logs with actionable fix suggestions.

## Workflow
1. Run `gh run list --pr <pr-number> --limit 1` to get the latest run.
2. Determine run status (success, failure, in_progress, etc.).
3. On failure, run `gh run view <run-id> --log` to inspect failing jobs.
4. Summarize root cause and propose concrete fixes.
5. Return any follow-up checks needed before rerun.

## Command Notes
- Latest run: `gh run list --pr <pr-number> --limit 1`
- Run logs: `gh run view <run-id> --log`

## Output Contract
Return:
1. PR number and run ID
2. CI status
3. For failures: concise "cause", "impact", and "fix"
4. Responses should be in Japanese
