//! KAI-14: hierarchical world assembly and compact materialization.

use std::collections::BTreeSet;

use klotho_compile::{
    RawPlacement, cook_doc, decode_chunk, diff_chunks, materialize, unique_blob_count,
    write_catalog, write_placement_chunks,
};
use klotho_core::{Hash, LocusKind};
use klotho_eval::{
    JourneyHost, RouteHost, critical_path_journey, human_critical_path_journey, reachability,
    run_journey,
};
use klotho_ir::{IntentDoc, Name, ProvenanceId, SeedFact, StyleIntent};
use klotho_pattern::{
    DressingInstance, expand_world_plan, greybox_dressing, greybox_route, regenerate_dressing,
};
use klotho_prove::blob_id_of;
use klotho_world::{PlaceRow, PlaceSnap};

use crate::{review_world, world_graph};

fn raw(d: &DressingInstance) -> RawPlacement {
    RawPlacement {
        place: d.place.clone(),
        zone: d.zone.clone(),
        mesh: d.mesh,
        material: d.material,
        clip: d.clip,
        variant: d.variant,
        pose: d.pose,
        yaw: d.yaw,
        scale_permille: d.scale_permille,
    }
}

fn assets_from(dressing: &[DressingInstance]) -> Vec<(klotho_core::BlobId, Vec<u8>)> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for tag in [
        b"greybox-tree".as_slice(),
        b"greybox-rock".as_slice(),
        b"greybox-mat".as_slice(),
        b"extra".as_slice(),
    ] {
        let id = blob_id_of(tag);
        if seen.insert(id)
            && dressing
                .iter()
                .any(|d| d.mesh == id || d.material == id || d.clip == Some(id))
        {
            out.push((id, tag.to_vec()));
        }
    }
    out
}

#[test]
fn greybox_graph_shows_eight_places_and_critical_path() {
    let plan = greybox_route();
    let dressing = greybox_dressing(&plan);
    let (solved, view) = review_world(&plan, &dressing).unwrap();
    assert_eq!(view.places.len(), 8);
    assert_eq!(view.critical_path.len(), 7);
    assert!(view.places.iter().all(|p| p.budget_ok && p.protected == 1));
    assert!(view.to_string().contains("hub (hub)"));
    assert!(solved.culled.is_empty());
    let graph = world_graph(&plan, &solved);
    assert_eq!(graph.edges.len(), plan.edges.len());
}

#[test]
fn regeneration_keeps_protected_anchors_and_does_not_generate_at_runtime() {
    let base = greybox_route();
    let mut proposed = greybox_dressing(&base);
    proposed[1].pose.x = 9_001;
    let (plan, solved) = regenerate_dressing(&base, &proposed).unwrap();
    assert_eq!(plan.protected, base.protected);
    assert_eq!(plan.critical_path, base.critical_path);
    let world = materialize(
        &solved.instances.iter().map(raw).collect::<Vec<_>>(),
        &assets_from(&solved.instances),
    )
    .unwrap();
    assert!(
        world
            .chunks
            .iter()
            .all(|c| !c.bytes.windows(4).any(|w| w == b"SEED"))
    );
    let decoded = decode_chunk(&world.chunks[0].bytes).unwrap();
    assert_eq!(decoded.records, world.chunks[0].records);
}

#[test]
fn duplicate_source_blobs_are_not_packaged_and_one_edit_is_local() {
    let plan = greybox_route();
    let dressing = greybox_dressing(&plan);
    let assets = assets_from(&dressing);
    let prev = materialize(&dressing.iter().map(raw).collect::<Vec<_>>(), &assets).unwrap();
    assert_eq!(unique_blob_count(&prev), 3, "tree, rock, mat");
    let mut next_dressing = dressing.clone();
    next_dressing
        .iter_mut()
        .find(|d| d.place.as_str() == "combat")
        .unwrap()
        .pose
        .x += 500;
    let next = materialize(&next_dressing.iter().map(raw).collect::<Vec<_>>(), &assets).unwrap();
    let (dirty, reused) = diff_chunks(&prev.chunks, &next.chunks);
    assert_eq!(dirty, vec![(Name::from("combat"), Name::from("dress"))]);
    assert_eq!(reused.len(), 7);
}

#[test]
fn eight_place_route_streams_and_completes_scripted_and_human_journeys() {
    let plan = greybox_route();
    plan.validate().unwrap();
    let report = reachability(&plan).unwrap();
    assert!(report.critical_complete);
    assert!(report.optional_reachable);
    assert_eq!(report.reachable.len(), 8);

    let change = Hash::from_bytes([14; 32]);
    let mut scripted = RouteHost::from_plan(&plan).unwrap();
    run_journey(&mut scripted, &critical_path_journey(&plan), change).unwrap();
    let mut human = RouteHost::from_plan(&plan).unwrap();
    run_journey(&mut human, &human_critical_path_journey(&plan), change).unwrap();
    assert_eq!(scripted.last_state(), human.last_state());

    let doc = IntentDoc {
        style: StyleIntent::default(),
        canon_diffs: Vec::new(),
        seed: plan
            .places
            .iter()
            .map(|p| SeedFact::Locus {
                name: p.name.clone(),
                kind: LocusKind::Place,
            })
            .collect(),
        minds: Vec::new(),
        provenance: ProvenanceId(Hash::ZERO),
    };
    let cooked = cook_doc(&doc).unwrap();
    let dir = std::env::temp_dir().join(format!("klotho-kai14-{}", std::process::id()));
    let mut snaps = Vec::new();
    for place in &plan.places {
        let sigil = klotho_compile::place_sigil(&place.name);
        snaps.push((
            PlaceSnap::new(
                sigil,
                cooked.canon_hash,
                Hash::from_bytes([1; 32]),
                vec![PlaceRow::new(sigil, LocusKind::Place)],
            ),
            place.envelope,
        ));
    }
    let man = write_catalog(&dir, &cooked, &snaps).unwrap();
    assert_eq!(man.places.len(), 8);
    for place in &plan.places {
        let sigil = klotho_compile::place_sigil(&place.name);
        assert!(
            man.places.iter().any(|p| p.place == sigil),
            "missing {}",
            place.name
        );
    }

    let dressing = greybox_dressing(&plan);
    let world = materialize(
        &dressing.iter().map(raw).collect::<Vec<_>>(),
        &assets_from(&dressing),
    )
    .unwrap();
    let written = write_placement_chunks(&dir, &world.chunks).unwrap();
    assert_eq!(written.dirty.len(), 8);
    let written_again = write_placement_chunks(&dir, &world.chunks).unwrap();
    assert!(written_again.dirty.is_empty());
    assert_eq!(written_again.reused.len(), 8);

    let module = plan.anchor.child(b"module");
    let instances = expand_world_plan(&plan, module).unwrap();
    assert!(
        instances
            .iter()
            .any(|i| i.pattern.as_str() == "world.safe_hub")
    );
    let _ = std::fs::remove_dir_all(&dir);
}
