//! Cook and report. CLI preview prints hash, bindings, and grain count.

use klotho_compile::{Cooked, cook_doc};
use klotho_ir::IntentDoc;

use crate::error::AuthorError;

/// Validate then cook `doc` against the closed kitbash.
pub fn cook_validated(doc: &IntentDoc) -> Result<Cooked, AuthorError> {
    doc.validate()?;
    cook_doc(doc).map_err(AuthorError::Cook)
}

/// One-line cook report: hash plus binding count.
#[must_use]
pub fn cook_summary(cooked: &Cooked) -> String {
    format!(
        "cook_hash={}\nbindings={}\n",
        cooked.cook_hash,
        cooked.bindings.len()
    )
}

/// CLI preview: hash, bindings, grain count. No window.
#[must_use]
pub fn preview_summary(cooked: &Cooked) -> String {
    let mut s = format!(
        "cook_hash={}\nbindings={}\ngrains={}\n",
        cooked.cook_hash,
        cooked.bindings.len(),
        cooked.grains.len()
    );
    for b in &cooked.bindings {
        s.push_str(&format!("binding locus={} tag={}\n", b.locus, b.tag));
    }
    s
}

#[cfg(test)]
mod tests {
    use klotho_compile::CompileError;
    use klotho_core::{Hash, LocusKind, Mm, PoseMm, YawMd};
    use klotho_ir::{IntentDoc, Name, ProvenanceId, SeedFact, StyleIntent};

    use super::*;
    use crate::error::AuthorError;

    fn stool_chair() -> IntentDoc {
        IntentDoc {
            style: StyleIntent {
                notes: String::new(),
                palettes: Vec::new(),
                kitbash_tags: vec![Name::from("prop.stool")],
            },
            canon_diffs: Vec::new(),
            seed: vec![
                SeedFact::Locus {
                    name: Name::from("chair"),
                    kind: LocusKind::Relic,
                },
                SeedFact::Pose {
                    of: Name::from("chair"),
                    pose: PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0)),
                },
            ],
            minds: Vec::new(),
            provenance: ProvenanceId(Hash::ZERO),
        }
    }

    #[test]
    fn missing_kitbash_tag_is_cook_error() {
        let mut doc = stool_chair();
        doc.style.kitbash_tags = vec![Name::from("no.such.tag")];
        let err = cook_validated(&doc).unwrap_err();
        assert!(err.to_string().contains("no.such.tag"));
        match err {
            AuthorError::Cook(CompileError::MissingTag(t)) => assert_eq!(t, "no.such.tag"),
            other => panic!("{other}"),
        }
    }

    #[test]
    fn chair_with_stool_tag_cooks() {
        let cooked = cook_validated(&stool_chair()).unwrap();
        assert!(!cooked.cook_hash.to_string().is_empty());
        assert_eq!(
            cooked.cook_hash,
            cook_validated(&stool_chair()).unwrap().cook_hash
        );
        let preview = preview_summary(&cooked);
        assert!(preview.contains("cook_hash="));
        assert!(preview.contains("grains="));
        let cook = cook_summary(&cooked);
        assert!(cook.contains(&cooked.cook_hash.to_string()));
    }
}
