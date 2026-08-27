#!/usr/bin/env bash
# forbidden_gameplay_imports (HLD K2 / PR 11a) and InferHost allowlist (K4).
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"

fail=0

check_gameplay_tables() {
  local dir="$1"
  if [[ ! -d "$dir" ]]; then
    return 0
  fi
  if command -v rg >/dev/null 2>&1; then
    if rg -n --glob '!target/**' 'klotho_manifest::tables|klotho-manifest::tables' "$dir"; then
      echo "forbidden_gameplay_imports: $dir must not import klotho-manifest::tables" >&2
      fail=1
    fi
  else
    if grep -RIn -E 'klotho_manifest::tables|klotho-manifest::tables' "$dir" >/dev/null 2>&1; then
      echo "forbidden_gameplay_imports: $dir must not import klotho-manifest::tables" >&2
      fail=1
    fi
  fi
}

check_gameplay_tables examples/hearth-slice
check_gameplay_tables examples/ash-slice
check_gameplay_tables examples/ember-slice
check_gameplay_tables examples/drift-slice
check_gameplay_tables examples/chorus-slice
check_gameplay_tables examples/netlock-slice
check_gameplay_tables crates/klotho-author
check_gameplay_tables crates/klotho-editor

# klotho-interest may not import commit (K49).
if command -v rg >/dev/null 2>&1; then
  if rg -n --glob '!target/**' 'klotho_commit::|klotho-commit' crates/klotho-interest; then
    echo "klotho-interest must depend on world+core only" >&2
    fail=1
  fi
else
  if grep -RIn -E 'klotho_commit::|klotho-commit' crates/klotho-interest >/dev/null 2>&1; then
    echo "klotho-interest must depend on world+core only" >&2
    fail=1
  fi
fi

# Gameplay never schedules jobs (K46).
check_gameplay_jobs() {
  local dir="$1"
  if [[ ! -d "$dir" ]]; then
    return 0
  fi
  if command -v rg >/dev/null 2>&1; then
    if rg -n --glob '!target/**' 'klotho_jobs::|klotho-jobs' "$dir"; then
      echo "forbidden_gameplay_imports: $dir must not import klotho-jobs" >&2
      fail=1
    fi
  else
    if grep -RIn -E 'klotho_jobs::|klotho-jobs' "$dir" >/dev/null 2>&1; then
      echo "forbidden_gameplay_imports: $dir must not import klotho-jobs" >&2
      fail=1
    fi
  fi
}
check_gameplay_jobs examples/hearth-slice
check_gameplay_jobs examples/ash-slice
check_gameplay_jobs crates/klotho-author

# InferHost::{new,submit,poll} may appear only in klotho-runtime and klotho-infer.
if [[ -d crates ]]; then
  hits=""
  if command -v rg >/dev/null 2>&1; then
    hits="$(rg -n --glob '!target/**' 'InferHost::(new|submit|poll)' crates || true)"
  else
    hits="$(grep -RIn -E 'InferHost::(new|submit|poll)' crates || true)"
  fi
  if [[ -n "$hits" ]]; then
    while IFS= read -r line; do
      [[ -z "$line" ]] && continue
      case "$line" in
        crates/klotho-runtime/*|crates/klotho-infer/*) ;;
        *)
          echo "InferHost call outside allowlist: $line" >&2
          fail=1
          ;;
      esac
    done <<EOF
$hits
EOF
  fi
fi

exit "$fail"
