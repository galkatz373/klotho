//! Cook-time Pin: freeze a preview fact into Canon or seed Trace.

use klotho_ir::{CanonDiff, IntentDoc, SeedFact};

use crate::error::AuthorError;

/// Cook-time Pin. Nothing is real until Pin.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum Pin {
    /// Apply a [`CanonDiff`] onto [`IntentDoc::canon_diffs`].
    ToCanon {
        /// Diff appended. An earlier same-variant+id is moved to the end unless a
        /// later opposite (AddLaw ↔ RetractLaw) still applies.
        diff: CanonDiff,
        /// Non-empty author justification.
        reason: String,
    },
    /// Apply a [`SeedFact`] (Locus / Rel / Qty / Pose) onto [`IntentDoc::seed`].
    ToSeedTrace {
        /// Seed fact. Matching identity replaces rather than duplicates.
        fact: SeedFact,
        /// Non-empty author justification.
        reason: String,
    },
    /// Record a rejection. Does not mutate seed or Canon.
    Reject {
        /// Preview proposal the author declined.
        proposal_id: u64,
        /// Non-empty author justification.
        reason: String,
    },
}

/// Apply `pin` to `doc`. Seed facts replace matching identity in place so
/// pinning twice yields the same seed RON. Canon diffs last-pin-wins: an
/// earlier same-variant+id is removed and the new diff is appended, unless a
/// later opposite on that id (AddLaw ↔ RetractLaw) must stay in the ledger.
pub fn apply_pin(doc: &mut IntentDoc, pin: Pin) -> Result<(), AuthorError> {
    match pin {
        Pin::ToCanon { diff, reason } => {
            require_reason(&reason)?;
            upsert_canon(&mut doc.canon_diffs, diff);
            Ok(())
        }
        Pin::ToSeedTrace { fact, reason } => {
            require_reason(&reason)?;
            upsert_seed(&mut doc.seed, fact);
            Ok(())
        }
        Pin::Reject { reason, .. } => {
            require_reason(&reason)?;
            Ok(())
        }
    }
}

fn require_reason(reason: &str) -> Result<(), AuthorError> {
    if reason.trim().is_empty() {
        Err(AuthorError::EmptyPinReason)
    } else {
        Ok(())
    }
}

fn upsert_seed(seed: &mut Vec<SeedFact>, fact: SeedFact) {
    if let Some(slot) = seed.iter_mut().find(|f| seed_identity(f, &fact)) {
        *slot = fact;
    } else {
        seed.push(fact);
    }
}

fn seed_identity(a: &SeedFact, b: &SeedFact) -> bool {
    match (a, b) {
        (SeedFact::Locus { name: x, .. }, SeedFact::Locus { name: y, .. }) => x == y,
        (SeedFact::Pose { of: x, .. }, SeedFact::Pose { of: y, .. }) => x == y,
        (
            SeedFact::Rel {
                a: a1,
                rel: r1,
                b: b1,
            },
            SeedFact::Rel {
                a: a2,
                rel: r2,
                b: b2,
            },
        ) => a1 == a2 && r1 == r2 && b1 == b2,
        (
            SeedFact::Qty {
                of: o1, res: r1, ..
            },
            SeedFact::Qty {
                of: o2, res: r2, ..
            },
        ) => o1 == o2 && r1 == r2,
        _ => false,
    }
}

fn upsert_canon(diffs: &mut Vec<CanonDiff>, diff: CanonDiff) {
    if let Some(i) = diffs.iter().rposition(|d| canon_identity(d, &diff)) {
        let blocked = diffs[i + 1..].iter().any(|d| canon_opposite(d, &diff));
        if !blocked {
            diffs.remove(i);
        }
    }
    diffs.push(diff);
}

fn canon_identity(a: &CanonDiff, b: &CanonDiff) -> bool {
    match (a, b) {
        (CanonDiff::AddLaw(x), CanonDiff::AddLaw(y)) => x.id == y.id,
        (CanonDiff::RetractLaw { id: x, .. }, CanonDiff::RetractLaw { id: y, .. }) => x == y,
        (CanonDiff::AddAffordance(x), CanonDiff::AddAffordance(y)) => x.id == y.id,
        (CanonDiff::AddRite(x), CanonDiff::AddRite(y)) => x.id == y.id,
        (CanonDiff::RetractRite { id: x, .. }, CanonDiff::RetractRite { id: y, .. }) => x == y,
        (CanonDiff::AddBeat(x), CanonDiff::AddBeat(y)) => x.id == y.id,
        _ => false,
    }
}

fn canon_opposite(a: &CanonDiff, b: &CanonDiff) -> bool {
    match (a, b) {
        (CanonDiff::AddLaw(x), CanonDiff::RetractLaw { id: y, .. }) => x.id == *y,
        (CanonDiff::RetractLaw { id: x, .. }, CanonDiff::AddLaw(y)) => *x == y.id,
        (CanonDiff::AddRite(x), CanonDiff::RetractRite { id: y, .. }) => x.id == *y,
        (CanonDiff::RetractRite { id: x, .. }, CanonDiff::AddRite(y)) => *x == y.id,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use klotho_core::{Hash, LocusKind, Mm, PoseMm, YawMd};
    use klotho_ir::{
        Affordance, CanonDiff, IntentDoc, Law, LawBody, Name, Pred, ProvenanceId, SeedFact,
        StyleIntent, Verb, to_ron, validate_doc,
    };

    use super::*;
    use crate::preview::cook_validated;

    fn blank_stool() -> IntentDoc {
        IntentDoc {
            style: StyleIntent {
                notes: String::new(),
                palettes: Vec::new(),
                kitbash_tags: vec![Name::from("prop.stool")],
            },
            canon_diffs: Vec::new(),
            seed: Vec::new(),
            minds: Vec::new(),
            provenance: ProvenanceId(Hash::ZERO),
        }
    }

    fn chair_locus() -> Pin {
        Pin::ToSeedTrace {
            fact: SeedFact::Locus {
                name: Name::from("chair"),
                kind: LocusKind::Relic,
            },
            reason: "place the chair".into(),
        }
    }

    fn chair_pose() -> Pin {
        Pin::ToSeedTrace {
            fact: SeedFact::Pose {
                of: Name::from("chair"),
                pose: PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0)),
            },
            reason: "pose the chair".into(),
        }
    }

    fn pin_chair(doc: &mut IntentDoc) {
        apply_pin(doc, chair_locus()).unwrap();
        apply_pin(doc, chair_pose()).unwrap();
    }

    #[test]
    fn pin_chair_recook_same_cook_hash() {
        let mut a = blank_stool();
        pin_chair(&mut a);
        validate_doc(&a).unwrap();
        let cooked_a = cook_validated(&a).unwrap();

        let mut b = blank_stool();
        pin_chair(&mut b);
        let cooked_b = cook_validated(&b).unwrap();

        assert_eq!(cooked_a.cook_hash, cooked_b.cook_hash);
        assert_eq!(to_ron(&a.seed).unwrap(), to_ron(&b.seed).unwrap());
        assert!(a.seed.iter().any(|f| matches!(
            f,
            SeedFact::Locus {
                name,
                kind: LocusKind::Relic
            } if name.as_str() == "chair"
        )));
    }

    #[test]
    fn pin_chair_twice_is_stable() {
        let mut once = blank_stool();
        pin_chair(&mut once);
        let hash_once = cook_validated(&once).unwrap().cook_hash;
        let seed_once = to_ron(&once.seed).unwrap();

        pin_chair(&mut once);
        let hash_twice = cook_validated(&once).unwrap().cook_hash;
        assert_eq!(hash_once, hash_twice);
        assert_eq!(seed_once, to_ron(&once.seed).unwrap());
        assert_eq!(
            once.seed
                .iter()
                .filter(|f| matches!(f, SeedFact::Locus { name, .. } if name.as_str() == "chair"))
                .count(),
            1
        );
        assert_eq!(
            once.seed
                .iter()
                .filter(|f| matches!(f, SeedFact::Pose { of, .. } if of.as_str() == "chair"))
                .count(),
            1
        );
    }

    #[test]
    fn reject_pin_does_not_change_seed() {
        let mut doc = blank_stool();
        pin_chair(&mut doc);
        let before = to_ron(&doc).unwrap();
        apply_pin(
            &mut doc,
            Pin::Reject {
                proposal_id: 7,
                reason: "leave the stool where it is".into(),
            },
        )
        .unwrap();
        assert_eq!(before, to_ron(&doc).unwrap());
    }

    #[test]
    fn empty_pin_reason_is_rejected() {
        let mut doc = blank_stool();
        let err = apply_pin(
            &mut doc,
            Pin::ToSeedTrace {
                fact: SeedFact::Locus {
                    name: Name::from("chair"),
                    kind: LocusKind::Relic,
                },
                reason: String::new(),
            },
        )
        .unwrap_err();
        assert_eq!(err, AuthorError::EmptyPinReason);
        let err = apply_pin(
            &mut doc,
            Pin::ToSeedTrace {
                fact: SeedFact::Locus {
                    name: Name::from("chair"),
                    kind: LocusKind::Relic,
                },
                reason: "   ".into(),
            },
        )
        .unwrap_err();
        assert_eq!(err, AuthorError::EmptyPinReason);
        assert!(doc.seed.is_empty());
    }

    #[test]
    fn to_canon_retract_is_cook_time() {
        let mut doc = blank_stool();
        apply_pin(
            &mut doc,
            Pin::ToCanon {
                diff: CanonDiff::AddLaw(Law {
                    id: Name::from("lock.use"),
                    when: Pred::EqVerb(Verb::Use),
                    body: LawBody::Pred {
                        must: Pred::EqVerb(Verb::Use),
                        ought: None,
                    },
                }),
                reason: "admit Use".into(),
            },
        )
        .unwrap();
        apply_pin(
            &mut doc,
            Pin::ToCanon {
                diff: CanonDiff::RetractLaw {
                    id: Name::from("lock.use"),
                    reason: "not in this slice".into(),
                },
                reason: "drop lock.use".into(),
            },
        )
        .unwrap();
        assert!(matches!(
            &doc.canon_diffs[..],
            [CanonDiff::AddLaw(_), CanonDiff::RetractLaw { id, .. }]
                if id.as_str() == "lock.use"
        ));
        assert!(
            cook_validated(&doc)
                .unwrap()
                .canon
                .law_id("lock.use")
                .is_none()
        );
    }

    fn lock_use(body: Pred) -> CanonDiff {
        CanonDiff::AddLaw(Law {
            id: Name::from("lock.use"),
            when: Pred::EqVerb(Verb::Use),
            body: LawBody::Pred {
                must: body,
                ought: None,
            },
        })
    }

    #[test]
    fn add_retract_add_leaves_law_in_force() {
        let mut doc = blank_stool();
        apply_pin(
            &mut doc,
            Pin::ToCanon {
                diff: lock_use(Pred::EqVerb(Verb::Use)),
                reason: "admit Use".into(),
            },
        )
        .unwrap();
        apply_pin(
            &mut doc,
            Pin::ToCanon {
                diff: CanonDiff::RetractLaw {
                    id: Name::from("lock.use"),
                    reason: "not in this slice".into(),
                },
                reason: "drop lock.use".into(),
            },
        )
        .unwrap();
        apply_pin(
            &mut doc,
            Pin::ToCanon {
                diff: lock_use(Pred::EqVerb(Verb::Talk)),
                reason: "bring it back".into(),
            },
        )
        .unwrap();
        assert!(matches!(
            &doc.canon_diffs[..],
            [
                CanonDiff::AddLaw(_),
                CanonDiff::RetractLaw { .. },
                CanonDiff::AddLaw(law)
            ] if law.body == LawBody::Pred {
                must: Pred::EqVerb(Verb::Talk),
                ought: None,
            }
        ));
        assert!(
            cook_validated(&doc)
                .unwrap()
                .canon
                .law_id("lock.use")
                .is_some()
        );
    }

    #[test]
    fn pin_same_add_law_twice_replaces_trailing() {
        let mut doc = blank_stool();
        apply_pin(
            &mut doc,
            Pin::ToCanon {
                diff: lock_use(Pred::EqVerb(Verb::Use)),
                reason: "first".into(),
            },
        )
        .unwrap();
        apply_pin(
            &mut doc,
            Pin::ToCanon {
                diff: lock_use(Pred::EqVerb(Verb::Talk)),
                reason: "second".into(),
            },
        )
        .unwrap();
        assert_eq!(doc.canon_diffs.len(), 1);
        match &doc.canon_diffs[0] {
            CanonDiff::AddLaw(law) => {
                assert_eq!(
                    law.body,
                    LawBody::Pred {
                        must: Pred::EqVerb(Verb::Talk),
                        ought: None,
                    }
                );
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn add_law_after_affordance_rewrites_earlier_slot() {
        let mut doc = blank_stool();
        apply_pin(
            &mut doc,
            Pin::ToCanon {
                diff: lock_use(Pred::EqVerb(Verb::Use)),
                reason: "admit Use".into(),
            },
        )
        .unwrap();
        apply_pin(
            &mut doc,
            Pin::ToCanon {
                diff: CanonDiff::AddAffordance(Affordance {
                    id: Name::from("Sittable"),
                    requires: vec![],
                    grants: vec![],
                    conflicts: vec![],
                }),
                reason: "stool sits".into(),
            },
        )
        .unwrap();
        apply_pin(
            &mut doc,
            Pin::ToCanon {
                diff: lock_use(Pred::EqVerb(Verb::Talk)),
                reason: "rewrite law".into(),
            },
        )
        .unwrap();
        assert!(matches!(
            &doc.canon_diffs[..],
            [CanonDiff::AddAffordance(_), CanonDiff::AddLaw(law)]
                if law.body == LawBody::Pred {
                    must: Pred::EqVerb(Verb::Talk),
                    ought: None,
                }
        ));
        let cooked = cook_validated(&doc).unwrap();
        assert!(cooked.canon.law_id("lock.use").is_some());
        assert!(cooked.canon.affordance_id("Sittable").is_some());
    }
}
