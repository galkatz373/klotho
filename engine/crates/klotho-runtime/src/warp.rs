//! Load a `.warp` with caps applied before the bytes are trusted.

use std::path::Path;
use std::sync::Arc;

use klotho_commit::CommitKernel;
use klotho_compile::{Cooked, WARP_CAP_DESKTOP, unpack_warp};
use klotho_core::PlayerId;
use klotho_ir::SeedFact;
use klotho_platform::read_capped;
use klotho_world::World;

use crate::RuntimeProfile;

/// Read a `.warp` with the desktop file cap, then unpack (headers, blob caps, licenses).
pub fn load_cooked_warp(path: &Path) -> Result<Cooked, String> {
    load_cooked_warp_capped(path, WARP_CAP_DESKTOP)
}

/// Same as [`load_cooked_warp`] with an explicit read cap (tests use a small max).
pub fn load_cooked_warp_capped(path: &Path, cap: usize) -> Result<Cooked, String> {
    let bytes = read_capped(path, cap).map_err(|e| format!("{}: {e}", path.display()))?;
    unpack_warp(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// Boot a [`CommitKernel`] from a `.warp` file.
///
/// Applies packed Canon and seed facts only. Hearth projection affordances
/// (`Opaque`/`Lockable` on the door, `Portable`/`Flammable` on barrels) are
/// written by `hearth_slice::boot` after seed and are not in the package; this
/// is not a substitute for that path.
pub fn load_warp(path: &Path) -> Result<CommitKernel, String> {
    let cooked = load_cooked_warp(path)?;
    kernel_from_cooked(&cooked)
}

/// Seed loci from the packed IntentDoc and bind player 0 when a `player` pin exists.
///
/// Canon + seed only — not `hearth_slice::boot()`.
pub fn kernel_from_cooked(cooked: &Cooked) -> Result<CommitKernel, String> {
    kernel_from_cooked_profile(cooked, RuntimeProfile::Hearth)
}

/// Seed a kernel using the capacity selected by `profile`.
pub fn kernel_from_cooked_profile(
    cooked: &Cooked,
    profile: RuntimeProfile,
) -> Result<CommitKernel, String> {
    let mut k = CommitKernel::new(World::with_locus_cap(
        Arc::new(cooked.canon.clone()),
        cooked.canon_hash,
        profile.locus_cap(),
    ));
    apply_seed(&mut k, &cooked.doc.seed)?;
    if let Some(player) = k.canon().pin("player") {
        k.bind_player(PlayerId(0), player);
    }
    Ok(k)
}

fn apply_seed(k: &mut CommitKernel, seed: &[SeedFact]) -> Result<(), String> {
    for fact in seed {
        match fact {
            SeedFact::Physics { .. } | SeedFact::ContactTrack { .. } => {} // Configuration was bound by Canon cook.

            SeedFact::Locus { name, kind } => {
                let s = k
                    .canon()
                    .pin(name.as_str())
                    .ok_or_else(|| format!("pin {}", name.as_str()))?;
                k.world_mut()
                    .insert_locus(s, *kind)
                    .map_err(|e| format!("locus {}: {e}", name.as_str()))?;
            }
            SeedFact::Rel { a, rel, b } => {
                let sa = k
                    .canon()
                    .pin(a.as_str())
                    .ok_or_else(|| format!("pin {}", a.as_str()))?;
                let sb = k
                    .canon()
                    .pin(b.as_str())
                    .ok_or_else(|| format!("pin {}", b.as_str()))?;
                k.world_mut()
                    .add_rel(sa, *rel, sb)
                    .map_err(|e| format!("rel: {e}"))?;
            }
            SeedFact::Qty { of, res, value } => {
                let s = k
                    .canon()
                    .pin(of.as_str())
                    .ok_or_else(|| format!("pin {}", of.as_str()))?;
                let r = k
                    .canon()
                    .resource_id(res.as_str())
                    .ok_or_else(|| format!("resource {}", res.as_str()))?;
                k.world_mut()
                    .set_qty(s, r, *value)
                    .map_err(|e| format!("qty: {e}"))?;
            }
            SeedFact::Pose { of, pose } => {
                let s = k
                    .canon()
                    .pin(of.as_str())
                    .ok_or_else(|| format!("pin {}", of.as_str()))?;
                k.world_mut()
                    .set_pose(s, *pose)
                    .map_err(|e| format!("pose: {e}"))?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use klotho_compile::{WARP_CAP_MOBILE, cook_doc, write_warp};
    use klotho_platform::read_capped;

    use super::*;

    fn temp_warp() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "klotho-runtime-{}.warp",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ))
    }

    #[test]
    fn loader_uses_desktop_cap_constant() {
        assert_eq!(WARP_CAP_DESKTOP, 512 * 1024 * 1024);
        assert_eq!(WARP_CAP_MOBILE, 192 * 1024 * 1024);
    }

    #[test]
    fn hearth_warp_loads_and_oversize_is_refused_before_read() {
        let cooked = cook_doc(&hearth_slice::hearth_doc()).unwrap();
        let path = temp_warp();
        write_warp(&path, &cooked).unwrap();
        assert!(path.is_file());

        let cap_err = read_capped(&path, 8).unwrap_err();
        assert_eq!(cap_err.kind(), std::io::ErrorKind::InvalidData);
        let load_err = load_cooked_warp_capped(&path, 8).unwrap_err();
        assert!(
            load_err.contains("exceeds cap") || load_err.contains("InvalidData"),
            "{load_err}"
        );

        let loaded = load_cooked_warp(&path).unwrap();
        assert_eq!(loaded.cook_hash, cooked.cook_hash);
        assert_eq!(loaded.canon_hash, cooked.canon_hash);
        let ids: Vec<_> = loaded.cas.iter().map(|(id, _)| id).collect();
        let orig: Vec<_> = cooked.cas.iter().map(|(id, _)| id).collect();
        assert_eq!(ids, orig);

        let kernel = load_warp(&path).unwrap();
        assert_eq!(kernel.world().canon_hash(), cooked.canon_hash);
        assert!(kernel.canon().pin("player").is_some());
        assert!(kernel.canon().rite_id("lockpick").is_some());

        let _ = fs::remove_file(&path);
    }
}
