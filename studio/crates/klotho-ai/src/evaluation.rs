//! Read-only broker for trusted KAI-06 evidence.

use std::collections::BTreeMap;

use klotho_core::Hash;
use klotho_eval::EvidenceBundle;
use klotho_ir::to_ron;
use klotho_prove::hash_bytes;

use crate::error::AiError;

/// Broker row.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct EvidenceRecord {
    /// Content hash of the sealed bundle.
    pub hash: Hash,
    /// Trusted sealed bundle.
    pub bundle: EvidenceBundle,
}

/// Evidence can only be registered after signature verification. Agent tools
/// receive cloned sealed bundles and have no builder or append capability.
#[derive(Default)]
pub struct EvaluationBroker {
    records: BTreeMap<Hash, EvidenceBundle>,
}

impl EvaluationBroker {
    /// Register evidence produced by a trusted evaluation host.
    pub fn register_trusted(&mut self, bundle: EvidenceBundle) -> Result<Hash, AiError> {
        bundle
            .verify_signature()
            .map_err(|error| AiError::Evidence(error.to_string()))?;
        let text = to_ron(&bundle).map_err(|error| AiError::Ser(error.to_string()))?;
        let hash = hash_bytes(text.as_bytes());
        self.records.insert(hash, bundle);
        Ok(hash)
    }

    /// Read a sealed bundle.
    pub fn get(&self, hash: Hash) -> Result<EvidenceRecord, AiError> {
        self.records
            .get(&hash)
            .cloned()
            .map(|bundle| EvidenceRecord { hash, bundle })
            .ok_or(AiError::UnknownEvidence(hash))
    }
}
