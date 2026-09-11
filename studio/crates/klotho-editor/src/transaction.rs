//! Headless authoring transaction handle around a loaded session document.

use std::path::Path;

use klotho_ai::{
    AuthorOp, ChangeScope, ReviewQueueEntry, SemanticDiff, TransactionStore, TxBudget, TxId,
};
use klotho_author::write_bundle;
use klotho_core::Hash;
use klotho_ir::{IntentDoc, Name, migrate_doc};

use crate::error::EditorError;
use crate::session::EditorSession;

/// Isolated transaction over a copy of the session document.
pub struct EditorTransaction {
    store: TransactionStore,
    id: TxId,
}

impl EditorSession {
    /// Open an isolated transaction. The live document is not written.
    pub fn open_transaction(&self, workspace: &Path) -> Result<EditorTransaction, EditorError> {
        EditorTransaction::open(workspace, self.doc().clone())
    }
}

impl EditorTransaction {
    /// Start a transaction from `doc` with an isolated workspace.
    pub fn open(workspace: &Path, doc: IntentDoc) -> Result<Self, EditorError> {
        let bundle = migrate_doc(Name::from("session"), Name::from("main"), doc)?;
        let live = workspace.join("live");
        write_bundle(&bundle, &live)?;
        let mut store = TransactionStore::open(workspace, &live.join("project.ron"))?;
        let id =
            store.create_from_bundle(bundle, ChangeScope::unrestricted(), TxBudget::default())?;
        Ok(Self { store, id })
    }

    /// Apply semantic ops to the isolated snapshot.
    pub fn apply(&mut self, ops: Vec<AuthorOp>) -> Result<Hash, EditorError> {
        Ok(self.store.apply(self.id, ops)?)
    }

    /// Semantic diff against the transaction base.
    pub fn diff(&self) -> Result<SemanticDiff, EditorError> {
        Ok(self.store.diff(self.id)?)
    }

    /// Drop unsubmitted changes. The live session document is untouched.
    pub fn cancel(mut self) -> Result<(), EditorError> {
        self.store.cancel(self.id)?;
        Ok(())
    }

    /// Submit to review. Does not Pin and does not mutate the live document.
    pub fn submit(mut self) -> Result<ReviewQueueEntry, EditorError> {
        Ok(self.store.submit(self.id)?)
    }
}
