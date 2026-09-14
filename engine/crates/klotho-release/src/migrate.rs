//! Save and DLC migration across Canon epoch packs.

use std::sync::Arc;

use klotho_compile::CanonEpochPack;
use klotho_core::ResourceId;
use klotho_save::SaveBlob;
use klotho_trace::{TraceBody, TraceEvent};
use klotho_world::{SnapRow, WorldSnapshot};

use crate::ReleaseError;

/// Remap a pause save onto `pack`. Packed ids without a mapping fail closed.
///
/// The original blob is not mutated. Projection columns are rewritten as a
/// whole snapshot; they are never merged with another device's columns.
///
/// # Errors
///
/// Returns [`ReleaseError::Migrate`] when identity does not match `pack.from_*`
/// or a packed id was removed.
pub fn migrate_save(blob: &SaveBlob, pack: &CanonEpochPack) -> Result<SaveBlob, ReleaseError> {
    if blob.canon_hash != pack.from_canon_hash || blob.epoch != pack.from_epoch {
        return Err(ReleaseError::migrate(
            "save is not the pack's from identity",
        ));
    }
    let mut rows = blob.snap.snap_rows();
    for row in &mut rows {
        remap_row(row, pack)?;
    }
    let snap = Arc::new(WorldSnapshot::from_snap_rows(
        pack.epoch,
        blob.snap.tick,
        pack.canon_hash,
        blob.prefix,
        None,
        rows,
    )?);
    let mut suffix = blob.suffix.clone();
    for event in &mut suffix {
        remap_event(event, pack)?;
    }
    Ok(SaveBlob {
        canon_hash: pack.canon_hash,
        epoch: pack.epoch,
        prefix: blob.prefix,
        snap,
        suffix,
        trace_from_tick: blob.trace_from_tick,
    })
}

fn remap_row(row: &mut SnapRow, pack: &CanonEpochPack) -> Result<(), ReleaseError> {
    for (res, _) in &mut row.qty {
        *res = map_resource(*res, pack)?;
    }
    for fact in &mut row.knows {
        *fact = pack
            .map
            .fact(*fact)
            .ok_or_else(|| ReleaseError::migrate(format!("fact {fact} removed")))?;
    }
    for (rite, _) in &mut row.rites {
        *rite = pack
            .map
            .rite(klotho_canon::RiteId(*rite))
            .map(|id| id.0)
            .ok_or_else(|| ReleaseError::migrate(format!("rite {rite} removed")))?;
    }
    Ok(())
}

fn remap_event(event: &mut TraceEvent, pack: &CanonEpochPack) -> Result<(), ReleaseError> {
    match &mut event.body {
        TraceBody::QtyChanged { res, .. } => {
            *res = map_resource(*res, pack)?;
        }
        TraceBody::Learned { fact, .. } => {
            *fact = pack
                .map
                .fact(*fact)
                .ok_or_else(|| ReleaseError::migrate(format!("fact {fact} removed")))?;
        }
        TraceBody::RiteBegan { rite, .. }
        | TraceBody::RiteAdvanced { rite, .. }
        | TraceBody::RiteEnded { rite, .. } => {
            *rite = pack
                .map
                .rite(klotho_canon::RiteId(*rite))
                .map(|id| id.0)
                .ok_or_else(|| ReleaseError::migrate(format!("rite {rite} removed")))?;
        }
        TraceBody::Uttered { fact_ids, .. } => {
            for fact in fact_ids {
                *fact = pack
                    .map
                    .fact(*fact)
                    .ok_or_else(|| ReleaseError::migrate(format!("utterance {fact} removed")))?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn map_resource(old: ResourceId, pack: &CanonEpochPack) -> Result<ResourceId, ReleaseError> {
    pack.map
        .resource(old)
        .ok_or_else(|| ReleaseError::migrate(format!("resource {} removed", old.0)))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use klotho_compile::{cook_doc, cook_epoch_pack};
    use klotho_core::{Epoch, Hash, LocusKind, Sigil, Tick};
    use klotho_ir::{
        CanonDiff, IntentDoc, Law, LawBody, Name, Pred, ProvenanceId, SeedFact, StyleIntent, Verb,
    };
    use klotho_save::pause_save;
    use klotho_world::{SnapRow, WorldSnapshot};

    use super::*;

    fn doc() -> IntentDoc {
        IntentDoc {
            style: StyleIntent::default(),
            canon_diffs: vec![
                CanonDiff::AddLaw(Law {
                    id: Name::from("live.remove_me"),
                    when: Pred::EqVerb(Verb::Use),
                    body: LawBody::Ramp {
                        res: Name::from("live_obsolete"),
                        per_tick: 0,
                        quantum: 1,
                        cap: 1,
                    },
                }),
                CanonDiff::AddLaw(Law {
                    id: Name::from("live.keep"),
                    when: Pred::EqVerb(Verb::Use),
                    body: LawBody::Ramp {
                        res: Name::from("health"),
                        per_tick: 0,
                        quantum: 1,
                        cap: 100,
                    },
                }),
            ],
            seed: vec![SeedFact::Locus {
                name: Name::from("player"),
                kind: LocusKind::Actor,
            }],
            minds: Vec::new(),
            provenance: ProvenanceId(Hash::ZERO),
        }
    }

    fn relic() -> Sigil {
        Sigil::pack(LocusKind::Relic, 0, 1).unwrap()
    }

    #[test]
    fn prior_save_migrates_health_id() {
        let cooked = cook_doc(&doc()).unwrap();
        let old_health = cooked.canon.resource_id("health").unwrap();
        let mut row = SnapRow::new(relic(), LocusKind::Relic);
        row.qty = vec![(old_health, 7)];
        let snap = Arc::new(
            WorldSnapshot::from_snap_rows(
                Epoch::ZERO,
                Tick(1),
                cooked.canon_hash,
                Hash::from_bytes([3; 32]),
                None,
                vec![row],
            )
            .unwrap(),
        );
        let blob = pause_save(&snap).unwrap();
        let pack = cook_epoch_pack(
            &cooked,
            Epoch::ZERO,
            &[CanonDiff::RetractLaw {
                id: Name::from("live.remove_me"),
                reason: "live cleanup".into(),
            }],
        )
        .unwrap();
        let migrated = migrate_save(&blob, &pack).unwrap();
        let new_health = pack.canon.resource_id("health").unwrap();
        assert_eq!(migrated.canon_hash, pack.canon_hash);
        assert_eq!(migrated.epoch, Epoch(1));
        assert_eq!(migrated.snap.view().qty(relic(), new_health), 7);
        assert_eq!(blob.canon_hash, cooked.canon_hash);
    }

    #[test]
    fn removed_resource_fails_and_leaves_original() {
        let cooked = cook_doc(&doc()).unwrap();
        let obsolete = cooked.canon.resource_id("live_obsolete").unwrap();
        let mut row = SnapRow::new(relic(), LocusKind::Relic);
        row.qty = vec![(obsolete, 1)];
        let snap = Arc::new(
            WorldSnapshot::from_snap_rows(
                Epoch::ZERO,
                Tick(1),
                cooked.canon_hash,
                Hash::ZERO,
                None,
                vec![row],
            )
            .unwrap(),
        );
        let blob = pause_save(&snap).unwrap();
        let pack = cook_epoch_pack(
            &cooked,
            Epoch::ZERO,
            &[CanonDiff::RetractLaw {
                id: Name::from("live.remove_me"),
                reason: "live cleanup".into(),
            }],
        )
        .unwrap();
        assert!(migrate_save(&blob, &pack).is_err());
        assert_eq!(blob.snap.view().qty(relic(), obsolete), 1);
    }
}
