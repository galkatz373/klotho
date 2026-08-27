//! Snapshot + CAS binds → [`VisualManifest`]. Extract is ≤ 1.5 ms budget (K14).

use std::collections::BTreeMap;

use klotho_compile::Binding;
use klotho_core::{BlobId, Sigil};
use klotho_manifest::{InstancePass, MaterialRef, PaletteSlot, SkinnedInstance, VisualManifest};
use klotho_world::WorldSnapshot;

/// Mesh + material for a locus. Presentation table, not a World column.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct VisualBind {
    /// Clustered-mesh CAS id.
    pub mesh: BlobId,
    /// Closed material.
    pub material: MaterialRef,
    /// Which instance list to fill. Hearth kitbash is [`InstancePass::Opaque`].
    pub pass: InstancePass,
}

/// Pin cook bindings onto seed Sigils.
#[must_use]
pub fn binds_from_cooked(
    canon_pin: impl Fn(&str) -> Option<Sigil>,
    bindings: &[Binding],
) -> BTreeMap<Sigil, VisualBind> {
    let mut out = BTreeMap::new();
    for b in bindings {
        if let Some(s) = canon_pin(b.locus.as_str()) {
            out.insert(
                s,
                VisualBind {
                    mesh: b.mesh,
                    material: MaterialRef {
                        tag: b.material,
                        palette: 0,
                    },
                    pass: InstancePass::Opaque,
                },
            );
        }
    }
    out
}

/// Build a visual buffer from the published snapshot. No GPU, no Sigils on the
/// cluster hot path (debug_sigils only if `debug` is true).
#[must_use]
pub fn extract_visual(
    snap: &WorldSnapshot,
    binds: &BTreeMap<Sigil, VisualBind>,
    debug: bool,
) -> VisualManifest {
    let view = snap.view();
    let mut items = Vec::new();
    let mut debug_sigils = Vec::new();
    let mut masked = Vec::new();
    let mut skinned = Vec::new();
    let mut palettes = Vec::new();
    for (s, bind) in binds {
        let Some(pose) = view.pose(*s) else {
            continue;
        };
        match bind.pass {
            InstancePass::Opaque => items.push((bind.mesh, pose, bind.material)),
            InstancePass::Masked => masked.push((bind.mesh, pose, bind.material)),
            InstancePass::Skinned => {
                let Ok(palette) = u16::try_from(palettes.len()) else {
                    continue;
                };
                palettes.push(PaletteSlot {
                    gpu: klotho_manifest::GpuHandle::NONE,
                    bones: 0,
                });
                skinned.push(SkinnedInstance {
                    blob: bind.mesh,
                    gpu: klotho_manifest::GpuHandle::NONE,
                    pose,
                    palette,
                    material: bind.material,
                });
            }
        }
        if debug {
            if let Some(h) = view.posed_hull(*s) {
                debug_sigils.push((*s, h));
            }
        }
    }
    VisualManifest::from_v2(
        snap.epoch,
        snap.tick,
        items,
        masked,
        skinned,
        palettes,
        [],
        [],
        klotho_manifest::PostFlags::UNLIT,
        debug_sigils,
    )
}

#[cfg(test)]
mod tests {
    use klotho_compile::Binding;
    use klotho_core::{BlobId, LocusKind, Sigil};
    use klotho_ir::Name;
    use klotho_manifest::MaterialTag;

    use super::*;

    #[test]
    fn binds_map_pin_names() {
        let s = Sigil::pack(LocusKind::Relic, 0, 7).unwrap();
        let mesh = BlobId::from_bytes([2; 32]);
        let b = Binding {
            locus: Name::from("oak_door"),
            tag: Name::from("door.oak.lockable"),
            hull: BlobId::ZERO,
            mesh,
            material: MaterialTag::Organic,
        };
        let map = binds_from_cooked(
            |n| {
                if n == "oak_door" { Some(s) } else { None }
            },
            std::slice::from_ref(&b),
        );
        assert_eq!(map.get(&s).unwrap().mesh, mesh);
        assert!(binds_from_cooked(|_| None, std::slice::from_ref(&b)).is_empty());
    }
}
