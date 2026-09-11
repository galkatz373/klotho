//! Monotonic audit log. Sequence numbers, not wall-clock, are authoritative.

use serde::{Deserialize, Serialize};

use klotho_core::Hash;

use crate::conflict::ConflictWitness;
use crate::ids::{Cell, ChangeId, LeaseId, TxId};

/// One audit row.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditRecord {
    /// Monotonic store sequence.
    pub seq: u64,
    /// Transaction.
    pub tx: TxId,
    /// Change identity.
    pub change: ChangeId,
    /// Event kind.
    pub kind: AuditKind,
    /// Base hash at the event.
    pub base_hash: Hash,
    /// Hashes of ops involved.
    pub op_hashes: Vec<Hash>,
    /// Read cells.
    pub reads: Vec<Cell>,
    /// Write cells.
    pub writes: Vec<Cell>,
    /// Conflict witnesses, if any.
    pub witnesses: Vec<ConflictWitness>,
    /// Lease event payload.
    pub lease: Option<LeaseId>,
    /// New base after rebase.
    pub rebase_base: Option<Hash>,
}

/// Audit event kind.
#[derive(Copy, Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum AuditKind {
    /// Transaction created.
    Create,
    /// Ops applied.
    Apply,
    /// Rebase onto a new base.
    Rebase,
    /// Lease acquired.
    LeaseAcquire,
    /// Lease renewed.
    LeaseRenew,
    /// Lease expired (clock advanced past it).
    LeaseExpire,
    /// Transaction cancelled.
    Cancel,
    /// Submitted to review.
    Submit,
}

/// Review-queue entry. Submit does not Pin or write the live project.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewQueueEntry {
    /// Transaction.
    pub tx: TxId,
    /// Change.
    pub change: ChangeId,
    /// Base hash.
    pub base_hash: Hash,
    /// Proposed snapshot hash.
    pub proposed_hash: Hash,
    /// Sequence at submit.
    pub seq: u64,
}
