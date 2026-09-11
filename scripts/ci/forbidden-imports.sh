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

check_gameplay_tables engine/examples/hearth-slice
check_gameplay_tables engine/examples/ash-slice
check_gameplay_tables engine/examples/ember-slice
check_gameplay_tables engine/examples/drift-slice
check_gameplay_tables engine/examples/chorus-slice
check_gameplay_tables engine/examples/netlock-slice
check_gameplay_tables studio/crates/klotho-author
check_gameplay_tables studio/crates/klotho-editor
check_gameplay_tables studio/crates/klotho-ai
check_gameplay_tables studio/crates/klotho-dcc
check_gameplay_tables studio/crates/klotho-pattern

# klotho-interest may not import commit (K49).
if command -v rg >/dev/null 2>&1; then
  if rg -n --glob '!target/**' 'klotho_commit::|klotho-commit' engine/crates/klotho-interest; then
    echo "klotho-interest must depend on world+core only" >&2
    fail=1
  fi
else
  if grep -RIn -E 'klotho_commit::|klotho-commit' engine/crates/klotho-interest >/dev/null 2>&1; then
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
check_gameplay_jobs engine/examples/hearth-slice
check_gameplay_jobs engine/examples/ash-slice
check_gameplay_jobs engine/examples/ember-slice
check_gameplay_jobs engine/examples/drift-slice
check_gameplay_jobs engine/examples/chorus-slice
check_gameplay_jobs engine/examples/netlock-slice
check_gameplay_jobs studio/crates/klotho-author
check_gameplay_jobs studio/crates/klotho-editor
check_gameplay_jobs studio/crates/klotho-ai

# Gameplay and authoring must not import cook-time DCC.
check_no_dcc() {
  local dir="$1"
  if [[ ! -d "$dir" ]]; then
    return 0
  fi
  if command -v rg >/dev/null 2>&1; then
    if rg -n --glob '!target/**' 'klotho_dcc::|klotho-dcc' "$dir"; then
      echo "forbidden_imports: $dir must not import klotho-dcc" >&2
      fail=1
    fi
  else
    if grep -RIn -E 'klotho_dcc::|klotho-dcc' "$dir" >/dev/null 2>&1; then
      echo "forbidden_imports: $dir must not import klotho-dcc" >&2
      fail=1
    fi
  fi
}
check_no_dcc engine/examples/hearth-slice
check_no_dcc engine/examples/ash-slice
check_no_dcc engine/examples/ember-slice
check_no_dcc engine/examples/drift-slice
check_no_dcc engine/examples/chorus-slice
check_no_dcc engine/examples/netlock-slice
check_no_dcc studio/crates/klotho-author
check_no_dcc studio/crates/klotho-editor
check_no_dcc studio/crates/klotho-ai
check_no_dcc engine/crates/klotho-sim
check_no_dcc engine/crates/klotho-commit

# Gameplay, motion, and sim must not import phys internals.
check_no_phys() {
  local dir="$1"
  if [[ ! -d "$dir" ]]; then
    return 0
  fi
  if command -v rg >/dev/null 2>&1; then
    if rg -n --glob '!target/**' 'klotho_phys::|klotho-phys' "$dir"; then
      echo "forbidden_imports: $dir must not import klotho-phys" >&2
      fail=1
    fi
  else
    if grep -RIn -E 'klotho_phys::|klotho-phys' "$dir" >/dev/null 2>&1; then
      echo "forbidden_imports: $dir must not import klotho-phys" >&2
      fail=1
    fi
  fi
}
check_no_phys engine/examples/hearth-slice
check_no_phys engine/examples/ash-slice
check_no_phys engine/examples/ember-slice
check_no_phys engine/examples/drift-slice
check_no_phys engine/examples/chorus-slice
check_no_phys engine/examples/netlock-slice
check_no_phys studio/crates/klotho-author
check_no_phys studio/crates/klotho-editor
check_no_phys studio/crates/klotho-ai
check_no_phys engine/crates/klotho-motion
check_no_phys engine/crates/klotho-sim
check_no_phys engine/crates/klotho-commit

# Gameplay, sim, and commit must not import stream. Stream does not import commit
# and must not enable world/mutate.
check_no_stream() {
  local dir="$1"
  if [[ ! -d "$dir" ]]; then
    return 0
  fi
  if command -v rg >/dev/null 2>&1; then
    if rg -n --glob '!target/**' 'klotho_stream::|klotho-stream' "$dir"; then
      echo "forbidden_imports: $dir must not import klotho-stream" >&2
      fail=1
    fi
  else
    if grep -RIn -E 'klotho_stream::|klotho-stream' "$dir" >/dev/null 2>&1; then
      echo "forbidden_imports: $dir must not import klotho-stream" >&2
      fail=1
    fi
  fi
}
check_no_stream engine/examples/hearth-slice
check_no_stream engine/examples/ash-slice
check_no_stream engine/examples/ember-slice
check_no_stream engine/examples/drift-slice
check_no_stream engine/examples/chorus-slice
check_no_stream engine/examples/netlock-slice
check_no_stream studio/crates/klotho-author
check_no_stream studio/crates/klotho-editor
check_no_stream studio/crates/klotho-ai
check_no_stream engine/crates/klotho-sim
check_no_stream engine/crates/klotho-commit

if [[ -d engine/crates/klotho-stream ]]; then
  if command -v rg >/dev/null 2>&1; then
    if rg -n --glob '!target/**' 'klotho_commit::|klotho-commit' engine/crates/klotho-stream; then
      echo "klotho-stream must not import klotho-commit" >&2
      fail=1
    fi
  else
    if grep -RIn -E 'klotho_commit::|klotho-commit' engine/crates/klotho-stream >/dev/null 2>&1; then
      echo "klotho-stream must not import klotho-commit" >&2
      fail=1
    fi
  fi
  if grep -E 'mutate' engine/crates/klotho-stream/Cargo.toml >/dev/null 2>&1; then
    echo "klotho-stream must not enable klotho-world/mutate" >&2
    fail=1
  fi
fi

if [[ -d engine/crates/klotho-save ]]; then
  if command -v rg >/dev/null 2>&1; then
    if rg -n --glob '!target/**' 'klotho_commit::|klotho-commit|klotho_stream::|klotho-stream' engine/crates/klotho-save; then
      echo "klotho-save must not import klotho-commit or klotho-stream" >&2
      fail=1
    fi
  else
    if grep -RIn -E 'klotho_commit::|klotho-commit|klotho_stream::|klotho-stream' engine/crates/klotho-save >/dev/null 2>&1; then
      echo "klotho-save must not import klotho-commit or klotho-stream" >&2
      fail=1
    fi
  fi
  if grep -E 'mutate' engine/crates/klotho-save/Cargo.toml >/dev/null 2>&1; then
    echo "klotho-save must not enable klotho-world/mutate" >&2
    fail=1
  fi
fi

# The world write path (`mutate`) may be enabled in [dependencies] solely by
# klotho-commit. Any crate may enable it in [dev-dependencies] for tests:
# dev-dependencies never ship, so that cannot unify the write API into
# production builds. Section-aware: a bare grep cannot tell the two apart.
if [[ -d engine/crates ]]; then
  for manifest in engine/crates/*/Cargo.toml; do
    awk -v file="$manifest" '
      /^\[/ { section = $0; next }
      /mutate/ {
        if ($0 ~ /required-features/) next
        if (file == "engine/crates/klotho-world/Cargo.toml" && section == "[features]") next
        if (file == "engine/crates/klotho-commit/Cargo.toml" && section == "[dependencies]") next
        if (section == "[dev-dependencies]") next
        print "klotho-world/mutate outside allowlist: " file ":" FNR ": " $0
        bad = 1
      }
      END { exit bad }
    ' "$manifest" || fail=1
  done
fi

# InferHost::{new,spawn,submit,poll} may appear only in klotho-runtime and klotho-infer.
if [[ -d engine/crates ]]; then
  hits=""
  if command -v rg >/dev/null 2>&1; then
    hits="$(rg -n --glob '!target/**' 'InferHost::(new|spawn|submit|poll)' engine/crates || true)"
  else
    hits="$(grep -RIn -E 'InferHost::(new|spawn|submit|poll)' engine/crates || true)"
  fi
  if [[ -n "$hits" ]]; then
    while IFS= read -r line; do
      [[ -z "$line" ]] && continue
      case "$line" in
        engine/crates/klotho-runtime/*|engine/crates/klotho-infer/*) ;;
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

# Studio AI crate is not in the engine graph and may not import commit/world/sim/runtime/infer.
if [[ -d engine/crates ]]; then
  if command -v rg >/dev/null 2>&1; then
    if rg -n --glob '!target/**' 'klotho_ai::|klotho-ai' engine; then
      echo "engine must not import klotho-ai" >&2
      fail=1
    fi
    if rg -n --glob '!target/**' 'klotho_pattern::' engine; then
      echo "engine must not import klotho-pattern" >&2
      fail=1
    fi
    if rg -n --glob 'Cargo.toml' 'klotho-pattern' engine; then
      echo "engine Cargo.toml must not depend on klotho-pattern" >&2
      fail=1
    fi
  else
    if grep -RIn -E 'klotho_ai::|klotho-ai' engine >/dev/null 2>&1; then
      echo "engine must not import klotho-ai" >&2
      fail=1
    fi
    if grep -RIn -E 'klotho_pattern::' engine >/dev/null 2>&1; then
      echo "engine must not import klotho-pattern" >&2
      fail=1
    fi
    if grep -RIn -E 'klotho-pattern' engine --include='Cargo.toml' >/dev/null 2>&1; then
      echo "engine Cargo.toml must not depend on klotho-pattern" >&2
      fail=1
    fi
  fi
fi

if [[ -d studio/crates/klotho-pattern ]]; then
  if command -v rg >/dev/null 2>&1; then
    if rg -n --glob '!target/**' 'klotho_commit::|klotho-commit|klotho_world::|klotho-world|klotho_sim::|klotho-sim|klotho_runtime::|klotho-runtime|klotho_infer::|klotho-infer' studio/crates/klotho-pattern; then
      echo "klotho-pattern must not import commit/world/sim/runtime/infer" >&2
      fail=1
    fi
  else
    if grep -RIn -E 'klotho_commit::|klotho-commit|klotho_world::|klotho-world|klotho_sim::|klotho-sim|klotho_runtime::|klotho-runtime|klotho_infer::|klotho-infer' studio/crates/klotho-pattern >/dev/null 2>&1; then
      echo "klotho-pattern must not import commit/world/sim/runtime/infer" >&2
      fail=1
    fi
  fi
  if grep -E 'klotho-commit|klotho-world|klotho-sim|klotho-runtime|klotho-infer' studio/crates/klotho-pattern/Cargo.toml >/dev/null 2>&1; then
    echo "klotho-pattern Cargo.toml must not depend on commit/world/sim/runtime/infer" >&2
    fail=1
  fi
fi

if [[ -d studio/crates/klotho-ai ]]; then
  if command -v rg >/dev/null 2>&1; then
    if rg -n --glob '!target/**' 'klotho_commit::|klotho-commit|klotho_world::|klotho-world|klotho_sim::|klotho-sim|klotho_runtime::|klotho-runtime|klotho_infer::|klotho-infer' studio/crates/klotho-ai; then
      echo "klotho-ai must not import commit/world/sim/runtime/infer" >&2
      fail=1
    fi
  else
    if grep -RIn -E 'klotho_commit::|klotho-commit|klotho_world::|klotho-world|klotho_sim::|klotho-sim|klotho_runtime::|klotho-runtime|klotho_infer::|klotho-infer' studio/crates/klotho-ai >/dev/null 2>&1; then
      echo "klotho-ai must not import commit/world/sim/runtime/infer" >&2
      fail=1
    fi
  fi
  if grep -E 'klotho-commit|klotho-world|klotho-sim|klotho-runtime|klotho-infer' studio/crates/klotho-ai/Cargo.toml >/dev/null 2>&1; then
    echo "klotho-ai Cargo.toml must not depend on commit/world/sim/runtime/infer" >&2
    fail=1
  fi
fi

exit "$fail"
