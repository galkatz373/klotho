//! The Intent document: one AST for RON and (later) kdown.

use serde::{Deserialize, Serialize};

use klotho_prove::ProvenanceId;

use crate::decl::CanonDiff;
use crate::error::IrError;
use crate::mind::MindSpec;
use crate::seed::SeedFact;
use crate::style::StyleIntent;

/// Authoring document. Cooked into Canon + seed Trace + CAS.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub struct IntentDoc {
    /// Presentation / kitbash hints.
    pub style: StyleIntent,
    /// Canon patches. `RetractLaw` is cook-time only (K16).
    pub canon_diffs: Vec<CanonDiff>,
    /// Seed Trace facts (loci, rels, qtys, poses).
    pub seed: Vec<SeedFact>,
    /// NPC mind specs.
    pub minds: Vec<MindSpec>,
    /// Provenance root of this document.
    pub provenance: ProvenanceId,
}

impl IntentDoc {
    /// Structural validation. Does not compile predicates or check kitbash tags.
    pub fn validate(&self) -> Result<(), IrError> {
        crate::validate::validate_doc(self)
    }
}
