//! Cook-time failures. None of these are `KernelFault` or `RejectReason`.

use core::fmt;

use klotho_ir::{
    Diagnostic, FailureClass, diagnose_cap, diagnose_cfg, diagnose_contradiction, diagnose_named,
};

/// Why a Canon document failed to cook.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum CookError {
    /// Graph mixes unlabeled ops and explicit pcs.
    MixedLabeling,
    /// Two nodes share a pc.
    DuplicatePc(u16),
    /// `entry` is not a defined pc.
    MissingEntry(u16),
    /// Jump / fail target is not a defined pc.
    MissingTarget {
        /// Source pc.
        from: u16,
        /// Missing destination.
        to: u16,
    },
    /// Node is not reachable from `entry`.
    Unreachable(u16),
    /// A non-halt op would fall off the end of the graph.
    FallOff(u16),
    /// Successor graph contains a cycle.
    Cycle,
    /// `Name(id)` is not a seed locus / declared id.
    UnboundName(String),
    /// IntentDoc failed structural validation.
    InvalidDoc(String),
    /// Duplicate Law / Affordance / Rite / Beat / seed locus id.
    DuplicateId(String),
    /// A retraction named a declaration that is not in the draft.
    UnknownRetract(String),
    /// Compiled pred exceeds [`crate::PRED_OPS_PER_EVAL`].
    PredTooLarge,
    /// Packed table id space exhausted.
    TableFull,
    /// Tiny-fragment Law `must` is unsatisfiable, or two Laws contradict.
    Contradiction(String),
    /// `Lockable` is declared without a key-or-rite admission pred.
    LockableNeedsKeyOrRite,
}

impl CookError {
    /// Shared diagnostic envelope. [`Display`] of `self` is the message.
    #[must_use]
    pub fn to_diagnostic(&self) -> Diagnostic {
        let message = self.to_string();
        match self {
            Self::Contradiction(s) => {
                let laws: Vec<&str> = s.split('|').collect();
                diagnose_contradiction(&laws, message)
            }
            Self::LockableNeedsKeyOrRite => diagnose_contradiction(&["Lockable"], message),
            Self::MixedLabeling => diagnose_cfg("rite", 0, 0, message),
            Self::DuplicatePc(pc) => diagnose_cfg("rite", *pc, *pc, message),
            Self::MissingEntry(pc) => diagnose_cfg("rite", 0, *pc, message),
            Self::MissingTarget { from, to } => diagnose_cfg("rite", *from, *to, message),
            Self::Unreachable(pc) => diagnose_cfg("rite", 0, *pc, message),
            Self::FallOff(pc) => diagnose_cfg("rite", *pc, *pc, message),
            Self::Cycle => diagnose_cfg("rite", 0, 0, message),
            Self::PredTooLarge => diagnose_cap("pred", 65, 64, message),
            Self::TableFull => diagnose_cap("table", 65536, 65535, message),
            Self::UnboundName(n) => diagnose_named(
                "CANON.UnboundName",
                FailureClass::Schema,
                "name",
                n,
                message,
            ),
            Self::InvalidDoc(n) => {
                diagnose_named("CANON.InvalidDoc", FailureClass::Schema, "doc", n, message)
            }
            Self::DuplicateId(n) => {
                diagnose_named("CANON.DuplicateId", FailureClass::Schema, "id", n, message)
            }
            Self::UnknownRetract(n) => diagnose_named(
                "CANON.UnknownRetract",
                FailureClass::Schema,
                "id",
                n,
                message,
            ),
        }
    }
}

impl fmt::Display for CookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MixedLabeling => write!(f, "MixedLabeling"),
            Self::DuplicatePc(pc) => write!(f, "DuplicatePc({pc})"),
            Self::MissingEntry(pc) => write!(f, "MissingEntry({pc})"),
            Self::MissingTarget { from, to } => write!(f, "MissingTarget({from}->{to})"),
            Self::Unreachable(pc) => write!(f, "Unreachable({pc})"),
            Self::FallOff(pc) => write!(f, "FallOff({pc})"),
            Self::Cycle => write!(f, "Cycle"),
            Self::UnboundName(n) => write!(f, "UnboundName({n})"),
            Self::InvalidDoc(s) => write!(f, "InvalidDoc({s})"),
            Self::DuplicateId(s) => write!(f, "DuplicateId({s})"),
            Self::UnknownRetract(s) => write!(f, "UnknownRetract({s})"),
            Self::PredTooLarge => write!(f, "PredTooLarge"),
            Self::TableFull => write!(f, "TableFull"),
            Self::Contradiction(s) => write!(f, "Contradiction({s})"),
            Self::LockableNeedsKeyOrRite => write!(f, "LockableNeedsKeyOrRite"),
        }
    }
}

impl core::error::Error for CookError {}

#[cfg(test)]
mod tests {
    use super::*;
    use klotho_ir::DiagnosticCode;

    #[test]
    fn contradiction_envelope_preserves_display() {
        let err = CookError::Contradiction("alive|dead".into());
        let d = err.to_diagnostic();
        assert_eq!(d.to_string(), err.to_string());
        assert_eq!(d.code.0, DiagnosticCode::CONTRADICTION);
        assert!(d.points_to_anchor());
        assert_eq!(d.related.len(), 1);
    }

    #[test]
    fn missing_target_is_cfg() {
        let err = CookError::MissingTarget { from: 0, to: 99 };
        let d = err.to_diagnostic();
        assert_eq!(d.to_string(), "MissingTarget(0->99)");
        assert_eq!(d.code.0, DiagnosticCode::CFG_TARGET);
        match d.witness {
            Some(klotho_ir::Counterexample::Cfg {
                last_reachable_pc,
                blocked_pc,
            }) => {
                assert_eq!(last_reachable_pc, 0);
                assert_eq!(blocked_pc, 99);
            }
            other => panic!("{other:?}"),
        }
    }
}
