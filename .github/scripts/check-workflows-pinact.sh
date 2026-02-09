#!/usr/bin/env bash
set -euo pipefail

WORKFLOWS_DIR=".github/workflows"

err() {
  echo "[ERROR] $*" >&2
}

info() {
  echo "[INFO] $*"
}

usage() {
  cat <<USAGE
Usage:
  .github/scripts/check-workflows-pinact.sh

What it does:
  1. Finds all workflow files (*.yml, *.yaml) under ${WORKFLOWS_DIR}.
  2. Runs \`pinact run --check\` against all found workflows.
  3. Fails when unpinned actions/reusable workflows are detected.
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

command -v pinact >/dev/null 2>&1 || {
  err "pinact command not found in PATH"
  err "install pinact and retry"
  exit 1
}

[[ -d "${WORKFLOWS_DIR}" ]] || {
  err "workflow directory not found: ${WORKFLOWS_DIR}"
  exit 1
}

mapfile -t workflow_files < <(find "${WORKFLOWS_DIR}" -maxdepth 1 -type f \( -name '*.yml' -o -name '*.yaml' \) | sort)
[[ "${#workflow_files[@]}" -gt 0 ]] || {
  err "no workflow files found under ${WORKFLOWS_DIR}"
  exit 1
}

info "checking ${#workflow_files[@]} workflow files"
for f in "${workflow_files[@]}"; do
  info " - ${f}"
done

pinact run --check "${workflow_files[@]}"

info "pinact check passed"
