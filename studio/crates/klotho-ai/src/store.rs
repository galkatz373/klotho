//! Isolated, resumable authoring transactions.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use klotho_author::{Loaded, load_any};
use klotho_core::Hash;
use klotho_ir::{AnchorId, ProjectBundle, from_ron, to_ron};

use crate::audit::{AuditKind, AuditRecord, ReviewQueueEntry};
use crate::cells::declare;
use crate::diff::{ImpactEdge, SemanticDiff};
use crate::error::AiError;
use crate::exec::{PreparedOp, apply_ops, check_prepared};
use crate::ids::{ChangeId, LeaseId, TxId};
use crate::lease::{Lease, overlaps, subtree};
use crate::merge::merge_snapshots;
use crate::ops::{AuthorOp, ChangeScope, TxBudget};
use crate::workspace::{AuthoringSnapshot, ContentWorkspace};

const DEFAULT_LEASE_TTL: u64 = 32;

/// Live transaction.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoringTransaction {
    /// Transaction id.
    pub id: TxId,
    /// Change id.
    pub change: ChangeId,
    /// Base snapshot hash.
    pub base_hash: Hash,
    /// Current snapshot hash.
    pub current_hash: Hash,
    /// Scope.
    pub scope: ChangeScope,
    /// Budget.
    pub budget: TxBudget,
    /// Status.
    pub status: TxStatus,
    /// Applied ops with captured preconditions.
    pub ops: Vec<PreparedOp>,
    /// Held lease, if any.
    pub lease: Option<LeaseId>,
}

/// Transaction lifecycle.
#[derive(Copy, Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum TxStatus {
    /// Open for apply.
    Open,
    /// Submitted to review. Does not mutate the live project.
    Submitted,
    /// Cancelled; workspace dropped.
    Cancelled,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoreMeta {
    sequence: u64,
    base_hash: Hash,
    leases: Vec<Lease>,
    audit: Vec<AuditRecord>,
    review: Vec<ReviewQueueEntry>,
    txs: Vec<TxId>,
}

/// Transaction store rooted at an isolated workspace. The live project is read-only.
pub struct TransactionStore {
    workspace: ContentWorkspace,
    base_dir: PathBuf,
    sequence: u64,
    base_hash: Hash,
    txs: BTreeMap<TxId, AuthoringTransaction>,
    leases: BTreeMap<LeaseId, Lease>,
    audit: Vec<AuditRecord>,
    review: Vec<ReviewQueueEntry>,
}

impl TransactionStore {
    /// Open or create a store. Does not write `base`.
    pub fn open(workspace: &Path, base: &Path) -> Result<Self, AiError> {
        let workspace = ContentWorkspace::open(workspace)?;
        let meta_path = workspace.root().join("store.ron");
        if meta_path.exists() {
            return Self::resume(workspace, base, &meta_path);
        }
        let bundle = load_base(base)?;
        let snap = AuthoringSnapshot::from_bundle(bundle);
        let base_hash = workspace.put(&snap)?;
        let store = Self {
            workspace,
            base_dir: base.to_path_buf(),
            sequence: 0,
            base_hash,
            txs: BTreeMap::new(),
            leases: BTreeMap::new(),
            audit: Vec::new(),
            review: Vec::new(),
        };
        store.persist()?;
        Ok(store)
    }

    fn resume(workspace: ContentWorkspace, base: &Path, meta_path: &Path) -> Result<Self, AiError> {
        let text = fs::read_to_string(meta_path).map_err(|e| AiError::Io(e.to_string()))?;
        let meta: StoreMeta = from_ron(&text).map_err(|e| AiError::Ser(e.to_string()))?;
        let mut txs = BTreeMap::new();
        for id in &meta.txs {
            let path = workspace
                .root()
                .join("tx")
                .join(id.to_string())
                .join("tx.ron");
            if !path.exists() {
                continue;
            }
            let text = fs::read_to_string(&path).map_err(|e| AiError::Io(e.to_string()))?;
            let tx: AuthoringTransaction =
                from_ron(&text).map_err(|e| AiError::Ser(e.to_string()))?;
            txs.insert(*id, tx);
        }
        let mut leases = BTreeMap::new();
        for lease in meta.leases {
            leases.insert(lease.id, lease);
        }
        Ok(Self {
            workspace,
            base_dir: base.to_path_buf(),
            sequence: meta.sequence,
            base_hash: meta.base_hash,
            txs,
            leases,
            audit: meta.audit,
            review: meta.review,
        })
    }

    /// Create a transaction on the current live base.
    pub fn create(&mut self, scope: ChangeScope, budget: TxBudget) -> Result<TxId, AiError> {
        let bundle = load_base(&self.base_dir)?;
        self.create_from_bundle(bundle, scope, budget)
    }

    /// Create a transaction on an in-memory bundle (tests / editor).
    pub fn create_from_bundle(
        &mut self,
        bundle: ProjectBundle,
        scope: ChangeScope,
        budget: TxBudget,
    ) -> Result<TxId, AiError> {
        let snap = AuthoringSnapshot::from_bundle(bundle);
        let base_hash = self.workspace.put(&snap)?;
        self.sequence += 1;
        let mut key = Vec::from(base_hash.0);
        key.extend_from_slice(&self.sequence.to_le_bytes());
        let id = TxId::derive(&key);
        let change = ChangeId::derive(id.as_bytes());
        let tx = AuthoringTransaction {
            id,
            change,
            base_hash,
            current_hash: base_hash,
            scope,
            budget,
            status: TxStatus::Open,
            ops: Vec::new(),
            lease: None,
        };
        self.txs.insert(id, tx);
        self.audit(
            AuditKind::Create,
            id,
            change,
            base_hash,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
            None,
        );
        self.persist_tx(id)?;
        self.persist()?;
        Ok(id)
    }

    /// Apply `ops` to the isolated snapshot.
    pub fn apply(&mut self, id: TxId, ops: Vec<AuthorOp>) -> Result<Hash, AiError> {
        let tx = self.txs.get(&id).cloned().ok_or(AiError::UnknownTx(id))?;
        self.require_open(&tx)?;
        if tx.ops.len() as u32 + ops.len() as u32 > tx.budget.max_ops {
            return Err(AiError::Budget);
        }
        let mut snap = self.workspace.get(tx.current_hash)?;
        for op in &ops {
            self.check_scope(&tx.scope, &snap, op)?;
        }
        let prepared = apply_ops(&mut snap, tx.change, &ops)?;
        let hash = self.workspace.put(&snap)?;
        let mut reads = Vec::new();
        let mut writes = Vec::new();
        let mut hashes = Vec::new();
        let tx = self.txs.get_mut(&id).ok_or(AiError::UnknownTx(id))?;
        for item in prepared {
            reads.extend(item.reads.iter().copied());
            writes.extend(item.writes.iter().copied());
            hashes.push(item.hash);
            tx.ops.push(item);
        }
        tx.current_hash = hash;
        let change = tx.change;
        let base_hash = tx.base_hash;
        self.audit(
            AuditKind::Apply,
            id,
            change,
            base_hash,
            hashes,
            reads,
            writes,
            None,
            None,
        );
        self.persist_tx(id)?;
        self.persist()?;
        Ok(hash)
    }

    /// Semantic diff plus impact graph.
    pub fn diff(&self, id: TxId) -> Result<SemanticDiff, AiError> {
        let tx = self.txs.get(&id).ok_or(AiError::UnknownTx(id))?;
        let snap = self.workspace.get(tx.current_hash)?;
        let mut impact = Vec::new();
        let mut reads = Vec::new();
        let mut writes = Vec::new();
        for item in &tx.ops {
            reads.extend(item.reads.iter().copied());
            writes.extend(item.writes.iter().copied());
            for cell in &item.writes {
                for dep in snap.dependents(cell.anchor) {
                    impact.push(ImpactEdge {
                        target: cell.anchor,
                        dependent: dep,
                    });
                }
            }
        }
        reads.sort();
        reads.dedup();
        writes.sort();
        writes.dedup();
        Ok(SemanticDiff {
            change: tx.change,
            base_hash: tx.base_hash,
            current_hash: tx.current_hash,
            ops: tx.ops.iter().map(|o| o.op.clone()).collect(),
            writes,
            reads,
            impact,
        })
    }

    /// Re-evaluate preconditions against `new_base`. Never replays text patches.
    pub fn rebase(&mut self, id: TxId, new_base: &ProjectBundle) -> Result<Hash, AiError> {
        let tx = self.txs.get(&id).cloned().ok_or(AiError::UnknownTx(id))?;
        self.require_open(&tx)?;
        let original = self.workspace.get(tx.base_hash)?;
        let proposed = self.workspace.get(tx.current_hash)?;
        let current = AuthoringSnapshot::from_bundle(new_base.clone());
        check_prepared(&current, &tx.ops)?;
        let current_id = ChangeId::derive(b"rebase-current");
        let merged = merge_snapshots(&original, &current, &proposed, current_id, tx.change)?;
        let hash = self.workspace.put(&merged)?;
        let new_base_hash = self.workspace.put(&current)?;
        let tx = self.txs.get_mut(&id).ok_or(AiError::UnknownTx(id))?;
        tx.base_hash = new_base_hash;
        tx.current_hash = hash;
        let change = tx.change;
        self.audit(
            AuditKind::Rebase,
            id,
            change,
            new_base_hash,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
            Some(new_base_hash),
        );
        self.persist_tx(id)?;
        self.persist()?;
        Ok(hash)
    }

    /// Drop only the transaction workspace. Base is untouched.
    pub fn cancel(&mut self, id: TxId) -> Result<(), AiError> {
        let tx = self.txs.get(&id).cloned().ok_or(AiError::UnknownTx(id))?;
        if tx.status == TxStatus::Submitted {
            return Err(AiError::Submitted);
        }
        self.leases.retain(|_, lease| lease.tx != id);
        self.txs.remove(&id);
        self.workspace.delete_tx(&id.to_string())?;
        self.audit(
            AuditKind::Cancel,
            id,
            tx.change,
            tx.base_hash,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            tx.lease,
            None,
        );
        self.persist()?;
        Ok(())
    }

    /// Submit to the review queue. Does not write the live project.
    pub fn submit(&mut self, id: TxId) -> Result<ReviewQueueEntry, AiError> {
        let tx = self.txs.get(&id).cloned().ok_or(AiError::UnknownTx(id))?;
        self.require_open(&tx)?;
        self.sequence += 1;
        let entry = ReviewQueueEntry {
            tx: id,
            change: tx.change,
            base_hash: tx.base_hash,
            proposed_hash: tx.current_hash,
            seq: self.sequence,
        };
        if let Some(live) = self.txs.get_mut(&id) {
            live.status = TxStatus::Submitted;
        }
        self.review.push(entry.clone());
        let hashes: Vec<Hash> = tx.ops.iter().map(|o| o.hash).collect();
        self.audit(
            AuditKind::Submit,
            id,
            tx.change,
            tx.base_hash,
            hashes,
            Vec::new(),
            Vec::new(),
            None,
            None,
        );
        self.persist_tx(id)?;
        self.persist()?;
        Ok(entry)
    }

    /// Acquire a renewable lease on the subtree rooted at `root`.
    pub fn acquire_lease(&mut self, id: TxId, root: AnchorId) -> Result<LeaseId, AiError> {
        let tx = self.txs.get(&id).cloned().ok_or(AiError::UnknownTx(id))?;
        self.require_open(&tx)?;
        let snap = self.workspace.get(tx.current_hash)?;
        let want = subtree(&snap, root);
        self.sequence += 1;
        let now = self.sequence;
        for lease in self.leases.values() {
            if lease.expired(now) || lease.tx == id {
                continue;
            }
            let holder = self
                .txs
                .get(&lease.tx)
                .and_then(|t| self.workspace.get(t.current_hash).ok());
            let Some(holder) = holder else {
                continue;
            };
            let held = subtree(&holder, lease.root);
            if overlaps(&want, &held) {
                return Err(AiError::LeaseHeld {
                    anchor: root,
                    by: lease.tx,
                });
            }
        }
        let mut key = Vec::from(id.0);
        key.extend_from_slice(&root.0);
        key.extend_from_slice(&now.to_le_bytes());
        let lease_id = LeaseId::derive(&key);
        let lease = Lease {
            id: lease_id,
            tx: id,
            root,
            expires_at: now + DEFAULT_LEASE_TTL,
        };
        if let Some(prev) = tx.lease {
            self.leases.remove(&prev);
        }
        self.leases.insert(lease_id, lease);
        if let Some(live) = self.txs.get_mut(&id) {
            live.lease = Some(lease_id);
        }
        self.audit(
            AuditKind::LeaseAcquire,
            id,
            tx.change,
            tx.base_hash,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Some(lease_id),
            None,
        );
        self.persist_tx(id)?;
        self.persist()?;
        Ok(lease_id)
    }

    /// Renew a live lease. Expired leases cannot be renewed into an overwrite.
    pub fn renew_lease(&mut self, id: TxId, lease: LeaseId) -> Result<(), AiError> {
        let tx = self.txs.get(&id).cloned().ok_or(AiError::UnknownTx(id))?;
        self.require_open(&tx)?;
        self.sequence += 1;
        let now = self.sequence;
        let held = self
            .leases
            .get(&lease)
            .cloned()
            .ok_or(AiError::UnknownLease(lease))?;
        if held.tx != id {
            return Err(AiError::LeaseHeld {
                anchor: held.root,
                by: held.tx,
            });
        }
        if held.expired(now) {
            self.audit(
                AuditKind::LeaseExpire,
                id,
                tx.change,
                tx.base_hash,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Some(lease),
                None,
            );
            self.persist()?;
            return Err(AiError::LeaseExpired(lease));
        }
        if let Some(row) = self.leases.get_mut(&lease) {
            row.expires_at = now + DEFAULT_LEASE_TTL;
        }
        self.audit(
            AuditKind::LeaseRenew,
            id,
            tx.change,
            tx.base_hash,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Some(lease),
            None,
        );
        self.persist()?;
        Ok(())
    }

    /// Advance the sequence clock (tests / expiry).
    pub fn advance_clock(&mut self, ticks: u64) {
        self.sequence = self.sequence.saturating_add(ticks);
        let now = self.sequence;
        let expired: Vec<LeaseId> = self
            .leases
            .iter()
            .filter(|(_, l)| l.expired(now))
            .map(|(id, _)| *id)
            .collect();
        for id in expired {
            if let Some(lease) = self.leases.get(&id).cloned() {
                self.audit.push(AuditRecord {
                    seq: now,
                    tx: lease.tx,
                    change: self
                        .txs
                        .get(&lease.tx)
                        .map(|t| t.change)
                        .unwrap_or_else(|| ChangeId::derive(&lease.tx.0)),
                    kind: AuditKind::LeaseExpire,
                    base_hash: self.base_hash,
                    op_hashes: Vec::new(),
                    reads: Vec::new(),
                    writes: Vec::new(),
                    witnesses: Vec::new(),
                    lease: Some(id),
                    rebase_base: None,
                });
            }
        }
        let _ = self.persist();
    }

    /// Borrow a transaction.
    pub fn transaction(&self, id: TxId) -> Result<&AuthoringTransaction, AiError> {
        self.txs.get(&id).ok_or(AiError::UnknownTx(id))
    }

    /// Load the current snapshot.
    pub fn snapshot(&self, id: TxId) -> Result<AuthoringSnapshot, AiError> {
        let tx = self.txs.get(&id).ok_or(AiError::UnknownTx(id))?;
        self.workspace.get(tx.current_hash)
    }

    /// Load the immutable base snapshot of one transaction.
    pub fn transaction_base_snapshot(&self, id: TxId) -> Result<AuthoringSnapshot, AiError> {
        let tx = self.txs.get(&id).ok_or(AiError::UnknownTx(id))?;
        self.workspace.get(tx.base_hash)
    }

    /// Load the immutable live-base snapshot used by new transactions.
    pub fn base_snapshot(&self) -> Result<AuthoringSnapshot, AiError> {
        self.workspace.get(self.base_hash)
    }

    /// Content hash of the immutable live base.
    #[must_use]
    pub const fn base_hash(&self) -> Hash {
        self.base_hash
    }

    /// Review queue.
    #[must_use]
    pub fn review_queue(&self) -> &[ReviewQueueEntry] {
        &self.review
    }

    /// Audit log.
    #[must_use]
    pub fn audit_log(&self) -> &[AuditRecord] {
        &self.audit
    }

    /// True when `lease` is expired at the current sequence.
    #[must_use]
    pub fn lease_expired(&self, lease: LeaseId) -> bool {
        self.leases
            .get(&lease)
            .is_none_or(|l| l.expired(self.sequence))
    }

    fn require_open(&self, tx: &AuthoringTransaction) -> Result<(), AiError> {
        match tx.status {
            TxStatus::Open => Ok(()),
            TxStatus::Submitted => Err(AiError::Submitted),
            TxStatus::Cancelled => Err(AiError::Cancelled),
        }
    }

    fn check_scope(
        &self,
        scope: &ChangeScope,
        snap: &AuthoringSnapshot,
        op: &AuthorOp,
    ) -> Result<(), AiError> {
        if scope.modules.is_empty() && scope.anchors.is_empty() {
            return Ok(());
        }
        let decl = declare(op, Some(snap));
        let parent = op_parent_module(op, snap);
        let created = op.created_anchor();
        for cell in decl.writes {
            if scope.allows(cell.anchor) {
                continue;
            }
            if created == Some(cell.anchor) && parent.is_some_and(|m| scope.allows(m)) {
                continue;
            }
            if parent.is_some_and(|m| scope.allows(m)) {
                continue;
            }
            if snap
                .owning_module(cell.anchor)
                .is_some_and(|m| scope.allows(m))
            {
                continue;
            }
            return Err(AiError::Scope(cell.anchor));
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn audit(
        &mut self,
        kind: AuditKind,
        tx: TxId,
        change: ChangeId,
        base_hash: Hash,
        op_hashes: Vec<Hash>,
        reads: Vec<crate::ids::Cell>,
        writes: Vec<crate::ids::Cell>,
        lease: Option<LeaseId>,
        rebase_base: Option<Hash>,
    ) {
        self.sequence += 1;
        self.audit.push(AuditRecord {
            seq: self.sequence,
            tx,
            change,
            kind,
            base_hash,
            op_hashes,
            reads,
            writes,
            witnesses: Vec::new(),
            lease,
            rebase_base,
        });
    }

    fn persist_tx(&self, id: TxId) -> Result<(), AiError> {
        let tx = self.txs.get(&id).ok_or(AiError::UnknownTx(id))?;
        let dir = self.workspace.root().join("tx").join(id.to_string());
        fs::create_dir_all(&dir).map_err(|e| AiError::Io(e.to_string()))?;
        let text = to_ron(tx).map_err(|e| AiError::Ser(e.to_string()))?;
        write_atomic(&dir.join("tx.ron"), &text)
    }

    fn persist(&self) -> Result<(), AiError> {
        let meta = StoreMeta {
            sequence: self.sequence,
            base_hash: self.base_hash,
            leases: self.leases.values().cloned().collect(),
            audit: self.audit.clone(),
            review: self.review.clone(),
            txs: self.txs.keys().copied().collect(),
        };
        let text = to_ron(&meta).map_err(|e| AiError::Ser(e.to_string()))?;
        write_atomic(&self.workspace.root().join("store.ron"), &text)
    }
}

fn write_atomic(path: &Path, contents: &str) -> Result<(), AiError> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, contents).map_err(|e| AiError::Io(e.to_string()))?;
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(AiError::Io(e.to_string()))
        }
    }
}

fn op_parent_module(op: &AuthorOp, snap: &AuthoringSnapshot) -> Option<AnchorId> {
    match op {
        AuthorOp::AddLocus { module, .. }
        | AuthorOp::AddFact { module, .. }
        | AuthorOp::AddCanonDiff { module, .. } => Some(*module),
        AuthorOp::AddModule { module } => Some(module.anchor),
        AuthorOp::BindAsset { locus, .. }
        | AuthorOp::AddReference { target: locus, .. }
        | AuthorOp::Remove { target: locus, .. }
        | AuthorOp::Rename { target: locus, .. }
        | AuthorOp::SetArgument {
            instance: locus, ..
        } => snap.owning_module(*locus).or(Some(*locus)),
        AuthorOp::Instantiate { instance } => Some(instance.module),
        AuthorOp::AddJourney { .. } => None,
    }
}

fn load_base(path: &Path) -> Result<ProjectBundle, AiError> {
    match load_any(path).or_else(|_| load_any(&path.join("project.ron")))? {
        Loaded::Project(bundle) => Ok(bundle),
        Loaded::Doc(doc) => Ok(klotho_ir::migrate_doc(
            klotho_ir::Name::from("session"),
            klotho_ir::Name::from("main"),
            doc,
        )?),
    }
}

impl Drop for TransactionStore {
    fn drop(&mut self) {
        // Isolation: dropping never writes the live project tree.
        let _ = self.persist();
    }
}
