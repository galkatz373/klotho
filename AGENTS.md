# Klotho — agent notes

Greenfield Rust engine. The programming model is **Canon + Intent + Trace + Projection**, not ECS/components. Read `docs/hld.md` before adding a type.

## Hard rules

- Four categories, nothing else: source (Canon, PlayerIntent, Trace), derived authoritative state (Projection), non-authoritative proposal (Space, Motion, Mind, Infer), disposable presentation (Manifest).
- Only `CommitKernel` commits. Everyone else emits `Proposal`s. Partial writes are never visible (K21).
- `World` is a view of `(canon_hash, trace_prefix_hash)` plus the live Intent heap. Not a source.
- No `Update()` on loci. No `&mut World` in `klotho-infer`. No `InferToken`. No HashMap iteration on the commit path (K25).
- `#![forbid(unsafe_code)]` on every crate except `klotho-infer`, `klotho-render`, `klotho-audio`, `klotho-platform`, `klotho-jobs` (steal queues), `klotho-phys`, `klotho-stream` (mmap after header validate).
- Number types are frozen in `klotho-core` (K20): `Mm(i32)`, `VelFx` 16.16, `YawMd` millidegrees. Do not introduce `f32` on the commit path.
- One `klotho_core::Rng`, seeded from `canon_hash ⊕ tick`. No other RNG.

## Crate graph (do not violate)

- `klotho-sim` does **not** depend on infer, render, mind, space, motion, jobs, or stream.
- `klotho-commit` does **not** depend on space/motion/mind types. `HullWitness` lives in `klotho-core`. Commit does **not** depend on stream.
- `klotho-infer` does **not** depend on `klotho-commit`. It returns `InferIntent`.
- `klotho-world` feature `mutate` is enabled **only** by `klotho-commit`. `klotho-stream` and `klotho-save` do not enable `mutate`.
- `klotho-save` depends on world + trace + core only. It does **not** depend on commit or stream.
- `klotho-release` depends on compile + save + world + trace + prove + platform. It does **not** enable `klotho-world/mutate`, append Trace, or depend on commit, sim, infer, ai, or eval. Platform adapters may read Trace/saves/package identity; they do not mutate Projection.
- Runtime (not stream) builds `Proposal::Residency`. Stream returns `Arc<PlaceSnap>` after header-validate + mmap.
- `InferHost::{new,submit,poll}` may appear only in `engine/crates/klotho-runtime/**` and `engine/crates/klotho-infer/**` (CI allowlist).
- Gameplay (`engine/examples/hearth-slice`, `engine/examples/ash-slice`, ember, drift, `klotho-author`, `klotho-editor`) may not import `klotho-manifest::tables` or `klotho-stream`.
- `klotho-dialogue` lowers at cook and cannot be imported by gameplay slices. It does not depend on commit, world, infer, or ai.
- `klotho-eval` may depend on public debug/runtime test interfaces but may not enable `klotho-world/mutate`, append Trace, or mint player Agency. Engine never depends on `klotho-eval`.
- `klotho-live` is an out-of-band service plane over `klotho-net`; it does not depend on world, commit, trace, or runtime. Matchmaking, experiments, moderation, anti-cheat, and telemetry never mutate Projection.

## PR plan (merge order)

PR 01 `klotho-core` → 02 prove → 03 ir → 04a/b canon → 05 trace → 06 world → 07 commit → 07b Hearth goldens → 07c Ash goldens → 08 sim/runtime → … see HLD.

Do not grow Hearth. Ash is the generality gate (K26). If Ash needs `DamageComponent`, the ontology has leaked.

## First-title freeze (AAA-27)

- Ship profile: single-player action-adventure, `RuntimeProfile::AaaAdventure`, 30 Hz authoritative simulation and 60–120 Hz presentation on desktop Windows/Linux/macOS. Infer stays default-off.
- Release-blocking slices: Hearth and Ash (ontology/determinism), Ember (action combat), Drift (Phys + two-Place residency), and Chorus (SimLod scale). Keep them as bounded goldens; do not turn them into title content.
- Shooter-only gates are maintained regressions, not first-title blockers: `RuntimeProfile::AaaShooter`, dedicated 60 Hz simulation, lag compensation/rewind acceptance, Netlock release acceptance, multiplayer lobby scale, and the competitive render permutation.
- Console certification, live epoch deployment, GPU particles, runtime infer, marketplace/UGC, localization/UMG, 64-player scale, and virtualized geometry are post-title-one work. Their landed boundaries and tests stay intact. KAI-23 and KAI-24 landed P0 public console and multiplayer/live boundaries; production/certification claims still require signed P1 confidential evidence and P2 external acceptance.

## Vocabulary

Author-facing: Locus, Canon, Intent, Trace, Manifest, Rite, Law.
Runtime: Proposal, CommitKernel, Affordance, Predicate, Pin, Sigil.
Do not leak `AdmitBuf` / `SyncProposer` into Distaff docs.
