//! Pattern expansion failures. Never a kernel fault.

use core::fmt;

use klotho_ir::{AnchorId, Diagnostic, FailureClass, IrError, diagnose_named};

/// Why expansion or migration failed.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum PatternError {
    /// Pattern id is not in the standard library.
    Unknown(String),
    /// Requested version is not published or not migratable.
    Version {
        /// Pattern id.
        id: String,
        /// Requested version.
        requested: u32,
    },
    /// Argument missing or wrong type.
    Arg {
        /// Pattern id.
        id: String,
        /// Parameter name.
        key: String,
        /// Why it failed.
        reason: String,
    },
    /// Required capability is absent on a host locus.
    Capability {
        /// Parameter / locus.
        locus: String,
        /// Required affordance id.
        cap: String,
    },
    /// Conflicting capability is already granted.
    Conflict {
        /// Parameter / locus.
        locus: String,
        /// Conflicting affordance id.
        cap: String,
    },
    /// Declared budget exceeded by the expansion.
    Budget {
        /// Pattern id.
        id: String,
        /// Which counter missed.
        counter: String,
        /// Observed.
        used: u32,
        /// Declared cap.
        cap: u32,
    },
    /// Place multidomain budget cannot be met without deleting a protected or
    /// critical-path anchor (K75).
    WorldBudget {
        /// Place that missed.
        place: String,
        /// Density, visibility, streaming, nav, phys, audio, or gpu.
        domain: String,
        /// Observed cost.
        used: u64,
        /// Declared cap.
        cap: u64,
    },
    /// World plan graph is incomplete, cyclic where forbidden, or names a
    /// missing Place.
    WorldGraph {
        /// Why the plan failed.
        reason: String,
    },
    /// Regeneration or a budget solve would drop a protected semantic anchor.
    ProtectedAnchor {
        /// Anchor token.
        token: String,
    },
    /// Underlying IR validation.
    Ir(IrError),
}

impl PatternError {
    /// Shared diagnostic envelope.
    #[must_use]
    pub fn to_diagnostic(&self, primary: AnchorId) -> Diagnostic {
        let message = self.to_string();
        let mut d = match self {
            Self::Unknown(id) => diagnose_named(
                "PATTERN.Unknown",
                FailureClass::Schema,
                "pattern",
                id,
                message,
            ),
            Self::Version { id, .. } => diagnose_named(
                "PATTERN.Version",
                FailureClass::Schema,
                "pattern",
                id,
                message,
            ),
            Self::Arg { key, .. } => {
                diagnose_named("PATTERN.Arg", FailureClass::Schema, "arg", key, message)
            }
            Self::Capability { locus, .. } => diagnose_named(
                "PATTERN.Capability",
                FailureClass::Schema,
                "locus",
                locus,
                message,
            ),
            Self::Conflict { locus, .. } => diagnose_named(
                "PATTERN.Conflict",
                FailureClass::Schema,
                "locus",
                locus,
                message,
            ),
            Self::Budget { id, .. } => diagnose_named(
                "PATTERN.Budget",
                FailureClass::Budget,
                "pattern",
                id,
                message,
            ),
            Self::WorldBudget { place, .. } => diagnose_named(
                "PATTERN.WorldBudget",
                FailureClass::Budget,
                "place",
                place,
                message,
            ),
            Self::WorldGraph { reason } => diagnose_named(
                "PATTERN.WorldGraph",
                FailureClass::Schema,
                "world",
                reason,
                message,
            ),
            Self::ProtectedAnchor { token } => diagnose_named(
                "PATTERN.ProtectedAnchor",
                FailureClass::Schema,
                "anchor",
                token,
                message,
            ),
            Self::Ir(e) => e.to_diagnostic(),
        };
        if primary != AnchorId::ZERO {
            d.primary = primary;
        }
        d
    }
}

impl fmt::Display for PatternError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(id) => write!(f, "UnknownPattern({id})"),
            Self::Version { id, requested } => {
                write!(f, "PatternVersion({id}@{requested})")
            }
            Self::Arg { id, key, reason } => write!(f, "PatternArg({id}.{key}: {reason})"),
            Self::Capability { locus, cap } => write!(f, "PatternCapability({locus} needs {cap})"),
            Self::Conflict { locus, cap } => write!(f, "PatternConflict({locus} has {cap})"),
            Self::Budget {
                id,
                counter,
                used,
                cap,
            } => write!(f, "PatternBudget({id} {counter} {used}/{cap})"),
            Self::WorldBudget {
                place,
                domain,
                used,
                cap,
            } => write!(f, "WorldBudget({place} {domain} {used}/{cap})"),
            Self::WorldGraph { reason } => write!(f, "WorldGraph({reason})"),
            Self::ProtectedAnchor { token } => write!(f, "ProtectedAnchor({token})"),
            Self::Ir(e) => write!(f, "{e}"),
        }
    }
}

impl core::error::Error for PatternError {}

impl From<IrError> for PatternError {
    fn from(e: IrError) -> Self {
        Self::Ir(e)
    }
}
