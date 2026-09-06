#!/usr/bin/env bash
set -euo pipefail

# AAA-24: logical 50 GiB/multi-volume planning plus the dirty-Place <60 s gate.
cargo test --release -p klotho-compile --test cook_farm
