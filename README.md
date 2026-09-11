# Klotho

A semantic game engine. The source of truth is **intent under law**: a structured, versioned, provenance-bearing description of what the world is allowed to be, what agents want, and what has happened. The runtime **commits** a deterministic simulation from that description and **weaves** a disposable presentation.

v1 tests one hypothesis:

> Can a game be authored and simulated as **Canon + Intent + Trace + Projection** instead of entities, components, and scripts?

This is not Bevy + an LLM. Models may propose; only `CommitKernel` commits.

**Author-facing vocabulary:** Locus, Canon, Intent, Trace, Manifest, Rite, Law.
**Tools:** Distaff (authoring), Weaver (cook + present), `.warp` (package).

The current high-level design is in [`docs/hld.md`](docs/hld.md). The original v1 design is preserved in [`docs/hld-v1.md`](docs/hld-v1.md).
The proposed post-AAA plan for AI-native, end-to-end production of Klotho's first AAA title is in [`docs/hld-ai.md`](docs/hld-ai.md); it does not supersede the current HLD until its decisions land.

## Status

PR 01 — workspace and `klotho-core` (K20 number freeze, K25 `Rng`).
PR 02 — `klotho-prove` (K9 provenance DAG, `LicenseSpan`, in-memory CAS).
PR 03 — `klotho-ir` (IntentDoc, PlayerIntent, MindIntent, InferIntent, RON).
PR 04a — predicate / Rite language RFC, `klotho-canon` CFG checks.
PR 04b — `klotho-canon` eval (pred compiler, tables, tiny-fragment contradiction).
PR 05 — `klotho-trace` (TraceEvent, prefix hash, replay equality).
PR 06 — `klotho-world` (projection, `space_ix`, snapshot, feature `mutate`).
PR 07 — `klotho-commit` (K21 speculate, rite VM, Laws on post-state).
PR 07b — Hearth headless goldens (lock / carry / burn / trade).
PR 07c — Ash headless goldens (same `klotho-commit` binary, K26).
PR 08 — `klotho-sim` phase loop and headless `klotho-runtime`.
PR 09 — `klotho-input` (device → PlayerIntent, bind table).
PR 10 — `klotho-space` (stateless 2.5D admission, golden 8).
PR 11a — `klotho-manifest` (Visual/Sonic/Ui, `tables` pub(crate)).
PR 11b — `klotho-compile` + closed kitbash (retrieval; missing tag = cook error).
PR 12 — `klotho-render` wgpu presenter + `klotho-platform` (Look, render thread).
PR 12b — Hearth pixels (door / barrel / fire / HUD goldens).
PR 13 — `klotho-motion` (verb→clip + root motion, debug T-pose).
PR 14 — `klotho-audio` (grains from Trace, one bed, header caps).
PR 15 — `klotho-ui` (attention from snapshot, denied facts, pause save).
PR 16 — Distaff (`klotho-author`: RON + kdown, CLI cook/preview, cook-time Pin).
PR 17 — Minds + infer isolator (`klotho-mind` GOAP, `klotho-infer` stub).
PR 18 — Debug + determinism CI (`klotho-debug` Trace player, replay.yml).
PR 19 — `.warp` packaging (`klotho-compile` pack, `klotho-runtime` load, caps + license gate).
PR 20 — `klotho-net` listen-server (ed25519, TraceDelta, 2-player local). Optional; Hearth local play is unchanged.
AAA-21b — `klotho-ui` production HUD skin (Knows-gated attention, safe areas, accessibility palette).
AAA-22 — `klotho-infer` OS-process sidecar (snapshot IPC, `InferIntent`-only output, fail-closed child death).
AAA-23 — Netlock dedicated slice.
AAA-24 — incremental cook farm and 50 GB logical fixture.
AAA-25 — live Canon epoch packs.
AAA-26 — console platform/render HAL boundary.
AAA-27 — first-title freeze: 30 Hz single-player action-adventure on desktop; shooter gates remain regressions.
KAI-00 — locked AI-production benchmark, machine, model, farm, and capacity contract.
KAI-01 — separate engine/studio workspaces and generated authoring schema catalog.

## Build

Rust 1.85+ (edition 2024). `rustup` reads `rust-toolchain.toml`.

```bash
cargo test --manifest-path engine/Cargo.toml --workspace
cargo test --manifest-path studio/Cargo.toml --workspace
cargo clippy --manifest-path engine/Cargo.toml --workspace --all-targets -- -D warnings
cargo clippy --manifest-path studio/Cargo.toml --workspace --all-targets -- -D warnings
cargo fmt --manifest-path engine/Cargo.toml --all -- --check
cargo fmt --manifest-path studio/Cargo.toml --all -- --check
cargo run --manifest-path engine/Cargo.toml -p hearth-slice --example pixels
```

Pixel goldens (open in Preview): `engine/examples/hearth-slice/fixtures/pixels/*.bmp`.

## Layout

```
klotho/
  engine/Cargo.toml       # Runtime/presenter/game-package workspace
  engine/crates/klotho-core/ # Tick, Mm, VelFx, YawMd, Sigil, Budget, Hash, Rng
  engine/crates/klotho-prove/ # Provenance DAG, LicenseSpan, blake3 CAS
  engine/crates/klotho-ir/       # IntentDoc, IntentProject, PlayerIntent, MindIntent, InferIntent, RON
  engine/crates/klotho-canon/    # Laws, Affordances, Pred bytecode, Rite ISA, eval
  engine/crates/klotho-trace/    # TraceEvent, TraceLog prefix hash, TraceDelta
  engine/crates/klotho-world/    # Private World, Projection, space_ix, snapshot
  engine/crates/klotho-commit/   # CommitKernel, Proposal, AdmitBuf, rite VM
  engine/crates/klotho-sim/      # phase loop, budget timers, profile hook
  engine/crates/klotho-jobs/     # steal queues, island propose (K34)
  engine/crates/klotho-interest/ # SimLod from observer pose
  engine/crates/klotho-input/    # device sample → PlayerIntent
  engine/crates/klotho-space/    # 2.5D AABB / swept-capsule SyncProposer (non-Actor)
  engine/crates/klotho-phys/     # scalar XPBD island proposer
  engine/crates/klotho-motion/   # verb→clip + root-motion SyncProposer (Actors)
  engine/crates/klotho-mind/     # GOAP SyncProposer (MindIntent, no Agency)
  engine/crates/klotho-infer/    # Default-off OS sidecar; snapshot IPC → InferIntent only
  engine/crates/klotho-manifest/ # Visual/Sonic/Ui manifests; tables pub(crate)
  engine/crates/klotho-compile/  # kitbash retrieval cook → CAS → .warp / sharded catalog
  studio/Cargo.toml       # Distaff/Weaver/AI/evaluation workspace
  studio/crates/klotho-schema/ # Generated machine-readable authoring catalog
  studio/crates/klotho-dcc/ # glTF 2.0 cook → quantized KLTH mesh/hull/clip
  engine/crates/klotho-stream/   # Place shard pager, KCAS volumes (mmap after header validate)
  engine/crates/klotho-save/     # epoch compaction, K19/K48 I/O
  engine/crates/klotho-platform/ # window, OS events, Look accum, audio device (no world mutation)
  engine/crates/klotho-render/   # wgpu clustered meshes, Presenter, render thread
  engine/crates/klotho-audio/    # grains from Trace, one bed, integer mix, device output
  engine/crates/klotho-ui/       # Knows-gated attention, production HUD skin, pause save
  engine/crates/klotho-cinematic/ # Beat-driven Observer tracks (presentation only)
  studio/crates/klotho-author/ # Distaff: RON/kdown, cook-time Pin, CLI cook/preview
  studio/crates/klotho-editor/ # Distaff viewport: Manifest, outliner, Pin, cook, play-in-editor
  engine/crates/klotho-debug/    # Trace player, reject inspector, 4 ms budget gate
  engine/crates/klotho-runtime/  # headless Intent-script / .warp player
  engine/crates/klotho-net/      # listen-server packets, ed25519, TraceDelta
  engine/examples/hearth-slice/ # Appendix A goldens (PR 07b)
  engine/examples/ash-slice/ # Appendix B goldens (PR 07c)
  engine/examples/chorus-slice/ # AAA-17: 2000 Far + 200 Full SimLod headless
  engine/data/kitbash/    # hashed, licensed, affordance-tagged library
  docs/hld.md             # Current high-level design (rev 6, AAA-01–27)
  docs/hld-v1.md          # Preserved v1 high-level design (rev 5)
  docs/pred-lang.md       # Predicate / Rite RFC (PR 04a)
```

Further crates land in the order of the HLD PR plan. `klotho-sim` will never depend on `klotho-infer` or `klotho-jobs`. Unsafe is forbidden except in `klotho-infer`, `klotho-render`, `klotho-audio`, `klotho-platform`, `klotho-jobs`, `klotho-phys`, and `klotho-stream`.
