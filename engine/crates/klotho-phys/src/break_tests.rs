use std::sync::Arc;

use klotho_canon::cook_diffs;
use klotho_commit::{AdmitBuf, CommitKernel, SyncProposer};
use klotho_core::{AabbMm, BlobId, Budget, Hash, IVec3, LocusKind, Mm, PoseMm, Sigil, Tick, YawMd};
use klotho_ir::{CanonDiff, Rel, from_ron};
use klotho_trace::TraceBody;
use klotho_world::World;

use crate::Phys;

fn relic(n: u128) -> Sigil {
    Sigil::pack(LocusKind::Relic, 0, n).unwrap()
}
fn hull_id(n: u8) -> BlobId {
    BlobId::from_bytes([n; 32])
}
fn crate_hull() -> AabbMm {
    AabbMm::new(
        IVec3 {
            x: -200,
            y: 0,
            z: -200,
        },
        IVec3 {
            x: 200,
            y: 400,
            z: 200,
        },
    )
}
fn pose(x: i32, y: i32, z: i32) -> PoseMm {
    PoseMm::new(Mm(x), Mm(y), Mm(z), YawMd::ZERO)
}

fn floor_hull() -> AabbMm {
    AabbMm::new(
        IVec3 {
            x: -50_000,
            y: -200,
            z: -50_000,
        },
        IVec3 {
            x: 50_000,
            y: 0,
            z: 50_000,
        },
    )
}

fn boot(threshold: i32, fragments: u8, sep_mm: i32) -> CommitKernel {
    let a = relic(1);
    let b = relic(2);
    let cid = relic(9);
    let src = if fragments > 0 {
        r#"[AddAffordance(Affordance(id: "Fragment", requires: [], grants: [], conflicts: []))]"#
    } else {
        "[]"
    };
    let mut canon = cook_diffs(&from_ron::<Vec<CanonDiff>>(src).unwrap()).unwrap();
    assert!(canon.bind_constraint(
        cid,
        klotho_core::ConstraintPhysics {
            a,
            b,
            binding: hull_id(7),
            break_impulse: threshold,
            fragments,
            anchor_a: IVec3 {
                x: 200,
                y: 200,
                z: 0,
            },
            anchor_b: IVec3 {
                x: -200,
                y: 200,
                z: 0,
            },
            ..klotho_core::ConstraintPhysics::default()
        }
    ));
    let mut k = CommitKernel::new(World::new(Arc::new(canon), Hash::ZERO));
    let floor = Sigil::pack(LocusKind::Place, 0, 9).unwrap();
    {
        let mut w = k.world_mut();
        w.insert_locus(floor, LocusKind::Place).unwrap();
        w.set_hull(floor, floor_hull(), hull_id(9)).unwrap();
        w.set_pose(floor, pose(0, 0, 0)).unwrap();
        w.set_island(floor, 99, 0).unwrap();
    }
    for (s, x) in [(a, 0), (b, sep_mm)] {
        let mut w = k.world_mut();
        w.insert_locus(s, LocusKind::Relic).unwrap();
        w.set_hull(s, crate_hull(), hull_id(1)).unwrap();
        w.set_pose(s, pose(x, 400, 0)).unwrap();
        w.set_island(s, 99, 0).unwrap();
    }
    k.world_mut().add_rel(a, Rel::PartOf, b).unwrap();
    k
}

fn step(k: &mut CommitKernel) -> klotho_trace::TraceDelta {
    k.partition();
    k.step(Tick(1), Budget::AAA_ADVENTURE, &mut [&mut Phys])
        .unwrap()
}

fn fragment_count(k: &CommitKernel) -> usize {
    let Some(mark) = k.canon().affordance_id("Fragment") else {
        return 0;
    };
    k.world()
        .view()
        .loci()
        .filter(|&s| k.world().view().has_affordance(s, mark))
        .count()
}

#[test]
fn below_threshold_impulse_holds_part_of() {
    let mut k = boot(1_000_000, 0, 400);
    let d = step(&mut k);
    assert!(d.rejects.is_empty(), "{d:?}");
    assert!(k.world().view().has_rel(relic(1), Rel::PartOf, relic(2)));
    assert!(
        !k.world()
            .view()
            .constraint_state(relic(9))
            .is_some_and(|s| s.broken)
    );
    assert!(
        !d.events
            .iter()
            .any(|e| matches!(e.body, TraceBody::ConstraintBroken { .. }))
    );
}

#[test]
fn above_threshold_impulse_produces_one_semantic_break() {
    let mut k = boot(1, 0, 8_000);
    let d = step(&mut k);
    assert!(d.rejects.is_empty(), "{d:?}");
    assert!(!k.world().view().has_rel(relic(1), Rel::PartOf, relic(2)));
    let state = k.world().view().constraint_state(relic(9)).unwrap();
    assert!(state.broken);
    assert_eq!(
        d.events
            .iter()
            .filter(|e| matches!(e.body, TraceBody::ConstraintBroken { .. }))
            .count(),
        1
    );
}

#[test]
fn admitted_break_spawns_authoritative_fragments() {
    let mut k = boot(1, 3, 8_000);
    let before = k.world().view().loci().count();
    let d = step(&mut k);
    assert!(d.rejects.is_empty(), "{d:?}");
    assert_eq!(fragment_count(&k), 3);
    assert_eq!(k.world().view().loci().count(), before + 3);
    let mark = k.canon().affordance_id("Fragment").unwrap();
    for s in k.world().view().loci() {
        if k.world().view().has_affordance(s, mark) {
            assert!(k.world().view().hull(s).is_some());
        }
    }
    assert_eq!(
        d.events
            .iter()
            .filter(|e| matches!(e.body, TraceBody::Spawned { .. }))
            .count(),
        3
    );
}

#[test]
fn cosmetic_debris_is_trace_only() {
    let mut k = boot(1, 0, 8_000);
    let d = step(&mut k);
    assert!(d.rejects.is_empty(), "{d:?}");
    assert!(
        d.events
            .iter()
            .any(|e| matches!(e.body, TraceBody::ConstraintBroken { fragments: 0, .. }))
    );
    assert_eq!(
        k.world()
            .view()
            .loci()
            .filter(|s| s.kind() == Some(LocusKind::Relic))
            .count(),
        2
    );
    assert_eq!(fragment_count(&k), 0);
}

#[test]
fn save_load_after_break_restores_relations_and_fragments() {
    let mut k = boot(1, 3, 8_000);
    let d = step(&mut k);
    assert!(d.rejects.is_empty(), "{d:?}");
    let saved = klotho_save::pause_save(&k.snapshot()).unwrap();
    let loaded = klotho_save::decode(&klotho_save::encode(&saved).unwrap()).unwrap();
    let restored = klotho_save::restore(&loaded, saved.prefix, saved.canon_hash).unwrap();
    assert_eq!(
        restored.view().constraint_state(relic(9)),
        k.world().view().constraint_state(relic(9))
    );
    assert!(!restored.view().has_rel(relic(1), Rel::PartOf, relic(2)));
    let mark = k.canon().affordance_id("Fragment").unwrap();
    let live: Vec<_> = k
        .world()
        .view()
        .loci()
        .filter(|&s| k.world().view().has_affordance(s, mark))
        .collect();
    let back: Vec<_> = restored
        .view()
        .loci()
        .filter(|&s| restored.view().has_affordance(s, mark))
        .collect();
    assert_eq!(live, back);
    assert_eq!(live.len(), 3);
    for s in &back {
        assert_eq!(restored.view().hull(*s), k.world().view().hull(*s));
        assert_eq!(restored.view().pose(*s), k.world().view().pose(*s));
    }
}

#[test]
fn one_and_eight_workers_preserve_break_trace_and_projection() {
    fn run(workers: usize) -> (Hash, Vec<u8>) {
        let mut k = boot(1, 0, 8_000);
        for n in 11..=18 {
            let s = relic(n);
            let mut w = k.world_mut();
            w.insert_locus(s, LocusKind::Relic).unwrap();
            w.set_hull(s, crate_hull(), hull_id(1)).unwrap();
            w.set_pose(s, pose(n as i32 * 4_000, 400, 0)).unwrap();
            w.set_island(s, 99, 0).unwrap();
        }
        let islands = k.partition();
        assert!(islands.len() >= 8, "{islands:?}");
        struct Jobs(usize);
        impl SyncProposer for Jobs {
            fn name(&self) -> &'static str {
                "break-test-jobs"
            }
            fn propose(&mut self, view: &klotho_world::WorldView, _: Tick, out: &mut AdmitBuf) {
                let mut groups = std::collections::BTreeMap::<u16, Vec<Sigil>>::new();
                for s in view.loci() {
                    if let Some((island, _)) = view.island(s) {
                        if island != klotho_core::NO_ISLAND {
                            groups.entry(island).or_default().push(s);
                        }
                    }
                }
                let islands = groups.into_iter().collect::<Vec<_>>();
                for (p, _) in klotho_jobs::propose_islands(self.0, &islands, &[&Phys], view) {
                    out.push(p);
                }
            }
        }
        let d = k
            .step(Tick(1), Budget::AAA_ADVENTURE, &mut [&mut Jobs(workers)])
            .unwrap();
        assert!(d.rejects.is_empty(), "{d:?}");
        (
            k.world().trace_prefix_hash(),
            k.snapshot().encode().unwrap(),
        )
    }
    assert_eq!(run(1), run(8));
}
