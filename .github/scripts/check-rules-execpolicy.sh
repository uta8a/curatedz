#!/usr/bin/env bash
set -euo pipefail

RULES_DIR=".codex/rules"

err() {
  echo "[ERROR] $*" >&2
}

info() {
  echo "[INFO] $*"
}

usage() {
  cat <<USAGE
Usage:
  .github/scripts/check-rules-execpolicy.sh

What it does:
  1. Finds all *.rules under ${RULES_DIR}.
  2. Parses each rule's decision/match/not_match examples.
  3. Runs `codex execpolicy check` for each example command.
  4. Verifies executable/non-executable expectations:
     - match examples in allow rules -> decision must be allow
     - match examples in forbidden rules -> decision must be forbidden
     - not_match examples in forbidden rules -> decision must NOT be forbidden
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

command -v codex >/dev/null 2>&1 || {
  err "codex command not found in PATH"
  err "install Codex CLI and retry"
  exit 1
}

mapfile -t RULE_FILES < <(find "${RULES_DIR}" -maxdepth 1 -type f -name '*.rules' | sort)
[[ "${#RULE_FILES[@]}" -gt 0 ]] || {
  err "no .rules files found under ${RULES_DIR}"
  exit 1
}

rules_args=()
for f in "${RULE_FILES[@]}"; do
  rules_args+=(--rules "$f")
done

tmp_cases="$(mktemp)"
cleanup() {
  rm -f "$tmp_cases"
}
trap cleanup EXIT

# TSV output: file<TAB>decision<TAB>section(match|not_match)<TAB>command
awk '
  /decision[[:space:]]*=[[:space:]]*"/ {
    if (match($0, /decision[[:space:]]*=[[:space:]]*"([^"]+)"/, m)) decision = m[1]
  }
  /not_match[[:space:]]*=[[:space:]]*\[/ { section = "not_match"; next }
  /match[[:space:]]*=[[:space:]]*\[/ { section = "match"; next }
  section != "" {
    if ($0 ~ /^[[:space:]]*\]/) { section = ""; next }
    if ($0 ~ /"/) {
      cmd = $0
      sub(/^[^"]*"/, "", cmd)
      sub(/"[[:space:]]*,?[[:space:]]*$/, "", cmd)
      gsub(/\\"/, "\"", cmd)
      print FILENAME "\t" decision "\t" section "\t" cmd
    }
  }
' "${RULE_FILES[@]}" > "$tmp_cases"

total=0
ok=0
ng=0

check_case() {
  local file="$1" decision="$2" section="$3" cmd="$4"
  local expected=""

  if [[ "$section" == "match" ]]; then
    expected="$decision"
  elif [[ "$section" == "not_match" && "$decision" == "forbidden" ]]; then
    expected="not-forbidden"
  else
    return 0
  fi

  total=$((total + 1))
  info "checking [$file] ${section}: ${cmd}"

  # shellcheck disable=SC2086
  eval "set -- ${cmd}"
  output="$(codex execpolicy check "${rules_args[@]}" "$@" 2>&1 || true)"
  json_line="$(printf '%s\n' "$output" | rg '^\{.*"decision"' | tail -n 1 || true)"

  if [[ -z "$json_line" ]]; then
    err "execpolicy check failed to produce decision for: ${cmd}"
    printf '%s\n' "$output" >&2
    ng=$((ng + 1))
    return 0
  fi

  actual="$(printf '%s\n' "$json_line" | sed -E 's/.*"decision":"([^"]+)".*/\1/')"

  if [[ "$expected" == "not-forbidden" ]]; then
    if [[ "$actual" == "forbidden" ]]; then
      err "unexpected forbidden: ${cmd}"
      ng=$((ng + 1))
    else
      ok=$((ok + 1))
    fi
    return 0
  fi

  if [[ "$actual" == "$expected" ]]; then
    ok=$((ok + 1))
  else
    err "decision mismatch: expected=${expected} actual=${actual} cmd=${cmd}"
    ng=$((ng + 1))
  fi
}

while IFS=$'\t' read -r file decision section cmd; do
  [[ -n "$cmd" ]] || continue
  check_case "$file" "$decision" "$section" "$cmd"
done < "$tmp_cases"

echo "[RESULT] checked=${total} ok=${ok} ng=${ng}"
[[ "$ng" -eq 0 ]]
