//! Snapshot + CAS binds → [`VisualManifest`]. Extract is ≤ 1.5 ms budget (K14).

use std::collections::BTreeMap;

use klotho_anim::{ClipSet, capped_joints, clip_grounded, sample_joints};
use klotho_compile::Binding;
use klotho_core::{BlobId, Sigil, Vel3};
use klotho_manifest::{
    GpuHandle, InstancePass, MaterialRef, PaletteSlot, SkinnedInstance, VisualManifest,
};
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
///
/// Skinned binds with no [`ClipSet`] keep an identity palette (T-pose).
#[must_use]
pub fn extract_visual(
    snap: &WorldSnapshot,
    binds: &BTreeMap<Sigil, VisualBind>,
    debug: bool,
) -> VisualManifest {
    extract_visual_with_clips(snap, binds, None, debug)
}

/// Like [`extract_visual`], sampling joint palettes from `clips` when present.
///
/// Silent drops (no instance is pushed):
/// - missing pose
/// - `clips` is `Some` but lookup returns `None` (empty table)
/// - palette index would overflow `u16`
#[must_use]
pub fn extract_visual_with_clips(
    snap: &WorldSnapshot,
    binds: &BTreeMap<Sigil, VisualBind>,
    clips: Option<&ClipSet>,
    debug: bool,
) -> VisualManifest {
    extract_visual_with_clips_between(snap, None, binds, clips, debug)
}

/// Character locomotion uses displacement between admitted snapshots, rather
/// than the still-held Move request. A blocked root selects idle foot phase.
/// Without a matching prior snapshot a driven character presents idle; it
/// still uses the admitted pose. Legacy actors retain request-based sampling.
#[must_use]
pub fn extract_visual_with_clips_between(
    snap: &WorldSnapshot,
    previous: Option<&WorldSnapshot>,
    binds: &BTreeMap<Sigil, VisualBind>,
    clips: Option<&ClipSet>,
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
                let driven = view.character_physics(*s).is_some();
                let vel = if driven {
                    previous
                        .filter(|p| {
                            p.canon_hash == snap.canon_hash
                                && p.epoch == snap.epoch
                                && p.tick < snap.tick
                        })
                        .and_then(|p| p.view().pose(*s))
                        .map_or(Vel3::ZERO, |p| {
                            character_velocity(p, pose, view.vel(*s).map_or(Vel3::ZERO, |(v, _)| v))
                        })
                } else {
                    view.vel(*s).map(|(v, _)| v).unwrap_or(Vel3::ZERO)
                };
                let grounded = if driven {
                    view.support(*s).is_some()
                } else {
                    clip_grounded(view.support(*s).is_some(), pose.y.0)
                };
                if let Some((inst, slot)) =
                    skinned_instance(bind, pose, snap.tick, vel, grounded, clips, palettes.len())
                {
                    palettes.push(slot);
                    skinned.push(inst);
                }
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

/// Palette + instance for a skinned bind. `None` is a documented silent drop.
#[must_use]
pub fn skinned_instance(
    bind: &VisualBind,
    pose: klotho_core::PoseMm,
    tick: klotho_core::Tick,
    vel: Vel3,
    grounded: bool,
    clips: Option<&ClipSet>,
    palette_len: usize,
) -> Option<(SkinnedInstance, PaletteSlot)> {
    let Ok(palette) = u16::try_from(palette_len) else {
        return None;
    };
    let slot = match clips {
        None => PaletteSlot::identity(),
        Some(set) => {
            let locals = sample_joints(set, vel, grounded, tick)?;
            let joints = capped_joints(locals);
            let bones = u16::try_from(joints.len()).unwrap_or(u16::MAX);
            PaletteSlot {
                gpu: GpuHandle::NONE,
                bones,
                joints,
            }
        }
    };
    Some((
        SkinnedInstance {
            blob: bind.mesh,
            gpu: GpuHandle::NONE,
            pose,
            palette,
            material: bind.material,
        },
        slot,
    ))
}

fn character_velocity(
    previous: klotho_core::PoseMm,
    admitted: klotho_core::PoseMm,
    command: Vel3,
) -> Vel3 {
    // An idle rider is carried by the platform without walking in place.
    if command.x == klotho_core::VelFx::ZERO && command.z == klotho_core::VelFx::ZERO {
        return Vel3::ZERO;
    }
    Vel3::new(
        klotho_core::VelFx::from_mm_per_tick(admitted.x.0.saturating_sub(previous.x.0).signum()),
        klotho_core::VelFx::ZERO,
        klotho_core::VelFx::from_mm_per_tick(admitted.z.0.saturating_sub(previous.z.0).signum()),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use klotho_anim::ClipSet;
    use klotho_canon::cook_diffs;
    use klotho_compile::Binding;
    use klotho_core::{BlobId, Hash, LocusKind, Mm, PoseMm, Sigil, Tick, Vel3, VelFx, YawMd};
    use klotho_ir::{CanonDiff, Name, from_ron};
    use klotho_manifest::{InstancePass, MaterialTag};
    use klotho_world::World;

    use super::*;

    fn bind(mesh: BlobId, pass: InstancePass) -> VisualBind {
        VisualBind {
            mesh,
            material: MaterialRef {
                tag: MaterialTag::Organic,
                palette: 0,
            },
            pass,
        }
    }

    fn empty_snap() -> World {
        let diffs: Vec<CanonDiff> = from_ron("[]").unwrap();
        let canon = cook_diffs(&diffs).unwrap();
        World::new(Arc::new(canon), Hash::ZERO)
    }

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

    #[test]
    fn missing_pose_drops_skinned() {
        let mut world = empty_snap();
        let snap = world.snapshot();
        let s = Sigil::pack(LocusKind::Actor, 0, 1).unwrap();
        let mut binds = BTreeMap::new();
        binds.insert(s, bind(BlobId::from_bytes([3; 32]), InstancePass::Skinned));
        let vis = extract_visual_with_clips(&snap, &binds, Some(&ClipSet::hearth()), false);
        assert!(vis.skinned.is_empty());
        assert!(vis.palettes.is_empty());
        assert!(vis.clusters.is_empty());
    }

    #[test]
    fn missing_clip_drops_skinned() {
        let pose = PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd::ZERO);
        let b = bind(BlobId::from_bytes([4; 32]), InstancePass::Skinned);
        assert!(
            skinned_instance(
                &b,
                pose,
                Tick::ZERO,
                Vel3::ZERO,
                true,
                Some(&ClipSet::default()),
                0
            )
            .is_none()
        );
        let (inst, slot) =
            skinned_instance(&b, pose, Tick::ZERO, Vel3::ZERO, true, None, 0).unwrap();
        assert_eq!(inst.palette, 0);
        assert_eq!(slot.bones, 0);
        assert!(slot.joints.is_empty());
    }

    #[test]
    fn palette_index_overflow_drops() {
        let pose = PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd::ZERO);
        let b = bind(BlobId::from_bytes([5; 32]), InstancePass::Skinned);
        assert!(
            skinned_instance(
                &b,
                pose,
                Tick::ZERO,
                Vel3::ZERO,
                true,
                None,
                u16::MAX as usize + 1
            )
            .is_none()
        );
    }

    #[test]
    fn clip_joints_fill_palette() {
        let pose = PoseMm::new(Mm(1), Mm(2), Mm(3), YawMd::ZERO);
        let joint = PoseMm::new(Mm(0), Mm(400), Mm(0), YawMd::ZERO);
        let mut clips = ClipSet::hearth();
        clips.clips[1].joints = vec![vec![joint]];
        let b = bind(BlobId::from_bytes([6; 32]), InstancePass::Skinned);
        let moving = Vel3::new(VelFx::ONE, VelFx::ZERO, VelFx::ZERO);
        let (inst, slot) =
            skinned_instance(&b, pose, Tick::ZERO, moving, true, Some(&clips), 0).unwrap();
        assert_eq!(inst.pose, pose);
        assert_eq!(slot.bones, 1);
        assert_eq!(slot.joints, vec![joint]);
        let idle = skinned_instance(&b, pose, Tick::ZERO, Vel3::ZERO, true, Some(&clips), 0)
            .unwrap()
            .1;
        assert_eq!(idle.bones, 0);
        assert!(idle.joints.is_empty());
    }

    #[test]
    fn blocked_character_selects_idle_feet_at_the_admitted_root() {
        let root = PoseMm::new(Mm(0), Mm(0), Mm(495), YawMd::ZERO);
        let command = Vel3::new(VelFx::ZERO, VelFx::ZERO, VelFx::from_mm_per_tick(20));
        let mut clips = ClipSet::hearth();
        clips.clips[1].joints = vec![vec![PoseMm::new(Mm(1), Mm(2), Mm(3), YawMd::ZERO)]];
        let binding = bind(BlobId::ZERO, InstancePass::Skinned);
        let (instance, stopped) = skinned_instance(
            &binding,
            root,
            Tick(80),
            character_velocity(root, root, command),
            true,
            Some(&clips),
            0,
        )
        .unwrap();
        assert_eq!(instance.pose, root);
        assert_eq!(stopped.bones, 0);
        let previous = PoseMm { z: Mm(475), ..root };
        let (_, walking) = skinned_instance(
            &binding,
            root,
            Tick(80),
            character_velocity(previous, root, command),
            true,
            Some(&clips),
            0,
        )
        .unwrap();
        assert_eq!(walking.bones, 1);
        assert_eq!(character_velocity(previous, root, Vel3::ZERO), Vel3::ZERO);
    }

    #[test]
    fn opaque_bind_is_unchanged() {
        let mut world = empty_snap();
        let snap = world.snapshot();
        let s = Sigil::pack(LocusKind::Relic, 0, 2).unwrap();
        let mut binds = BTreeMap::new();
        binds.insert(s, bind(BlobId::from_bytes([7; 32]), InstancePass::Opaque));
        let vis = extract_visual(&snap, &binds, false);
        assert!(vis.clusters.is_empty(), "missing pose still drops opaque");
        assert!(vis.skinned.is_empty());
    }
}
