//! Transaction, merge, and lease failures.

use core::fmt;

use klotho_author::AuthorError;
use klotho_core::Hash;
use klotho_ir::AnchorId;

use crate::agent::RequestId;
use crate::conflict::ConflictWitness;
use crate::ids::{LeaseId, TxId};
use crate::ops::OpKind;
use crate::policy::Capability;

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
    /// Request id is not known.
    UnknownRequest(RequestId),
    /// Another active request owns this semantic anchor.
    Ownership {
        /// Contested anchor.
        anchor: AnchorId,
        /// Owning request.
        by: RequestId,
    },
    /// A request omitted a meaningful acceptance contract.
    MissingAcceptance,
    /// Worker profile does not grant the tool capability.
    CapabilityDenied(Capability),
    /// No policy-compatible backend supports the requested modality.
    NoBackend,
    /// Model protocol payload exceeded a hard cap.
    BackendPayload,
    /// Model protocol framing/schema failed.
    BackendProtocol(String),
    /// Isolated process/service transport failed.
    BackendTransport(String),
    /// End-to-end request timed out.
    Timeout,
    /// Request budget was exceeded.
    RequestBudget,
    /// Embedding cache key or source hash mismatch.
    EmbeddingKeyMismatch,
    /// Memory lacked approval or exact source hashes.
    UnapprovedMemory,
    /// Evidence is not registered with the trusted broker.
    UnknownEvidence(Hash),
    /// Evidence seal was invalid.
    Evidence(String),
    /// Request state does not permit the operation.
    RequestState(String),
    /// Request base differs from the indexed live project.
    BaseHashMismatch,
    /// Transaction scope exceeds the acceptance contract.
    ContractScopeMismatch,
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
            Self::UnknownRequest(id) => write!(f, "unknown request {id}"),
            Self::Ownership { anchor, by } => write!(f, "anchor {anchor} owned by request {by}"),
            Self::MissingAcceptance => write!(f, "request requires text and an acceptance claim"),
            Self::CapabilityDenied(capability) => write!(f, "capability denied: {capability:?}"),
            Self::NoBackend => write!(f, "no policy-compatible model backend"),
            Self::BackendPayload => write!(f, "model protocol payload exceeds cap"),
            Self::BackendProtocol(error) => write!(f, "model protocol: {error}"),
            Self::BackendTransport(error) => write!(f, "model transport: {error}"),
            Self::Timeout => write!(f, "request timed out"),
            Self::RequestBudget => write!(f, "request budget exhausted"),
            Self::EmbeddingKeyMismatch => write!(f, "embedding key or source hash mismatch"),
            Self::UnapprovedMemory => write!(f, "project memory is not approved and source-linked"),
            Self::UnknownEvidence(hash) => write!(f, "unknown evidence {hash}"),
            Self::Evidence(error) => write!(f, "evidence: {error}"),
            Self::RequestState(state) => {
                write!(f, "request state does not permit operation: {state}")
            }
            Self::BaseHashMismatch => write!(f, "request base project hash mismatch"),
            Self::ContractScopeMismatch => {
                write!(f, "transaction scope exceeds acceptance contract")
            }
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
