//! Structural validator. No execution, no NL.

use crate::doc::IntentDoc;
use crate::error::IrError;

/// Walk an [`IntentDoc`] and reject empty names, nested quantifiers, bad caps.
pub fn validate_doc(doc: &IntentDoc) -> Result<(), IrError> {
    doc.style.check()?;
    for d in &doc.canon_diffs {
        d.check()?;
    }
    for s in &doc.seed {
        s.check()?;
    }
    for m in &doc.minds {
        m.check()?;
    }
    Ok(())
}
