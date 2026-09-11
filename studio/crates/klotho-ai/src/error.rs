//! Transaction, merge, and lease failures.

use core::fmt;

use klotho_author::AuthorError;
use klotho_ir::AnchorId;

use crate::conflict::ConflictWitness;
use crate::ids::{LeaseId, TxId};
use crate::ops::OpKind;

/// Authoring-AI failure. Never a [`klotho_core::KernelFault`].
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum AiError {
    /// Semantic apply / flatten failure.
    Author(AuthorError),
    /// Workspace IO.
    Io(String),
    /// Snapshot encode/decode.
    Ser(String),
    /// Unknown transaction.
    UnknownTx(TxId),
    /// Conflict-matrix rejection with a witness.
    Conflict(ConflictWitness),
    /// A recorded value precondition did not hold.
    Precondition(String),
    /// Operation is stubbed and fails closed.
    FailClosed(OpKind),
    /// Another transaction holds a live overlapping lease.
    LeaseHeld {
        /// Contested anchor.
        anchor: AnchorId,
        /// Holder.
        by: TxId,
    },
    /// Lease id is not in the store.
    UnknownLease(LeaseId),
    /// Lease clock has passed expiry. Expiry does not authorize overwrite.
    LeaseExpired(LeaseId),
    /// Write outside the transaction scope.
    Scope(AnchorId),
    /// Operation budget exhausted.
    Budget,
    /// Transaction was cancelled.
    Cancelled,
    /// Transaction was already submitted.
    Submitted,
    /// Duplicate identity at apply time.
    DuplicateAnchor(String),
}

impl fmt::Display for AiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Author(e) => write!(f, "{e}"),
            Self::Io(s) => write!(f, "{s}"),
            Self::Ser(s) => write!(f, "{s}"),
            Self::UnknownTx(id) => write!(f, "unknown transaction {id}"),
            Self::Conflict(w) => write!(f, "conflict {w}"),
            Self::Precondition(s) => write!(f, "precondition {s}"),
            Self::FailClosed(k) => write!(f, "fail-closed {k}"),
            Self::LeaseHeld { anchor, by } => write!(f, "lease held on {anchor} by {by}"),
            Self::UnknownLease(id) => write!(f, "unknown lease {id}"),
            Self::LeaseExpired(id) => write!(f, "lease expired {id}"),
            Self::Scope(id) => write!(f, "out of scope {id}"),
            Self::Budget => write!(f, "transaction budget exhausted"),
            Self::Cancelled => write!(f, "transaction cancelled"),
            Self::Submitted => write!(f, "transaction already submitted"),
            Self::DuplicateAnchor(id) => write!(f, "duplicate anchor {id}"),
        }
    }
}

impl core::error::Error for AiError {}

impl From<AuthorError> for AiError {
    fn from(e: AuthorError) -> Self {
        Self::Author(e)
    }
}

impl From<klotho_ir::IrError> for AiError {
    fn from(e: klotho_ir::IrError) -> Self {
        Self::Author(AuthorError::from(e))
    }
}
