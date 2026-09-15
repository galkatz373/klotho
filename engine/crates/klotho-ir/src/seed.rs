//! Seed facts: the initial Trace prefix, authored as names.

use serde::{Deserialize, Serialize};

use klotho_core::{LocusKind, PoseMm};

use crate::error::IrError;
use crate::name::Name;
use crate::rel::Rel;

/// One fact in the seed Trace. Cook allocates Sigils and writes the prefix.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub enum SeedFact {
    /// Allocate a locus.
    Locus {
        /// Authoring name (`"oak_door"`).
        name: Name,
        /// Kind packed into the Sigil.
        kind: LocusKind,
    },
    /// Relation row.
    Rel {
        /// Subject name.
        a: Name,
        /// Edge.
        rel: Rel,
        /// Object name.
        b: Name,
    },
    /// Quantity row.
    Qty {
        /// Locus name.
        of: Name,
        /// Resource name (`"mass_g"`).
        res: Name,
        /// Initial value.
        value: i32,
    },
    /// Canon physical configuration; does not append a Trace event.
    Physics {
        /// Bound locus name.
        of: Name,
        /// Canonical shape, material and optional character drive.
        body: klotho_core::BodyPhysics,
    },
    /// Canon semantic motion binding; never appends Trace.
    ContactTrack {
        /// Actor name.
        of: Name,
        /// Approved quantized trajectory.
        track: klotho_core::ContactTrack,
    },
    /// Initial pose.
    Pose {
        /// Locus name.
        of: Name,
        /// Pose.
        pose: PoseMm,
    },
}

impl SeedFact {
    pub(crate) fn check(&self) -> Result<(), IrError> {
        match self {
            Self::Locus { name, .. } => name.check(),
            Self::Rel { a, b, .. } => {
                a.check()?;
                b.check()
            }
            Self::Qty { of, res, .. } => {
                of.check()?;
                res.check()
            }
            Self::Pose { of, .. } => of.check(),
            Self::ContactTrack { of, track } => {
                of.check()?;
                if track.is_valid() {
                    Ok(())
                } else {
                    Err(IrError::Parse(format!("invalid contact track for {of}")))
                }
            }
            Self::Physics { of, body } => {
                of.check()?;
                if body.is_valid() {
                    Ok(())
                } else {
                    Err(IrError::Parse(format!(
                        "invalid physical configuration for {of}"
                    )))
                }
            }
        }
    }
}
