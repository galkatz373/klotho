//! Typed authoring operations and change-set envelope.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use klotho_author::AnchoredSeedFact;
use klotho_core::{Hash, LocusKind};
use klotho_ir::{AnchorId, CanonDiff, IntentModule, Name, ParameterValue};

use crate::ids::{AssetRequestId, ChangeId, ReferenceId};

/// Allowed module/anchor set. Empty sets mean unrestricted.
#[derive(Clone, Eq, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangeScope {
    /// Module identities this change may write.
    pub modules: BTreeSet<AnchorId>,
    /// Object identities this change may write.
    pub anchors: BTreeSet<AnchorId>,
}

impl ChangeScope {
    /// No restriction.
    #[must_use]
    pub fn unrestricted() -> Self {
        Self::default()
    }

    /// True when `id` is writable under this scope.
    #[must_use]
    pub fn allows(&self, id: AnchorId) -> bool {
        if self.modules.is_empty() && self.anchors.is_empty() {
            true
        } else {
            self.modules.contains(&id) || self.anchors.contains(&id)
        }
    }
}

/// Operation budget for one transaction.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TxBudget {
    /// Maximum ops that may be applied.
    pub max_ops: u32,
}

impl Default for TxBudget {
    fn default() -> Self {
        Self { max_ops: 10_000 }
    }
}

/// Acceptance contract stub.
#[derive(Clone, Eq, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceContract {
    /// Scope the contract permits.
    pub allowed_scope: ChangeScope,
    /// Optional free-form claims.
    pub claims: Vec<String>,
}

/// Provenance of a change set.
#[derive(Clone, Eq, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoringProvenance {
    /// Hash of the originating request bytes.
    pub request: Hash,
    /// Parent change, if this is a repair.
    pub parent: Option<ChangeId>,
}

/// Pattern argument stub.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatternArg {
    /// Parameter name.
    pub key: Name,
    /// Bound value.
    pub value: ParameterValue,
}

/// Pattern instance stub.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatternInstance {
    /// Instance identity.
    pub anchor: AnchorId,
    /// Authoring name.
    pub instance: Name,
    /// Pattern id.
    pub pattern: Name,
    /// Pattern version.
    pub version: u32,
    /// Arguments.
    pub args: Vec<PatternArg>,
}

/// Journey stub.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JourneySpec {
    /// Journey id.
    pub id: Name,
}

/// One reviewable authoring change.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorChangeSet {
    /// Change identity.
    pub id: ChangeId,
    /// Project hash this set was computed against.
    pub base_project_hash: Hash,
    /// Hash of the request that produced the set.
    pub request_hash: Hash,
    /// Declared write scope.
    pub scope: ChangeScope,
    /// Semantic operations.
    pub ops: Vec<AuthorOp>,
    /// Acceptance stub.
    pub acceptance: AcceptanceContract,
    /// Provenance edges.
    pub provenance: AuthoringProvenance,
}

/// Semantic authoring operation. Raw file patches are excluded.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum AuthorOp {
    /// Insert a module.
    AddModule {
        /// Module body.
        module: IntentModule,
    },
    /// Instantiate a pattern. Apply fails closed.
    Instantiate {
        /// Instance payload.
        instance: PatternInstance,
    },
    /// Set a pattern argument. Apply fails closed.
    SetArgument {
        /// Instance identity.
        instance: AnchorId,
        /// Argument name.
        key: Name,
        /// Argument value.
        value: PatternArg,
    },
    /// Allocate a locus.
    AddLocus {
        /// Owning module.
        module: AnchorId,
        /// Frozen identity.
        anchor: AnchorId,
        /// Current name.
        name: Name,
        /// Packed kind.
        kind: LocusKind,
    },
    /// Insert or replace a seed fact.
    AddFact {
        /// Module whose seed is written.
        module: AnchorId,
        /// Fact.
        fact: AnchoredSeedFact,
    },
    /// Append a Canon patch.
    AddCanonDiff {
        /// Owning module.
        module: AnchorId,
        /// Patch.
        diff: CanonDiff,
    },
    /// Record an asset candidate. Approval selects bindings later.
    BindAsset {
        /// Locus receiving the candidate.
        locus: AnchorId,
        /// Request identity.
        request: AssetRequestId,
    },
    /// Add a journey. Apply fails closed.
    AddJourney {
        /// Journey payload.
        journey: JourneySpec,
    },
    /// Record a reference edge.
    AddReference {
        /// Referenced object.
        target: AnchorId,
        /// Reference identity.
        reference: ReferenceId,
    },
    /// Tombstone an object.
    Remove {
        /// Removed object.
        target: AnchorId,
        /// Non-empty reason.
        reason: String,
    },
    /// Change [`Name`] only.
    Rename {
        /// Live object.
        target: AnchorId,
        /// Replacement name.
        to: Name,
    },
}

/// Kind discriminant for the conflict matrix.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[repr(u8)]
pub enum OpKind {
    /// [`AuthorOp::AddModule`].
    AddModule = 0,
    /// [`AuthorOp::Instantiate`].
    Instantiate = 1,
    /// [`AuthorOp::SetArgument`].
    SetArgument = 2,
    /// [`AuthorOp::AddLocus`].
    AddLocus = 3,
    /// [`AuthorOp::AddFact`].
    AddFact = 4,
    /// [`AuthorOp::AddCanonDiff`].
    AddCanonDiff = 5,
    /// [`AuthorOp::BindAsset`].
    BindAsset = 6,
    /// [`AuthorOp::AddJourney`].
    AddJourney = 7,
    /// [`AuthorOp::AddReference`].
    AddReference = 8,
    /// [`AuthorOp::Remove`].
    Remove = 9,
    /// [`AuthorOp::Rename`].
    Rename = 10,
}

impl OpKind {
    /// Every matrix row/column.
    pub const ALL: [Self; 11] = [
        Self::AddModule,
        Self::Instantiate,
        Self::SetArgument,
        Self::AddLocus,
        Self::AddFact,
        Self::AddCanonDiff,
        Self::BindAsset,
        Self::AddJourney,
        Self::AddReference,
        Self::Remove,
        Self::Rename,
    ];
}

impl AuthorOp {
    /// Matrix kind.
    #[must_use]
    pub fn kind(&self) -> OpKind {
        match self {
            Self::AddModule { .. } => OpKind::AddModule,
            Self::Instantiate { .. } => OpKind::Instantiate,
            Self::SetArgument { .. } => OpKind::SetArgument,
            Self::AddLocus { .. } => OpKind::AddLocus,
            Self::AddFact { .. } => OpKind::AddFact,
            Self::AddCanonDiff { .. } => OpKind::AddCanonDiff,
            Self::BindAsset { .. } => OpKind::BindAsset,
            Self::AddJourney { .. } => OpKind::AddJourney,
            Self::AddReference { .. } => OpKind::AddReference,
            Self::Remove { .. } => OpKind::Remove,
            Self::Rename { .. } => OpKind::Rename,
        }
    }

    /// Newly created identities, if any.
    #[must_use]
    pub fn created_anchor(&self) -> Option<AnchorId> {
        match self {
            Self::AddModule { module } => Some(module.anchor),
            Self::Instantiate { instance } => Some(instance.anchor),
            Self::AddLocus { anchor, .. } => Some(*anchor),
            _ => None,
        }
    }

    /// Primary write anchor for canonical apply order.
    #[must_use]
    pub fn primary_anchor(&self) -> AnchorId {
        match self {
            Self::AddModule { module } => module.anchor,
            Self::Instantiate { instance } => instance.anchor,
            Self::SetArgument { instance, .. } => *instance,
            Self::AddLocus { anchor, .. } => *anchor,
            Self::AddFact { fact, .. } => match fact {
                AnchoredSeedFact::Rel { a, .. } => *a,
                AnchoredSeedFact::Qty { of, .. } | AnchoredSeedFact::Pose { of, .. } => *of,
            },
            Self::AddCanonDiff { module, .. } => *module,
            Self::BindAsset { locus, .. } => *locus,
            Self::AddJourney { .. } => AnchorId::ZERO,
            Self::AddReference { target, .. } => *target,
            Self::Remove { target, .. } | Self::Rename { target, .. } => *target,
        }
    }
}

impl core::fmt::Display for OpKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let name = match self {
            Self::AddModule => "AddModule",
            Self::Instantiate => "Instantiate",
            Self::SetArgument => "SetArgument",
            Self::AddLocus => "AddLocus",
            Self::AddFact => "AddFact",
            Self::AddCanonDiff => "AddCanonDiff",
            Self::BindAsset => "BindAsset",
            Self::AddJourney => "AddJourney",
            Self::AddReference => "AddReference",
            Self::Remove => "Remove",
            Self::Rename => "Rename",
        };
        f.write_str(name)
    }
}
