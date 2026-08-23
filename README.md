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
Next: PR 03 `klotho-ir`.

## Build

Rust 1.85+ (edition 2024). `rustup` reads `rust-toolchain.toml`.

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

## Layout

```
klotho/
  crates/klotho-core/     # Tick, Mm, VelFx, YawMd, Sigil, Budget, Hash, Rng
  crates/klotho-prove/    # Provenance DAG, LicenseSpan, blake3 CAS
  docs/hld.md             # High-level design (rev 5)
```

Further crates land in the order of the HLD PR plan. `klotho-sim` will never depend on `klotho-infer`. Unsafe is forbidden except in `klotho-infer`, `klotho-render`, `klotho-audio`, and `klotho-platform`.
