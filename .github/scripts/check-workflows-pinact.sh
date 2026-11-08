#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$repo_root"
command -v pinact >/dev/null 2>&1 || {
  echo "pinact が必要です。mise install を実行してください。" >&2
  exit 1
}
workflows=()
for workflow in .github/workflows/*.yml .github/workflows/*.yaml; do
  [[ -f "$workflow" ]] && workflows+=("$workflow")
done
[[ "${#workflows[@]}" -gt 0 ]] || {
  echo "workflow がありません。" >&2
  exit 1
}
pinact run --check "${workflows[@]}"
