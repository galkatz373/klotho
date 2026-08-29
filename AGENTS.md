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
- `klotho-world` feature `mutate` is enabled **only** by `klotho-commit`. `klotho-stream` does not enable `mutate`.
- Runtime (not stream) builds `Proposal::Residency`. Stream returns `Arc<PlaceSnap>` after header-validate + mmap.
- `InferHost::{new,submit,poll}` may appear only in `crates/klotho-runtime/**` and `crates/klotho-infer/**` (CI allowlist).
- Gameplay (`examples/hearth-slice`, `examples/ash-slice`, `klotho-author`) may not import `klotho-manifest::tables` or `klotho-stream`.

## PR plan (merge order)

PR 01 `klotho-core` → 02 prove → 03 ir → 04a/b canon → 05 trace → 06 world → 07 commit → 07b Hearth goldens → 07c Ash goldens → 08 sim/runtime → … see HLD.

Do not grow Hearth. Ash is the generality gate (K26). If Ash needs `DamageComponent`, the ontology has leaked.

## Vocabulary

Author-facing: Locus, Canon, Intent, Trace, Manifest, Rite, Law.
Runtime: Proposal, CommitKernel, Affordance, Predicate, Pin, Sigil.
Do not leak `AdmitBuf` / `SyncProposer` into Distaff docs.
