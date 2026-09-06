//! Offline Canon epoch-pack cook for live-ops transitions.

use klotho_canon::{Canon, EpochMap, cook as cook_canon};
use klotho_core::{Epoch, Hash};
use klotho_ir::{CanonDiff, IntentDoc, to_ron};
use klotho_prove::hash_bytes;

use crate::{CompileError, Cooked};

/// A validated Canon replacement and the packed-id map needed to apply it.
///
/// This is cooked offline. Runtime code can apply this value but cannot add a
/// Law or reinterpret authoring diffs.
#[derive(Clone, Debug)]
pub struct CanonEpochPack {
    /// Canon hash that this pack may replace.
    pub from_canon_hash: Hash,
    /// Epoch that this pack may replace.
    pub from_epoch: Epoch,
    /// New Canon identity, chained from the old identity and patch bytes.
    pub canon_hash: Hash,
    /// New epoch (`from_epoch + 1`).
    pub epoch: Epoch,
    /// Fully cooked replacement Canon.
    pub canon: Canon,
    /// Stable-name packed-id translation.
    pub map: EpochMap,
    /// Resulting authoring document, retained for a subsequent pack cook.
    pub doc: IntentDoc,
}

/// Cook a `CanonDiff` pack against the currently installed cooked document.
///
/// The digest is an ancestry chain, not merely a content hash: reverting to
/// byte-identical Canon in a later epoch still produces a distinct identity.
pub fn cook_epoch_pack(
    base: &Cooked,
    from_epoch: Epoch,
    diffs: &[CanonDiff],
) -> Result<CanonEpochPack, CompileError> {
    cook_epoch_pack_from(&base.doc, &base.canon, base.canon_hash, from_epoch, diffs)
}

/// Cook the successor of an already-cooked epoch pack.
pub fn cook_next_epoch_pack(
    base: &CanonEpochPack,
    diffs: &[CanonDiff],
) -> Result<CanonEpochPack, CompileError> {
    cook_epoch_pack_from(&base.doc, &base.canon, base.canon_hash, base.epoch, diffs)
}

fn cook_epoch_pack_from(
    base_doc: &IntentDoc,
    base_canon: &Canon,
    base_hash: Hash,
    from_epoch: Epoch,
    diffs: &[CanonDiff],
) -> Result<CanonEpochPack, CompileError> {
    let next = from_epoch
        .0
        .checked_add(1)
        .map(Epoch)
        .ok_or(CompileError::EpochOverflow)?;
    let mut doc = base_doc.clone();
    doc.canon_diffs.extend_from_slice(diffs);
    let canon = cook_canon(&doc).map_err(CompileError::canon)?;
    let map = EpochMap::between(base_canon, &canon);
    let patch = to_ron(&diffs.to_vec()).map_err(|e| CompileError::Canon(e.to_string()))?;
    let whole = to_ron(&doc.canon_diffs).map_err(|e| CompileError::Canon(e.to_string()))?;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"KCEP");
    bytes.extend_from_slice(&COMPILER_EPOCH_PACK_VERSION.to_le_bytes());
    bytes.extend_from_slice(base_hash.as_bytes());
    bytes.extend_from_slice(&from_epoch.0.to_le_bytes());
    bytes.extend_from_slice(&next.0.to_le_bytes());
    put_section(&mut bytes, patch.as_bytes());
    put_section(&mut bytes, whole.as_bytes());
    let canon_hash = hash_bytes(&bytes);
    Ok(CanonEpochPack {
        from_canon_hash: base_hash,
        from_epoch,
        canon_hash,
        epoch: next,
        canon,
        map,
        doc,
    })
}

/// Epoch-pack encoding version mixed into the chained Canon hash.
pub const COMPILER_EPOCH_PACK_VERSION: u32 = 1;

fn put_section(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&u32::try_from(bytes.len()).unwrap_or(u32::MAX).to_le_bytes());
    out.extend_from_slice(bytes);
}

#[cfg(test)]
mod tests {
    use klotho_core::{Hash, LocusKind};
    use klotho_ir::{
        IntentDoc, Law, LawBody, Name, Pred, ProvenanceId, SeedFact, StyleIntent, Verb,
    };

    use super::*;
    use crate::cook_doc;

    #[test]
    fn pack_is_chained_and_resource_map_is_name_stable() {
        let base = IntentDoc {
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
        };
        let cooked = cook_doc(&base).unwrap();
        let old_health = cooked.canon.resource_id("health").unwrap();
        let pack = cook_epoch_pack(
            &cooked,
            Epoch::ZERO,
            &[CanonDiff::RetractLaw {
                id: Name::from("live.remove_me"),
                reason: "live cleanup".into(),
            }],
        )
        .unwrap();
        let new_health = pack.canon.resource_id("health").unwrap();
        assert_ne!(pack.canon_hash, cooked.canon_hash);
        assert_eq!(pack.epoch, Epoch(1));
        assert_eq!(pack.map.resource(old_health), Some(new_health));
        assert_ne!(old_health, new_health);

        let next = cook_next_epoch_pack(&pack, &[]).unwrap();
        assert_eq!(next.from_canon_hash, pack.canon_hash);
        assert_eq!(next.from_epoch, Epoch(1));
        assert_eq!(next.epoch, Epoch(2));
        assert_ne!(next.canon_hash, pack.canon_hash);
    }

    #[test]
    fn epoch_overflow_is_refused() {
        let cooked = cook_doc(&hearth_slice::hearth_doc()).unwrap();
        assert_eq!(
            cook_epoch_pack(&cooked, Epoch(u64::MAX), &[]).unwrap_err(),
            CompileError::EpochOverflow
        );
    }
}
