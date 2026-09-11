//! PR 12b visual harness. Door open, barrel carried, one fire, HUD owed mass.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use klotho_commit::CommitKernel;
use klotho_compile::{Cas, cook_doc, validate_hull};
use klotho_core::{IVec3, Mm, PoseMm, Sigil, YawMd};
use klotho_ir::{Rel, from_ron};
use klotho_manifest::{
    GpuBudget, LightKind, LightStub, MaterialTag, Observer, UiManifest, VisualManifest, Widget,
    WidgetKind,
};
use klotho_render::{
    Presenter, VisualBind, WgpuPresenter, binds_from_cooked, extract_visual, overlay_hud, write_bmp,
};
use klotho_world::World;

use crate::{apply_seed, hearth_doc, pin, replay};

/// One Appendix A pixel golden.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum PixelScene {
    /// After lockpick: door yaw 90°.
    DoorOpen,
    /// Barrel_0 WieldedBy player, posed in arms.
    BarrelCarried,
    /// One ignited barrel, emissive + point light.
    OneFire,
    /// Trade accept: HUD shows owed copper / mass.
    HudOwed,
}

impl PixelScene {
    /// All four goldens, in HLD order.
    pub const ALL: [Self; 4] = [
        Self::DoorOpen,
        Self::BarrelCarried,
        Self::OneFire,
        Self::HudOwed,
    ];

    /// File stem for BMP goldens.
    #[must_use]
    pub const fn stem(self) -> &'static str {
        match self {
            Self::DoorOpen => "door_open",
            Self::BarrelCarried => "barrel_carried",
            Self::OneFire => "one_fire",
            Self::HudOwed => "hud_owed",
        }
    }
}

/// Cooked Hearth with kitbash hulls/meshes bound.
pub struct VisualHearth {
    /// Kernel after seed + layout.
    pub kernel: CommitKernel,
    /// CAS (meshes).
    pub cas: Cas,
    /// Sigil → mesh/material.
    pub binds: BTreeMap<Sigil, VisualBind>,
}

/// 3/4 shop camera. Not first-person — goldens have to show the door.
#[must_use]
pub fn golden_camera() -> Observer {
    Observer {
        eye: PoseMm::new(Mm(-4200), Mm(3400), Mm(-4800), YawMd(41_000)),
        pitch_md: -28_000,
    }
}

/// Cook + seed + millimetre layout from the kitbash library.
#[must_use]
pub fn boot_visual() -> VisualHearth {
    let doc = hearth_doc();
    let cooked = cook_doc(&doc).expect("kitbash cook");
    let klotho_compile::Cooked {
        canon,
        cas,
        bindings,
        canon_hash,
        ..
    } = cooked;
    let mut kernel = CommitKernel::new(World::new(Arc::new(canon), canon_hash));
    apply_seed(&mut kernel, &doc);
    apply_layout(&mut kernel, &cas, &bindings);
    let player = kernel.canon().pin("player").expect("player pin");
    kernel.bind_player(klotho_core::PlayerId(0), player);
    let pin = |n: &str| kernel.canon().pin(n);
    let binds = binds_from_cooked(pin, &bindings);
    VisualHearth { kernel, cas, binds }
}

fn apply_layout(k: &mut CommitKernel, cas: &Cas, bindings: &[klotho_compile::Binding]) {
    for b in bindings {
        let s = pin(k, b.locus.as_str());
        if let Some(bytes) = cas.get(b.hull) {
            if let Ok(aabb) = validate_hull(bytes) {
                k.world_mut().set_hull(s, aabb, b.hull).expect("hull");
            }
        }
        k.world_mut()
            .set_pose(s, layout_pose(b.locus.as_str()))
            .expect("pose");
    }
}

fn layout_pose(name: &str) -> PoseMm {
    match name {
        "hearth" => PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd::ZERO),
        "player" => PoseMm::new(Mm(-400), Mm(0), Mm(-2200), YawMd::ZERO),
        "oak_door" => PoseMm::new(Mm(0), Mm(0), Mm(2800), YawMd::ZERO),
        "bran" => PoseMm::new(Mm(1600), Mm(0), Mm(1400), YawMd(270_000)),
        "mira" => PoseMm::new(Mm(900), Mm(0), Mm(900), YawMd(45_000)),
        "kel" => PoseMm::new(Mm(-1400), Mm(0), Mm(1600), YawMd(90_000)),
        "fathers_hammer" => PoseMm::new(Mm(1500), Mm(0), Mm(1100), YawMd::ZERO),
        "iron_key" => PoseMm::new(Mm(-600), Mm(400), Mm(400), YawMd::ZERO),
        "lockpick_tool" => PoseMm::new(Mm(-200), Mm(900), Mm(-2000), YawMd::ZERO),
        "ingot" => PoseMm::new(Mm(400), Mm(0), Mm(200), YawMd::ZERO),
        "bucket" => PoseMm::new(Mm(-900), Mm(0), Mm(200), YawMd::ZERO),
        n if n.starts_with("barrel_") => {
            let i: i32 = n.trim_start_matches("barrel_").parse().unwrap_or(0);
            PoseMm::new(Mm(-2000 + i * 500), Mm(0), Mm(500), YawMd::ZERO)
        }
        _ => PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd::ZERO),
    }
}

/// Stage a pixel golden: replay, presentation tweaks, extract vis + HUD.
#[must_use]
pub fn stage(scene: PixelScene) -> (VisualHearth, VisualManifest, UiManifest, Observer) {
    let mut h = boot_visual();
    match scene {
        PixelScene::DoorOpen => {
            replay(
                &mut h.kernel,
                &intents(include_str!("../fixtures/golden_01_lockpick.ron")),
            );
            let door = pin(&h.kernel, "oak_door");
            let mut p = h.kernel.world().view().pose(door).unwrap_or_default();
            p.yaw = YawMd(YawMd::QUARTER_TURN);
            h.kernel.world_mut().set_pose(door, p).expect("door yaw");
        }
        PixelScene::BarrelCarried => {
            replay(
                &mut h.kernel,
                &intents(include_str!("../fixtures/golden_03_carry.ron")),
            );
            let player = pin(&h.kernel, "player");
            let barrel = pin(&h.kernel, "barrel_0");
            let pp = h.kernel.world().view().pose(player).unwrap_or_default();
            h.kernel
                .world_mut()
                .set_pose(
                    barrel,
                    PoseMm::new(pp.x, Mm(pp.y.0 + 900), Mm(pp.z.0 + 350), pp.yaw),
                )
                .expect("carry pose");
        }
        PixelScene::OneFire => {
            let one = from_ron::<Vec<klotho_ir::PlayerIntent>>(
                r#"[PlayerIntent(player: PlayerId(0), at: Tick(0), verb: Use, target: Name("barrel_0"), analog: Analog(phase: 0, stick_x: 0, stick_z: 0, look_yaw: YawMd(0), look_pitch: 0), agency: Agency(claimed: [], assist: None))]"#,
            )
            .expect("one fire");
            replay(&mut h.kernel, &one);
        }
        PixelScene::HudOwed => {
            replay(
                &mut h.kernel,
                &intents(include_str!("../fixtures/golden_06a_trade_accept.ron")),
            );
        }
    }
    let snap = h.kernel.snapshot();
    let mut binds = h.binds.clone();
    let heat = h.kernel.canon().resource_id("heat");
    let mut lights = Vec::new();
    if let Some(heat) = heat {
        for (s, b) in binds.iter_mut() {
            if snap.view().qty(*s, heat) >= 400 {
                b.material.tag = MaterialTag::Emissive;
                if let Some(p) = snap.view().pose(*s) {
                    lights.push(LightStub {
                        pos: IVec3 {
                            x: p.x.0,
                            y: p.y.0 + 900,
                            z: p.z.0,
                        },
                        kind: LightKind::Point {
                            intensity_milli: 1400,
                        },
                    });
                }
            }
        }
    }
    let mut vis = extract_visual(&snap, &binds, false);
    vis.lights = lights;
    let ui = extract_hud(&h, &snap);
    (h, vis, ui, golden_camera())
}

fn extract_hud(h: &VisualHearth, snap: &klotho_world::WorldSnapshot) -> UiManifest {
    let view = snap.view();
    let player = pin(&h.kernel, "player");
    let mut widgets = Vec::new();
    let copper = h.kernel.canon().resource_id("copper");
    let mass = h.kernel.canon().resource_id("mass_g");
    let stamina = h.kernel.canon().resource_id("stamina");
    let mut owed = 0;
    for s in view.loci() {
        if view.has_rel(s, Rel::Owes, player) {
            owed += copper.map(|c| view.qty(s, c)).unwrap_or(0);
        }
    }
    if owed > 0 {
        widgets.push(Widget {
            kind: WidgetKind::Text,
            body: format!("owed {owed} copper"),
        });
    }
    if let Some(st) = stamina {
        widgets.push(Widget {
            kind: WidgetKind::Bar {
                value: view.qty(player, st),
                cap: 100,
            },
            body: "stamina".into(),
        });
    }
    if let Some(m) = mass {
        let mut carried = 0;
        for s in view.loci() {
            if view.has_rel(s, Rel::WieldedBy, player) {
                carried += view.qty(s, m);
            }
        }
        if carried > 0 {
            widgets.push(Widget {
                kind: WidgetKind::Text,
                body: format!("mass {carried}g"),
            });
        }
    }
    if widgets.is_empty() {
        widgets.push(Widget {
            kind: WidgetKind::Text,
            body: "hearth".into(),
        });
    }
    UiManifest::from_widgets(snap.epoch, widgets)
}

fn intents(src: &str) -> Vec<klotho_ir::PlayerIntent> {
    from_ron(src).expect("PlayerIntent RON")
}

/// Render a scene offscreen, overlay HUD, write a BMP. `None` if no GPU.
pub fn write_scene_bmp(scene: PixelScene, path: &Path) -> Option<(u32, u32, usize)> {
    let (h, vis, ui, observer) = stage(scene);
    let mut gpu = WgpuPresenter::try_headless()?;
    gpu.upload_cas(&vis, &h.cas);
    gpu.present(&vis, observer, GpuBudget::HEARTH);
    let (w, ht) = gpu.size();
    let mut rgba = gpu.read_rgba()?;
    overlay_hud(&mut rgba, w, ht, &ui);
    write_bmp(path, w, ht, &rgba).ok()?;
    Some((w, ht, vis.clusters.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use klotho_ir::Rel;

    #[test]
    fn door_open_yaws_quarter_turn() {
        let (h, vis, _, _) = stage(PixelScene::DoorOpen);
        let door = pin(&h.kernel, "oak_door");
        assert!(!h.kernel.world().view().has_rel(door, Rel::LockedBy, door));
        assert_eq!(
            h.kernel.world().view().pose(door).unwrap().yaw,
            YawMd(YawMd::QUARTER_TURN)
        );
        assert!(vis.clusters.len() >= 8);
    }

    #[test]
    fn barrel_carried_is_wielded() {
        let (h, _, ui, _) = stage(PixelScene::BarrelCarried);
        let player = pin(&h.kernel, "player");
        let barrel = pin(&h.kernel, "barrel_0");
        assert!(
            h.kernel
                .world()
                .view()
                .has_rel(barrel, Rel::WieldedBy, player)
        );
        assert!(ui.widgets.iter().any(|w| w.body.contains("mass")));
    }

    #[test]
    fn one_fire_is_emissive() {
        let (h, vis, _, _) = stage(PixelScene::OneFire);
        let heat = h.kernel.canon().resource_id("heat").unwrap();
        let barrel = pin(&h.kernel, "barrel_0");
        assert!(h.kernel.world().view().qty(barrel, heat) >= 400);
        assert!(vis.materials.iter().any(|m| m.tag == MaterialTag::Emissive));
        assert!(!vis.lights.is_empty());
    }

    #[test]
    fn hud_owed_names_copper() {
        let (h, _, ui, _) = stage(PixelScene::HudOwed);
        let player = pin(&h.kernel, "player");
        let ingot = pin(&h.kernel, "ingot");
        assert!(h.kernel.world().view().has_rel(ingot, Rel::Owes, player));
        assert!(ui.widgets.iter().any(|w| w.body.contains("owed")));
    }
}
