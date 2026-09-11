//! Trusted evidence bundles (K67). Agents may read results; they cannot seal them.

use serde::{Deserialize, Serialize};

use klotho_core::Hash;
use klotho_ir::{Name, to_ron};
use klotho_prove::{
    Activity, ArtifactKind, LicenseSpan, ProvenanceDag, ProvenanceKind, evidence_signature,
};

use crate::error::EvalError;

/// Evaluation layer that produced a check.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckLayer {
    /// Schema / module / provenance.
    Schema,
    /// Canon / CFG / agency.
    Canon,
    /// Headless journey.
    Journey,
    /// Save/load.
    Save,
    /// Capture / presentation.
    Capture,
    /// Budget.
    Budget,
    /// Package allowlist.
    Package,
    /// Named human approval.
    Approval,
}

/// One trusted check record.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckEvidence {
    /// Layer.
    pub layer: CheckLayer,
    /// Check name.
    pub name: Name,
    /// Whether the trusted tool passed the check.
    pub passed: bool,
    /// Digest of the tool output.
    pub digest: Hash,
}

/// Content-addressed capture or artifact pointer.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactRef {
    /// Capture or blob name.
    pub name: Name,
    /// Content hash.
    pub hash: Hash,
}

/// Named human approval.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalRef {
    /// Approver role or id.
    pub by: Name,
    /// What was approved.
    pub of: Name,
}

/// Hashes an evidence bundle must have been produced against.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceContext {
    /// Change identity.
    pub change: Hash,
    /// Authoring project hash.
    pub project_hash: Hash,
    /// Toolchain lock hash.
    pub toolchain_hash: Hash,
    /// Expanded IR hash.
    pub expanded_ir_hash: Hash,
    /// Cooked Canon hash.
    pub canon_hash: Hash,
    /// CAS root.
    pub cas_root: Hash,
}

/// Sealed evidence. The signature is blake3 of the canonical payload.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceBundle {
    /// Change identity.
    pub change: Hash,
    /// Authoring project hash.
    pub project_hash: Hash,
    /// Toolchain lock hash.
    pub toolchain_hash: Hash,
    /// Expanded IR hash.
    pub expanded_ir_hash: Hash,
    /// Cooked Canon hash.
    pub canon_hash: Hash,
    /// CAS root.
    pub cas_root: Hash,
    /// Trusted checks.
    pub checks: Vec<CheckEvidence>,
    /// Captures.
    pub captures: Vec<ArtifactRef>,
    /// Human approvals.
    pub approvals: Vec<ApprovalRef>,
    /// `evidence_signature` of the unsigned payload.
    pub signature: Hash,
}

#[derive(Serialize)]
struct UnsignedEvidence<'a> {
    change: Hash,
    project_hash: Hash,
    toolchain_hash: Hash,
    expanded_ir_hash: Hash,
    canon_hash: Hash,
    cas_root: Hash,
    checks: &'a [CheckEvidence],
    captures: &'a [ArtifactRef],
    approvals: &'a [ApprovalRef],
}

fn payload_bytes(bundle: &EvidenceBundle) -> Result<Vec<u8>, EvalError> {
    let unsigned = UnsignedEvidence {
        change: bundle.change,
        project_hash: bundle.project_hash,
        toolchain_hash: bundle.toolchain_hash,
        expanded_ir_hash: bundle.expanded_ir_hash,
        canon_hash: bundle.canon_hash,
        cas_root: bundle.cas_root,
        checks: &bundle.checks,
        captures: &bundle.captures,
        approvals: &bundle.approvals,
    };
    to_ron(&unsigned)
        .map(|s| s.into_bytes())
        .map_err(|e| EvalError::Host(e.to_string()))
}

impl EvidenceBundle {
    /// Recompute the seal. `Err` if the signature does not match.
    pub fn verify_signature(&self) -> Result<(), EvalError> {
        let bytes = payload_bytes(self)?;
        if evidence_signature(&bytes) == self.signature {
            Ok(())
        } else {
            Err(EvalError::BadSignature)
        }
    }

    /// Refuse when any context hash differs or the seal is wrong.
    pub fn accept(&self, ctx: &EvidenceContext) -> Result<(), EvalError> {
        self.verify_signature()?;
        let pairs = [
            ("change", self.change, ctx.change),
            ("project", self.project_hash, ctx.project_hash),
            ("toolchain", self.toolchain_hash, ctx.toolchain_hash),
            ("expanded_ir", self.expanded_ir_hash, ctx.expanded_ir_hash),
            ("canon", self.canon_hash, ctx.canon_hash),
            ("cas_root", self.cas_root, ctx.cas_root),
        ];
        for (field, got, expected) in pairs {
            if got != expected {
                return Err(EvalError::Stale {
                    field: field.to_owned(),
                });
            }
        }
        Ok(())
    }

    /// Read-only view. No append.
    #[must_use]
    pub fn checks(&self) -> &[CheckEvidence] {
        &self.checks
    }
}

/// Trusted builder. The agent-facing protocol is [`EvidenceBundle`] after seal.
#[derive(Clone, Debug)]
pub struct EvidenceBuilder {
    ctx: EvidenceContext,
    checks: Vec<CheckEvidence>,
    captures: Vec<ArtifactRef>,
    approvals: Vec<ApprovalRef>,
}

impl EvidenceBuilder {
    /// Start a bundle bound to `ctx`.
    #[must_use]
    pub fn new(ctx: EvidenceContext) -> Self {
        Self {
            ctx,
            checks: Vec::new(),
            captures: Vec::new(),
            approvals: Vec::new(),
        }
    }

    /// Append a trusted check.
    pub fn record_check(&mut self, layer: CheckLayer, name: Name, passed: bool, digest: Hash) {
        self.checks.push(CheckEvidence {
            layer,
            name,
            passed,
            digest,
        });
    }

    /// Append a capture produced by a trusted tool.
    pub fn record_capture(&mut self, name: Name, hash: Hash) {
        self.captures.push(ArtifactRef { name, hash });
    }

    /// Record a named human approval.
    pub fn record_approval(&mut self, by: Name, of: Name) {
        self.approvals.push(ApprovalRef { by, of });
    }

    /// Seal. Inserts an Eval activity into `dag` when provided.
    pub fn seal(self, dag: Option<&mut ProvenanceDag>) -> Result<EvidenceBundle, EvalError> {
        let mut bundle = EvidenceBundle {
            change: self.ctx.change,
            project_hash: self.ctx.project_hash,
            toolchain_hash: self.ctx.toolchain_hash,
            expanded_ir_hash: self.ctx.expanded_ir_hash,
            canon_hash: self.ctx.canon_hash,
            cas_root: self.ctx.cas_root,
            checks: self.checks,
            captures: self.captures,
            approvals: self.approvals,
            signature: Hash::ZERO,
        };
        let bytes = payload_bytes(&bundle)?;
        bundle.signature = evidence_signature(&bytes);
        if let Some(dag) = dag {
            dag.insert(
                ProvenanceKind::Activity {
                    activity: Activity::Eval,
                },
                LicenseSpan::Unknown,
                &[],
            )
            .map_err(|e| EvalError::Host(e.to_string()))?;
            debug_assert_eq!(ArtifactKind::Evidence as u8, 9);
        }
        Ok(bundle)
    }
}
