# Klotho

A semantic game engine. The source of truth is **intent under law**: a structured, versioned, provenance-bearing description of what the world is allowed to be, what agents want, and what has happened. The runtime **commits** a deterministic simulation from that description and **weaves** a disposable presentation.

v1 tests one hypothesis:

> Can a game be authored and simulated as **Canon + Intent + Trace + Projection** instead of entities, components, and scripts?

This is not Bevy + an LLM. Models may propose; only `CommitKernel` commits.

**Author-facing vocabulary:** Locus, Canon, Intent, Trace, Manifest, Rite, Law.
**Tools:** Distaff (authoring), Weaver (cook + present), `.warp` (package).

The full high-level design is in [`docs/hld.md`](docs/hld.md).

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

## Build

Rust 1.85+ (edition 2024). `rustup` reads `rust-toolchain.toml`.

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo run -p hearth-slice --example pixels   # window: keys 1–4 switch scenes
```

Pixel goldens (open in Preview): `examples/hearth-slice/fixtures/pixels/*.bmp`.

## Layout

```
klotho/
  crates/klotho-core/     # Tick, Mm, VelFx, YawMd, Sigil, Budget, Hash, Rng
  crates/klotho-prove/    # Provenance DAG, LicenseSpan, blake3 CAS
  crates/klotho-ir/       # IntentDoc, PlayerIntent, MindIntent, InferIntent, RON
  crates/klotho-canon/    # Laws, Affordances, Pred bytecode, Rite ISA, eval
  crates/klotho-trace/    # TraceEvent, TraceLog prefix hash, TraceDelta
  crates/klotho-world/    # Private World, Projection, space_ix, snapshot
  crates/klotho-commit/   # CommitKernel, Proposal, AdmitBuf, rite VM
  crates/klotho-sim/      # phase loop, budget timers, profile hook
  crates/klotho-jobs/     # steal queues, island propose (K34)
  crates/klotho-interest/ # SimLod from observer pose
  crates/klotho-input/    # device sample → PlayerIntent
  crates/klotho-space/    # 2.5D AABB / swept-capsule SyncProposer (non-Actor)
  crates/klotho-phys/     # scalar XPBD island proposer
  crates/klotho-motion/   # verb→clip + root-motion SyncProposer (Actors)
  crates/klotho-mind/     # GOAP SyncProposer (MindIntent, no Agency)
  crates/klotho-infer/    # InferHost stub; returns InferIntent (no &mut World)
  crates/klotho-manifest/ # Visual/Sonic/Ui manifests; tables pub(crate)
  crates/klotho-compile/  # kitbash retrieval cook → CAS → .warp / sharded catalog
  crates/klotho-dcc/      # glTF 2.0 cook → quantized KLTH mesh/hull/clip
  crates/klotho-stream/   # Place shard pager, KCAS volumes (mmap after header validate)
  crates/klotho-save/     # epoch compaction, K19/K48 I/O
  crates/klotho-platform/ # window, OS events, Look accum, audio device (no world mutation)
  crates/klotho-render/   # wgpu clustered meshes, Presenter, render thread
  crates/klotho-audio/    # grains from Trace, one bed, integer mix, device output
  crates/klotho-ui/       # Knows-gated attention, production HUD skin, pause save
  crates/klotho-cinematic/ # Beat-driven Observer tracks (presentation only)
  crates/klotho-author/   # Distaff: RON/kdown, cook-time Pin, CLI cook/preview
  crates/klotho-editor/   # Distaff viewport: Manifest, outliner, Pin, cook, play-in-editor
  crates/klotho-debug/    # Trace player, reject inspector, 4 ms budget gate
  crates/klotho-runtime/  # headless Intent-script / .warp player
  crates/klotho-net/      # listen-server packets, ed25519, TraceDelta
  examples/hearth-slice/  # Appendix A goldens (PR 07b)
  examples/ash-slice/     # Appendix B goldens (PR 07c)
  examples/chorus-slice/  # AAA-17: 2000 Far + 200 Full SimLod headless
  data/kitbash/           # hashed, licensed, affordance-tagged library
  docs/hld.md             # High-level design (rev 5)
  docs/pred-lang.md       # Predicate / Rite RFC (PR 04a)
```

Further crates land in the order of the HLD PR plan. `klotho-sim` will never depend on `klotho-infer` or `klotho-jobs`. Unsafe is forbidden except in `klotho-infer`, `klotho-render`, `klotho-audio`, `klotho-platform`, `klotho-jobs`, `klotho-phys`, and `klotho-stream`.
