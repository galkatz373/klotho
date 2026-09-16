# Klotho Physics and Animation-Contact Plan

| Field | Value |
| --- | --- |
| Status | Active — PHYS-A01–A11 landed; PHYS-A12 next |
| Date | 2026-09-16 |
| Scope | Production 3D physics, character resolution, vehicles, destruction, and animation-driven contact |
| Preserves | Canon + Intent + Trace + Projection; K21 atomic commit; crate firewalls |

## Outcome

Klotho needs more than a stronger solver. It needs physical interaction to be authoritative without creating a second world:

> Canon defines physical meaning and authored constraints. Physics and Motion derive bounded proposals. `CommitKernel` atomically admits them. Trace records gameplay consequences. Manifest presents the result.

The landed `klotho-phys` establishes the correct authority boundary and proves gravity, oriented contact, stacking, character drive, animation-contact admission, Canon-bound four-wheel vehicle rigs, and admitted constraint breaks with fragment caps. PHYS-A11 adds a bounded combined Anvil slice and read-only diagnostics; PHYS-A12 owns release acceptance.

Because the existing AAA sequence is marked complete, this work starts with an HLD amendment and a new PR series. It must not silently expand the claims of AAA-08 through AAA-10.

## Non-negotiable contract

- Physical configuration—shape, mass, material, joints, and vehicle rig—is Canon.
- Pose, velocity, sleep, support, active physical constraints, and authoritative action phase are Projection.
- Solver caches are disposable. They may affect a result only if their required state is represented in Projection.
- Physics and Motion receive read-only views and emit bounded proposals. They never mutate Projection.
- Every gameplay-visible contact is admitted by `CommitKernel` and validated against canonical geometry.
- Ragdolls, cloth, sparks, and cosmetic debris remain Manifest-only and are never read back into gameplay.
- Rite `WAIT` remains the authority for attack timing. Animation provides the spatial contact trajectory inside that window.
- Clip notifies never change pose, quantities, relations, health, or other authoritative state.
- A coupled physical solution is admitted atomically at contact-island granularity.
- Committed pose and velocity retain the frozen `klotho-core` integer formats. No `f32` enters the commit path.
- The only RNG remains `klotho_core::Rng`, seeded according to the HLD.

## Acceptance definition

Before implementation, freeze bounded golden cases for the following behaviors:

- Rotated crates fall, topple, stack, sleep, and wake.
- A character walks up stairs and permitted slopes without tunnelling or hovering.
- Root motion into a wall stops physically and visually.
- A sword damages only a target crossed by its cooked weapon trajectory during the active Rite window.
- A four-wheel vehicle accelerates, turns, brakes, loses grip, and rests on a slope.
- A hinge and a breakable constraint behave consistently.
- A moving platform carries a character.
- Save/load and replay preserve every authoritative result above.

These are engine goldens, not title content.

## Phase 1 — Atomic physics admission

Before PHYS-A02, the solver emitted an independent `PhysDelta` for each body. That could admit only part of the coupled solution when a neighbor rejected.

Replace the transaction grain with the bounded island proposal accepted by K59–K60, shaped like:

```rust
Proposal::PhysIsland {
    island,
    members: Vec<Sigil>,
    bodies: Vec<BodyDelta>,
    contacts: Vec<ContactClaim>,
    constraints: Vec<ConstraintRef>,
    breaks: Vec<ConstraintBreakClaim>,
}
```

PHYS-A01 accepts the outer names and consensus ordering/caps. PHYS-A02 owns the exact Rust field layout and wire tags.

Admission must:

1. Require bodies, contacts, and constraints in canonical Sigil order.
2. Verify island identity, membership, hull bindings, epoch, uniqueness, and payload caps.
3. Compute the complete write set, including attached children.
4. Begin one speculative transaction.
5. Apply every body delta to the speculative Projection.
6. Validate contacts and final penetration against canonical hulls.
7. Evaluate applicable Laws in deterministic `(LawId, body Sigil)` order against the complete proposed island state.
8. Commit the entire island or reject the entire island.

`write_cells` must cover every body, support row, velocity row, constraint row, and attached child affected by the batch. One invalid member rejects the whole proposal without a partial write.

PHYS-A02 removed legacy `PhysDelta`; all registered runtime physics paths now emit the island transaction.

The current `SweptHitsOpaqueClosed` behavior must also be refined:

- Resting or resolving contact is legal.
- Residual penetration beyond tolerance is illegal.
- Crossing a closed barrier is illegal.
- A gameplay contact requires a validated witness, not only a swept-AABB hint.

### Phase 1 gates

- Deliberately invalidate one crate in a solved stack; no member of that island commits.
- Reordering proposal payloads rejects deterministically; the kernel never silently canonicalizes it.
- Duplicate bodies and mismatched partition membership reject.
- An island rejection leaves Projection byte-identical.
- Existing Hearth and Ash hashes remain unchanged when Phys is disabled.

## Phase 2 — Shared deterministic geometry

Create the pure geometry boundary accepted by K61 and shared by `klotho-phys` and `klotho-commit`; commit must not depend on phys. Use a narrowly scoped `klotho-geom` crate depending only on `klotho-core`. Canonical identifiers and quantized witness types remain in `klotho-core`.

Add cooked canonical shapes in this order:

1. Oriented box
2. Sphere and capsule
3. Convex hull
4. Compound hull
5. Static triangle mesh or heightfield

Required deterministic or conservatively verifiable queries are:

- Broad-phase bounds
- Convex distance and penetration
- Ray and shape casts
- Swept capsule and convex contact
- Conservative continuous collision detection
- Contact-witness verification

`HullWitness` should identify canonical shapes and carry bounded, quantized evidence. It must not become an opaque solver manifold that the kernel blindly trusts.

Static terrain geometry is occupancy, not a giant dynamic island. Place and static environment hulls must never connect otherwise unrelated bodies into one island.

### Phase 2 gates

- Rotating a box changes its contact geometry.
- Capsules traverse box, convex, and static-mesh boundaries consistently.
- Fast bodies cannot tunnel through the thin-wall golden.
- Malformed, stale-epoch, wrong-hull, and non-finite witnesses fail closed.
- Broad-phase candidate order cannot change the admitted result.

## Phase 3 — Production rigid-body dynamics

Evolve the current scalar solver incrementally:

- Orientation-dependent collision geometry
- Angular integration, angular impulses, and torque
- Per-body mass, centre of mass, and inertia tensor
- Static, kinematic, and dynamic modes derived from Canon and relations
- Canonical physical materials with friction and restitution
- Stable multi-point contact manifolds
- Continuous collision detection for fast bodies
- Deterministic sleeping and waking
- Fixed, hinge, slider, and spring constraints
- Constraint impulse reporting for breakable joints

All solver inputs must be reconstructible from `(Canon, Projection, tick)`. If warm-starting is required for stability, its bounded impulses must become Projection data. Hidden warm-start caches remain forbidden.

Prefer scalar correctness and goldens before SIMD. Any SIMD path must preserve the pinned-ISA contract already established by K44.

### Phase 3 gates

- Long-run stack drift remains within the frozen tolerance.
- A side impact topples an asymmetric stack.
- Boxes settle on multiple slope angles according to friction.
- Restitution produces bounded, repeatable bounce behavior.
- Joint chains remain stable within the declared body and iteration caps.
- One-worker and eight-worker execution produce the same pinned-physics Trace hash.

## Phase 4 — Character and physics coupling

The current exclusive split—Motion owns Actors while Phys skips them—cannot express robust stairs, slopes, moving platforms, or pushing dynamic bodies.

Introduce a deterministic character drive constraint:

- Motion samples the semantic root trajectory for the active locomotion phase.
- That trajectory is desired displacement, not guaranteed displacement.
- Character resolution performs capsule sweep, grounding, depenetration, step-up, slope limiting, platform following, and dynamic-body interaction.
- The resolved pose is the only pose proposed for admission.
- Presented animation derives correction and foot phase from the admitted result.

Motion must not call Phys directly. Use one bounded, pure character proposer composed by runtime, or let the physics proposer consume a drive description deterministically derived from the same `WorldView`. Do not introduce a hidden mutable queue or proposer-to-proposer state.

K63 selects one spatial owner for the actor per tick, with root motion represented as a drive constraint inside the solved island rather than a competing `MotionDelta`.

PHYS-A06 binds `BodyPhysics::character` through the authored `Physics` seed fact.
The binding contains at most eight looping, quantized root samples and the step/slope
policy. Motion exposes the pure drive from the same read-only view consumed by Phys;
it emits no `MotionDelta` for these actors. Unbound actors retain the legacy path.
The shared integer capsule resolver uses bounded sweep intervals, up/forward/down
steps, grounding and depenetration. Moving kinematic platforms join the actor's
island and supply translation plus rotation from Projection, without a hidden
previous-pose cache. Dynamic crates resolve in that same atomic transaction.
The kernel reproduces the character path against static canonical geometry and
checks final penetration against the complete proposed island. Presentation keeps
using the admitted root; snapshot-pair extraction selects foot locomotion from
admitted displacement and presents blocked roots and idle platform riders at rest. Queries cap occupancy at 512 obstacles and displacement
at 1000 mm per axis; a failed query emits no island and identifies the actor on
`SolveOut`. The combined Anvil capture/overlay work remains PHYS-A11, and the
rejected-root/melee gate is landed by PHYS-A08.

### Phase 4 gates

- Walk up and down stairs at the frozen riser heights.
- Stand, start, stop, and turn on permitted inclines.
- Reject movement on slopes beyond the canonical limit.
- Ride a translating and rotating platform.
- Push a dynamic crate without interpenetration.
- Root motion into a wall stops both the admitted root and its presented correction.
- A rejected character solution cannot generate an admitted melee contact.

## Phase 5 — Semantic animation-contact tracks

Extend the cook pipeline with an authoritative artifact, referenced by Canon, that contains:

- Quantized root trajectory at authoritative tick boundaries
- Named semantic sockets such as hand, foot, and weapon grip
- Bounded swept capsules or convex volumes per action phase
- Foot-plant intervals when a Law or character constraint needs them
- A compatibility signature tying skeleton, instrument binding, action, and Rite timing together

K64 accepts `ContactTrack` as the artifact name. It is semantic motion data, not a dump of presenter bone palettes.

The visual clip remains Manifest data. A visual clip may be swapped without changing gameplay timing only when its cooked contact signature is compatible with the Canon-bound track. An incompatible change requires a Canon epoch change.

This replaces the overly weak guarantee that any ClipSet swap leaves melee unchanged. The desired guarantee is:

> Cosmetic animation variation may not change Trace, while a changed authoritative contact trajectory is an explicit Canon change.

PHYS-A07 binds an approved `ContactTrack` to an Actor through a configuration-only
seed fact. The first track format uses swept capsules, up to 65 authoritative
boundaries, eight named sockets, eight channels, and sixteen non-overlapping
foot-plant intervals. Absolute root-local coordinates are bounded to 100 m and
capsule radii to 1–2000 mm. Sampling is explicitly 30 or 60 Hz and names the exact
labeled, channel-bearing Rite `WAIT` instruction and its duration. Convex channels
can extend this artifact later; they are not encoded by the first format.

Cook writes a validated `KLTH` kind-11 CAS artifact with canonical integer RON
payload and a 128 KiB pre-parse cap. The compatibility signature hashes the entire
semantic artifact, including skeleton, instrument, action, timing and geometry.
Approved DCC semantic sidecars are separate from visual glTF palettes; contact-bound
animated imports require them and fail cook when identities, roots, timing,
channel topology or capsule radii differ. Socket and capsule-endpoint retargeting
must stay within the frozen 5 mm Euclidean envelope. Cosmetic mesh/palette bytes
stay outside the signature. Warp reconstruction preserves the Canon binding;
`cook_contact_epoch_pack` explicitly advances the epoch when an approved trajectory
changes. No runtime clip notify or sampled palette is consumed by simulation.

Distaff exposes a read-only Actor contact preview from cooked Canon, with desired
root, named sockets, capsule intervals, foot plants, compatibility identity and
cooked WAIT-window status at an authoritative boundary. It does not write Intent,
Trace or Projection. Kernel hit admission remains PHYS-A08.

### Phase 5 gates

- Contact samples are invariant to render rate and interpolation.
- Retargeting stays within the frozen socket/contact error tolerance.
- An incompatible clip or instrument binding fails cook.
- Cosmetic changes preserving the compatibility signature preserve Trace.
- Changing an authoritative contact track changes the Canon hash and epoch.

## Phase 6 — Animation-driven contact admission

Add a generic motion-contact proposal rather than a combat subsystem or component bag. Conceptually:

```rust
MotionContact {
    rite_instance,
    channel,
    actor,
    instrument,
    target,
    sweep,
    witness,
}
```

K64 requires this to be a bounded part of the character island proposal whenever root resolution can change the weapon sweep. A rejected root pose therefore cannot leave a detached contact.

The kernel admits a contact only when:

- The referenced Rite is active in the correct `WAIT` window.
- The Rite originated from valid Agency.
- Motion has not minted or extended that Agency.
- The contact artifact matches the Canon binding and epoch.
- The swept instrument volume intersects the target's canonical hull.
- The target satisfies the required affordances and Laws.
- Per-action hit caps and duplicate-hit rules are satisfied.

The admitted contact advances the Rite or supplies evidence to a Law that emits the semantic hit and quantity change. Trace records the hit and its semantic participants, not every sampled bone pose.

### Phase 6 gates

- A visible miss cannot damage the target.
- A visible crossing inside the active window produces one admitted hit.
- The same crossing outside the window produces no hit.
- One swing cannot hit the same target twice unless Canon permits it.
- A blocked or rejected character root cannot leave a detached hit sweep behind.
- Infer and Mind cannot claim the player's melee channel or Agency.

PHYS-A08 binds the active Canon WAIT to a player-authorized action instance and
admits `MotionContact` only inside the coupled physical island. The proposal carries
the exact Canon `ContactTrack`, channel, interval and bounded narrow-phase sample;
the kernel reproduces the sample against the resolved root and the target hull.
A contact evidence Trace event records only semantic participants and the approved
instrument identity. Its action ledger, original player channel and absolute WAIT
clock persist in Projection and snapshot v3; a seeded or stale Rite cannot supply
Agency. One target/contact per action is the initial bounded policy. Contact
consequences, Laws, spawned rows and body writes are included in the same
speculative write set, so a Law, witness, root, or member rejection leaves the
Projection byte-identical. Runtime jobs stamp the next authoritative tick, and
stream Place rows refuse live action metadata. Bounded integer capsule sampling
fails closed on query overflow; broad-phase AABB hints never establish a hit.

### Phase 6 gates — landed

- Rotated-box, convex-void, thin-wall, small-radius and excess-work queries are covered by pure geometry tests.
- Hits, visible misses, stale windows, blocked roots, invalid members and Law rejection are covered by Anvil-style kernel tests.
- Save/replay retains the action instance, original player authorization, WAIT clock and contact ledger.
- Mind and Infer cannot start, resume, or mint the player contact channel.
- One and eight worker execution produce identical contact Trace and snapshots.

## Phase 7 — Vehicles

Represent a vehicle with the existing `Driveable` affordance plus a Canon-bound physical rig, not a vehicle component hierarchy.

Implement:

- Chassis convex or compound body
- Wheel ray casts or shape casts
- Spring and damper suspension
- Longitudinal and lateral tire friction
- Steering, throttle, and braking driven from `PhysRequest`
- A bounded engine and gearing model only to the fidelity required by the title
- Grounding on slopes and transitions between canonical surface materials

`PilotedBy` remains the semantic relationship. The driver does not become a second spatial owner, and presented wheel rotation is derived from admitted chassis/wheel state.

Keep Drift bounded. If full vehicle regressions would turn it into title content, add a separate vehicle-physics slice and retain the original Drift goldens unchanged.

PHYS-A09 binds `BodyPhysics::vehicle` through the authored `Physics` seed fact.
The first rig is two-to-four wheels with quantized hub positions, rest length,
spring/damper permille, radius, steer envelope, and longitudinal/lateral tire
scales. Phys consumes throttle, brake and steer from chassis or attached-driver
`PhysRequest`; it does not mint a second spatial owner. Wheel rays use shared
integer geometry against occupancy, including oriented boxes and heightfield
ramps. Surface friction comes from the occupancy material. Tire caches stay
disposable. Unbound Driveable relics keep the legacy one-shot Δv fold, so
original Drift goldens are unchanged. Presented wheel rate derives from the
admitted chassis velocity and Canon radius. Breakable structures are landed by PHYS-A10.

### Phase 7 gates

- Accelerate, coast, brake, reverse, and steer.
- Lose lateral grip under a frozen excessive-speed case.
- Different canonical surface materials produce the expected traction ordering.
- Suspension settles on flat ground and a slope.
- Driver attach and detach never produce competing Motion and Phys commits.
- Save/load during motion resumes from the exact authoritative state.

## Phase 8 — Destruction and physical constraints

Canonical breakable structures use relations and constraints:

- Canon defines pieces, connections, and break thresholds.
- Phys proposes a constraint break with a bounded impulse witness.
- Commit validates and admits the semantic break.
- Projection changes the relevant relation or authoritative state.
- A bounded number of fragments become authoritative physical loci.
- Remaining dust, chips, and debris are Manifest TTL presentation.

A presentation fracture may never determine whether a passage is open, an object is destroyed, or a Law can proceed.

PHYS-A10 admits `ConstraintBreakClaim` as the semantic break. The kernel RelDels
`PartOf` between the joint endpoints, records one `ConstraintBroken` evidence
event, and spawns Canon `fragments` (0..=64) as Fragment Relics with hulls in
the same speculative write set. Global live-fragment count is 128, matching
Ember; a tighter Cap Law still rejects the island. Cosmetic chips extract as
Manifest TTL `OneShotMesh` with no Sigil. Anvil capture/overlay remains PHYS-A11.

### Phase 8 gates — landed

- Below-threshold impulses do not break the constraint.
- Above-threshold impulses produce one semantic break.
- Fragment counts respect the existing global and per-collapse caps.
- Cosmetic debris cannot collide with or damage authoritative loci.
- Save/load after a break restores the same relations and authoritative fragments.

## Proving slices

Do not grow Hearth. Keep Ember and Drift as bounded ontology and subsystem regressions.

Add one small physics/contact slice, provisionally called **Anvil**, containing:

- A rotatable five-crate stack
- Stairs and two slope angles
- A hinged obstacle
- A translating and rotating platform
- A character that pushes a crate
- One sword attack with a cooked weapon track
- One breakable constraint

Anvil proves rigid-body, character, and animation-contact coupling on the same `CommitKernel`. It is not a level, benchmark scene, or source of new gameplay ontology.

Drift continues to own vehicle and two-Place residency goldens. Ember continues to own melee Rite/Law ontology, hit caps, and destruction caps. Add only the minimum integration assertions needed to show that the new physical evidence feeds those existing semantics.

## Distaff and diagnostics

Distaff needs read-only or proposal-preview overlays for:

- Canonical collision shapes
- Contact points and normals
- Character capsule, step, and slope decisions
- Root drive versus resolved displacement
- Weapon sweeps and active Rite windows
- Island membership and sleep state
- Constraint forces and break thresholds
- Proposed versus admitted poses
- Exact kernel rejection reason

An island capture must be replayable from its Canon identity, epoch, Projection snapshot, tick, and proposal payload. Editing an overlay never changes the world unless the user performs an existing Pin/Canon authoring action.

## Determinism and platform policy

- Pinned Linux produces exact Phys Trace goldens using the existing K44 toolchain and floating-point restrictions.
- One-worker and eight-worker proposal execution must match exactly on that artifact.
- Windows, macOS, and Linux must pass behavioral envelopes for every release-blocking physics case, even where cross-OS Phys Trace equality is not claimed.
- Cross-platform save portability must either be explicitly supported by a stronger determinism result or explicitly rejected at load. It may not be accidental.
- Presentation rate, renderer, skinning, IK, and cosmetic animation variation must not alter authoritative Trace.
- Non-finite solver output emits no proposal and fails closed with diagnostics.

## Performance gates

Measure each stage independently:

- Partition and broad phase
- Narrow phase and contact generation
- Constraint solve
- Character solve
- Contact-track sampling
- Proposal encoding and sort
- Kernel witness validation
- Speculative island admission
- Snapshot publication

Retain the existing AAA adventure and shooter critical-path budgets. If the richer solver cannot meet them, reduce awake physical complexity through interest, sleeping, and bounded island policy before weakening authority or determinism.

No gate may be expressed only as average frame time. Record p50, p95, p99, maximum bounded workload, body count, contact count, constraint count, and island size.

## PR sequence

Each PR leaves the tree green and preserves existing non-Phys goldens.

1. **PHYS-A01 — Authority and transaction RFC — landed**
   Amend the HLD; freeze acceptance scenes, caps, shape vocabulary, island rejection policy, character ownership, and contact authority.

2. **PHYS-A02 — Atomic `PhysIsland` admission — landed**
   Add bounded batch proposal, complete write sets, full speculative apply, deterministic Law order, rejection tests, and legacy migration.

3. **PHYS-A03 — Shared geometry and oriented primitives — landed**
   Land the shared dependency boundary, oriented boxes, spheres, capsules, casts, and witness validation.

4. **PHYS-A04 — Convex/compound contact and CCD — landed**
   Add convex and compound hulls, stable manifolds, friction, restitution, mass/inertia, angular response, and thin-wall goldens.

5. **PHYS-A05 — Static terrain and constraints — landed**
   Add static mesh/heightfield queries, slopes, fixed/hinge/spring constraints, break witnesses, and sleeping/waking gates.

6. **PHYS-A06 — Character drive constraints — landed**
   Resolve root desire through capsule movement, steps, slopes, platforms, and dynamic-body interaction with one spatial owner.

7. **PHYS-A07 — Semantic contact-track cook — landed**
   Cook quantized socket/sweep tracks, enforce compatibility signatures, and add Distaff preview overlays.

8. **PHYS-A08 — Motion-contact admission — landed**
   Bind active Rite windows to validated instrument sweeps; prove spatially aligned melee without AnimNotify authority.

9. **PHYS-A09 — Vehicle rigs — landed**
   Add chassis, suspension, tires, surface friction, `PhysRequest` controls, and expanded bounded Drift regressions. Unbound Driveable relics keep the legacy PHYS_REQ Δv fold so original Drift goldens stay put.

10. **PHYS-A10 — Breakable structures — landed**
    Admit constraint breaks, authoritative fragment caps, and Manifest-only debris.

11. **PHYS-A11 — Anvil and production diagnostics — landed**
    The bounded Anvil scene contains five crates with a rotated middle body, 200/250 mm stairs, 30°/50° slope geometry, hinge and breakable constraints, a translating/rotating platform with rider, a character pushing a crate, and a Canon-bound sword track. A 30-tick combined golden admits one sword hit and one break while push and platform motion use the same kernel. An invalid crate hull rejects its entire island byte-identically. Published-snapshot island captures retain Canon identity, epoch, tick and exact proposal payload; pure replay compares the proposal. Read-only overlays expose hulls, prior/proposed poses, character desire/resolution and step/slope policy, WAIT sweeps, support/sleep, and constraint impulses/thresholds. Distaff can request a capture without admitting it. Reject explanations name the kernel reason. Disposable timings record broad/character/vehicle/narrow/constraint/encode stages and p50/p95/p99/max summaries with body/contact/constraint/island-size maxima. Captures are in-memory diagnostics; pinned Linux, supported-OS, persistence, and long-run envelopes remain PHYS-A12 gates.

12. **PHYS-A12 — Release acceptance**
    Run pinned determinism, supported-OS behavior, save/load, replay, scale, long-run stability, and crate-firewall gates; update HLD claims only after evidence lands.

## Completion checklist

This program is complete only when:

- No gameplay-visible physical contact bypasses `CommitKernel`.
- A rejected physical island leaves Projection byte-identical.
- Resting contact is distinguishable from illegal penetration or barrier crossing.
- Characters, rigid bodies, platforms, and instruments cannot acquire competing spatial owners.
- A sword hit requires both a valid Rite window and validated spatial contact.
- Presenter animation can be disabled without changing gameplay Trace.
- Cosmetic clip swaps preserve Trace only when their contact compatibility signature matches.
- Vehicles use admitted wheel/chassis physics rather than scripted translation.
- Destruction changes authoritative relations only through admitted proposals.
- Save/load during a stack, joint motion, vehicle motion, or attack resumes exactly under the declared platform policy.
- Phys-off profiles retain existing Hearth and Ash behavior.
- `klotho-sim`, `klotho-commit`, `klotho-save`, `klotho-release`, gameplay, and `klotho-world/mutate` firewalls remain intact.

The critical path is atomic island admission, shared geometry, character resolution, and semantic weapon trajectories. Vehicles and broad destruction follow that foundation; they must not force special-case authority paths into the kernel.
