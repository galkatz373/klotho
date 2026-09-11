//! Open a sharded catalog and load Place snaps. Runtime builds Residency proposals.

use std::path::Path;
use std::sync::Arc;

use klotho_core::Sigil;
use klotho_stream::{StreamCatalog, map_place};
use klotho_world::PlaceSnap;

/// Open `catalog.kwrp` (capped, header-validated).
pub fn open_stream_catalog(path: &Path) -> Result<StreamCatalog, String> {
    StreamCatalog::open(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Map a Place shard through the stream crate. Does not build a `Proposal`.
pub fn load_place_snap(catalog: &StreamCatalog, place: Sigil) -> Result<Arc<PlaceSnap>, String> {
    map_place(catalog, place).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;

    use klotho_compile::{cook_doc, write_catalog};
    use klotho_core::{AabbMm, Budget, Hash, IVec3, LocusKind, RejectReason, Tick};
    use klotho_interest::ResidencyCommand;
    use klotho_stream::PlaceRow;
    use klotho_world::PlaceSnap;

    use super::*;
    use crate::{kernel_from_cooked, residency_proposals};

    fn place(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Place, 0, id).unwrap()
    }

    fn temp_dir() -> std::path::PathBuf {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "klotho-runtime-stream-{}-{}",
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn mapped_snap_residency_is_admitted() {
        let cooked = cook_doc(&hearth_slice::hearth_doc()).unwrap();
        let p = place(99_003);
        let snap = PlaceSnap::new(
            p,
            cooked.canon_hash,
            Hash::from_bytes([1; 32]),
            vec![PlaceRow::new(p, LocusKind::Place)],
        );
        let dir = temp_dir();
        let man = write_catalog(&dir, &cooked, &[(snap, AabbMm::from_point(IVec3::ZERO))]).unwrap();

        let cat = open_stream_catalog(&man.catalog_path).unwrap();
        let loaded = load_place_snap(&cat, p).unwrap();
        assert_eq!(loaded.place, p);

        let mut catalog = BTreeMap::new();
        catalog.insert(p, loaded);

        let mut k = kernel_from_cooked(&cooked).unwrap();
        let live_canon = k.world().canon_hash();
        assert_eq!(live_canon, cooked.canon_hash);
        let props = residency_proposals(
            &[ResidencyCommand::Load(p)],
            &catalog,
            k.world().trace_prefix_hash(),
            live_canon,
        );
        assert_eq!(props.len(), 1);
        for prop in props {
            k.ingest(prop);
        }
        let d = k.step(Tick(1), Budget::HEARTH, &mut []).unwrap();
        assert!(
            !d.rejects.iter().any(|(_, r)| {
                matches!(r, RejectReason::Residency | RejectReason::EpochMismatch)
            }),
            "{d:?}"
        );
        assert!(k.world().view().contains(p));
        let _ = fs::remove_dir_all(&dir);
    }
}
