//! Wrap interest residency commands as [`Proposal::Residency`].

use std::collections::BTreeMap;
use std::sync::Arc;

use klotho_commit::{Proposal, ResidencyOp};
use klotho_core::{Hash, Sigil};
use klotho_interest::ResidencyCommand;
use klotho_world::PlaceSnap;

/// Build residency proposals from interest commands and an in-memory catalog.
///
/// [`ResidencyCommand::Load`] or [`ResidencyCommand::Evict`] with no catalog
/// entry is skipped — both ops need `Arc<PlaceSnap>`.
#[must_use]
pub fn residency_proposals(
    commands: &[ResidencyCommand],
    catalog: &BTreeMap<Sigil, Arc<PlaceSnap>>,
    live_prefix: Hash,
    live_canon: Hash,
) -> Vec<Proposal> {
    let mut out = Vec::new();
    for cmd in commands {
        let (place, op) = match *cmd {
            ResidencyCommand::Load(place) => (place, ResidencyOp::Load),
            ResidencyCommand::Evict(place) => (place, ResidencyOp::Evict),
        };
        let Some(snap) = catalog.get(&place) else {
            continue;
        };
        out.push(Proposal::Residency {
            place,
            op,
            prefix: live_prefix,
            canon_hash: live_canon,
            snap: Arc::clone(snap),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use klotho_core::LocusKind;
    use klotho_world::PlaceRow;

    use super::*;

    fn place(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Place, 0, id).unwrap()
    }

    fn snap(p: Sigil) -> Arc<PlaceSnap> {
        Arc::new(PlaceSnap::new(
            p,
            Hash::ZERO,
            Hash::from_bytes([1; 32]),
            vec![PlaceRow::new(p, LocusKind::Place)],
        ))
    }

    #[test]
    fn load_skips_missing_catalog_entry() {
        let p = place(1);
        let missing = place(2);
        let mut catalog = BTreeMap::new();
        catalog.insert(p, snap(p));
        let cmds = [
            ResidencyCommand::Load(missing),
            ResidencyCommand::Evict(missing),
            ResidencyCommand::Load(p),
        ];
        let out = residency_proposals(&cmds, &catalog, Hash::from_bytes([2; 32]), Hash::ZERO);
        assert_eq!(out.len(), 1);
        match &out[0] {
            Proposal::Residency {
                place,
                op,
                prefix,
                canon_hash,
                snap,
            } => {
                assert_eq!(*place, p);
                assert_eq!(*op, ResidencyOp::Load);
                assert_eq!(*prefix, Hash::from_bytes([2; 32]));
                assert_eq!(*canon_hash, Hash::ZERO);
                assert_eq!(snap.place, p);
            }
            other => panic!("expected Residency, got {other:?}"),
        }
    }

    #[test]
    fn evict_uses_catalog_snap() {
        let p = place(1);
        let mut catalog = BTreeMap::new();
        catalog.insert(p, snap(p));
        let out = residency_proposals(
            &[ResidencyCommand::Evict(p)],
            &catalog,
            Hash::ZERO,
            Hash::ZERO,
        );
        assert_eq!(out.len(), 1);
        match &out[0] {
            Proposal::Residency { op, .. } => assert_eq!(*op, ResidencyOp::Evict),
            other => panic!("expected Residency, got {other:?}"),
        }
    }
}
