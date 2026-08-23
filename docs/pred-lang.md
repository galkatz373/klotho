# Predicate and Rite language (PR 04a)

Author-facing AST lives in `klotho-ir`. This document is the grammar, caps, and **cook CFG** that `klotho-canon` enforces. Pred eval is `klotho-canon`. The rite interpreter is `klotho-commit`.

Closed-world, two-valued: failing to prove is **false**. No nested quantifiers. No recursion. No string match. Arithmetic is `Qty` compare only.

## Slots

`Self | Target | Other | Name(id)`

Every `Name(id)` is a cook-time pin to a seed Sigil. Unbound names fail cook.

## Atoms

| Atom | Meaning |
| --- | --- |
| `Affordance(s, a)` | projection bitset |
| `Rel(a, r, b)` | relation table |
| `Qty(s, res, cmp, n)` | `Lt Le Eq Ge Gt`, i32 |
| `EqVerb(Verb)` | current proposal verb |
| `RiteActive(id)` | `RiteMachine` row exists |
| `AabbNear(a, b, Mm)` | conservative AABB distance |
| `InWindow(rite, ch)` | current tick in that WAIT |
| `Knows(mind, fact)` | knows table |
| `SourceIs(kind)` | Player, Mind, Space, Motion, Infer |
| `AgencyClaimed(ch)` | on the *current* proposal |
| `Burning(s)` | sugar: `Qty(s, heat) Ge 400` |
| `OpaqueClosed(s)` | Opaque ∧ `LockedBy` (not an `Open` bit) |
| `SweptHitsOpaqueClosed` | current sweep vs any OpaqueClosed hull |
| `IslandAwake(s)` | `sleep_ticks == 0` |
| `SelfIs` / `TargetIs` / `OtherIs` | slot equality |

Combinators: binary `And` / `Or`, `Not`. `ExistsRelated` / `CountRelated` scan cap 64; `pred` is quantifier-free.

Sugar (cook desugar): `Burning(s)` → `Qty(s, heat) Ge 400`. `OpaqueClosed(s)` → Opaque ∧ `ExistsRelated` `LockedBy`. `Possessed` is not in the IR (authors write `Rel(..., WieldedBy, ...)`).

## LawBody

`Pred { must, ought }` | `Ramp` | `Spread` | `Conserve` | `Cap`

Continuous writers are only Ramp and Spread. `SETQ` mutates quantities only; relations use `REL_ADD` / `REL_DEL`.

## Rite ISA (12 ops)

`HALT` `GUARD` `SPEND` `WAIT` `EMIT` `BRANCH` `BIND` `SETQ` `REL_ADD` `REL_DEL` `AWAKE` `COMPLETE`

Canonical RON tags nodes `Op(...)` / `Labeled(pc: n, op: ...)`. Appendix A also writes bare ops and `{ pc: n, op: ... }`; `klotho_ir::from_ron` rewrites those.

`WAIT` commits the speculative burst (K21) and yields. Resume is a new transaction.

## Cook CFG (fail cook)

1. Do not mix unlabeled ops and explicit pcs in one graph.
2. Duplicate pcs fail. `entry` must exist.
3. Unlabeled nodes receive sequential pcs `0..n-1` in list order.
4. Fall-through is **list order**, not `pc+1` (gaps are allowed).
5. `COMPLETE` / `HALT` have **no** fall-through successor.
6. `GUARD` / `SPEND`: success → next in list; fail → `fail_pc` (must exist).
7. `BRANCH`: both targets exist; no implicit next.
8. Sequential ops and `WAIT` require a next node (no fall-off).
9. Every node is reachable from `entry`.
10. The successor graph is a **DAG**. `WAIT` edges are forward resumes, not loops. Backward `BRANCH` fails cook.

The rev-4 unlabeled `trade.offer` (`Complete(Success)` then dead `RelDel`) fails (4)+(5)+(9).

## Caps

| Cap | v1 |
| --- | --- |
| Pred ops / eval | 64 |
| Pred ops / tick | 8,192 |
| Related scan | 64 |
| Rite steps / rite / tick | 64 |
| Rite steps / tick | 2,000 |

Exceed → `RejectReason::Budget` at runtime. Cook does not execute world preds; it does tiny-fragment contradiction and the `Lockable` key-or-rite check.

## Cooked types

`PredChunk` ops: `PushAtom`, `And`, `Or`, `Not`, `ExistsRelated`, `CountRelated`, `Halt`.

`PredProgram` wraps the chunk with interned `Atom`s and `RelatedScan` payloads. `RiteChunk`: labeled `RiteInstr { pc, op }` plus caps; Guard/Branch preds compile to `PredProgram`s on `CookedRite`.
