//! Kernel numbers, identities, and commit-path primitives for Klotho.
//!
//! This crate **freezes K20**: committed space is millimetres (`i32`), velocity
//! is 16.16 fixed-point millimetres per tick, yaw is millidegrees. Presenters
//! may float; the kernel never does.
//!
//! It also owns the single deterministic [`Rng`] (K25), seeded from
//! `canon_hash ⊕ tick`.
//!
//! `#![forbid(unsafe_code)]` — all other kernel crates inherit this rule.
//! Unsafe is allowed only in `klotho-infer`, `klotho-render`, `klotho-audio`,
//! `klotho-platform`, and `klotho-jobs` (steal queues).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod budget;
mod hash;
mod id;
mod reject;
mod rng;
mod space;
mod tick;
mod units;
mod yaw;

pub use budget::Budget;
pub use hash::{BlobId, Hash};
pub use id::{
    AffordanceId, LOD_PERIOD, LawId, LocusKind, MAX_ISLAND_SIZE, MAX_ISLANDS, MAX_LOCI_HEARTH,
    MAX_LOCI_PROCESS, NO_ISLAND, PackedIx, PlayerId, ResourceId, Sigil, SimLod,
};
pub use reject::{KernelFault, RejectReason};
pub use rng::Rng;
pub use space::{AabbMm, HullWitness, IVec3, PhysRequest, PoseMm, Support, Vel3, frac_cmp};
pub use tick::{Epoch, Tick};
pub use units::{Mm, VelFx, YawMd};
pub use yaw::{look_offset, rotate_xz};
