#!/usr/bin/env bash
# KAI-01: prove that game releases can build with no studio tree present.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
engine_manifest="$root/engine/Cargo.toml"
studio_manifest="$root/studio/Cargo.toml"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

cargo metadata --manifest-path "$engine_manifest" --format-version 1 >"$tmp/engine.json"
cargo metadata --manifest-path "$studio_manifest" --format-version 1 >"$tmp/studio.json"

python3 - "$root" "$tmp/engine.json" "$tmp/studio.json" <<'PY'
import json
import pathlib
import sys

root = pathlib.Path(sys.argv[1]).resolve()
engine_root = root / "engine"
studio_root = root / "studio"

def manifests(path):
    data = json.loads(pathlib.Path(path).read_text())
    return [pathlib.Path(p["manifest_path"]).resolve() for p in data["packages"]]

engine = manifests(sys.argv[2])
studio = manifests(sys.argv[3])
bad_engine = [str(p) for p in engine if p == studio_root or studio_root in p.parents]
if bad_engine:
    raise SystemExit("engine metadata reaches studio: " + ", ".join(bad_engine))
bad_studio = [str(p) for p in studio if root in p.parents and not (
    p == engine_root or engine_root in p.parents or p == studio_root or studio_root in p.parents
)]
if bad_studio:
    raise SystemExit("studio metadata reaches an unowned workspace path: " + ", ".join(bad_studio))
PY

mkdir "$tmp/export"
# Copy only checked-in engine inputs. A developer's local target cache may be
# many GiB and is neither source nor part of the clean-export contract.
cp "$root/engine/Cargo.toml" "$root/engine/Cargo.lock" "$tmp/export/"
cp -R "$root/engine/crates" "$root/engine/examples" "$root/engine/data" "$tmp/export/"
if [[ -e "$tmp/export/../studio" ]]; then
  echo "clean export unexpectedly contains studio" >&2
  exit 1
fi
cargo test --manifest-path "$tmp/export/Cargo.toml" --workspace
