//! PHYS-A07 bounded semantic cook gates.
use klotho_compile::*;
use klotho_core::{ContactTrack, Hash, LocusKind};
use klotho_ir::{IntentDoc, Name, ProvenanceId, SeedFact, StyleIntent, from_ron};
use klotho_prove::ArtifactKind;
fn track() -> ContactTrack {
    from_ron(include_str!("../fixtures/sword-contact.ron")).unwrap()
}
fn doc() -> IntentDoc {
    IntentDoc {
        style: StyleIntent {
            notes: String::new(),
            palettes: vec![],
            kitbash_tags: vec![],
        },
        canon_diffs: from_ron::<Vec<klotho_ir::CanonDiff>>(include_str!(
            "../../klotho-canon/fixtures/ember.ron"
        ))
        .unwrap()
        .into_iter()
        .filter(|d| matches!(d, klotho_ir::CanonDiff::AddRite(r) if r.id.as_str() == "melee"))
        .collect(),
        seed: vec![
            SeedFact::Locus {
                name: Name::from("swordsman"),
                kind: LocusKind::Actor,
            },
            SeedFact::ContactTrack {
                of: Name::from("swordsman"),
                track: track(),
            },
        ],
        minds: vec![],
        provenance: ProvenanceId(Hash::ZERO),
    }
}
#[test]
fn cook_pack_and_reconstruct_semantic_identity() {
    let d = doc();
    let c = cook_doc(&d).unwrap();
    let bytes = encode_contact_track(&track()).unwrap();
    assert_eq!(peek_kind(&bytes).unwrap(), ArtifactKind::ContactTrack);
    validate_blob(&bytes).unwrap();
    assert_eq!(decode_contact_track(&bytes).unwrap(), track());
    assert!(c.cas.iter().any(|(_, b)| b == bytes));
    let restored = unpack_warp(&pack_warp(&c).unwrap()).unwrap();
    assert_eq!(restored.canon_hash, c.canon_hash);
    assert_eq!(
        restored.canon.contact_tracks.values().next(),
        Some(&track())
    );
    let mut changed = d.clone();
    if let SeedFact::ContactTrack { track, .. } = &mut changed.seed[1] {
        track.sweeps[0].samples[1][1].z += 1;
    }
    assert_ne!(cook_doc(&changed).unwrap().canon_hash, c.canon_hash);
}
#[test]
fn compatibility_checks_rig_timing_root_and_retarget_envelope() {
    let t = track();
    let sig = contact_signature(&t).unwrap();
    for hz in [60, 120, 144] {
        for _ in 0..hz {
            assert_eq!(certify_contact_clip(&t, &t, sig).unwrap(), sig);
        }
    }
    let mut v = t.clone();
    v.sockets[0].samples[1].x += 5;
    assert_eq!(certify_contact_clip(&t, &v, sig).unwrap(), sig);
    v.sockets[0].samples[1].y += 1;
    assert!(certify_contact_clip(&t, &v, sig).is_err());
    for field in 0..5 {
        let mut v = t.clone();
        match field {
            0 => v.instrument = Hash([2; 32]),
            1 => v.skeleton = Hash([2; 32]),
            2 => v.roots[1].x = 1,
            3 => v.wait_pc = 3,
            _ => v.tick_hz = 60,
        };
        assert!(certify_contact_clip(&t, &v, sig).is_err());
    }
    assert!(certify_contact_clip(&t, &t, Hash::ZERO).is_err());
}
#[test]
fn malformed_noncanonical_and_wrong_rite_bindings_fail_closed() {
    let b = encode_contact_track(&track()).unwrap();
    for end in 0..b.len() {
        assert!(decode_contact_track(&b[..end]).is_err());
    }
    let mut b = b;
    b.push(b' ');
    assert!(decode_contact_track(&b).is_err());
    let mut t = track();
    t.sockets.push(t.sockets[0].clone());
    assert!(encode_contact_track(&t).is_err());
    let mut t = track();
    t.sweeps[0].samples[1][1].x = i32::MAX;
    assert!(encode_contact_track(&t).is_err());
    for pc in [0, 2, 99] {
        let mut d = doc();
        if let SeedFact::ContactTrack { track, .. } = &mut d.seed[1] {
            track.wait_pc = pc;
        }
        assert!(cook_doc(&d).is_err());
    }
    let mut d = doc();
    d.seed.push(d.seed[1].clone());
    assert!(cook_doc(&d).is_err());
}

#[test]
fn changed_authoritative_track_requires_an_explicit_epoch_pack() {
    let base = cook_doc(&doc()).unwrap();
    let mut changed = track();
    changed.sweeps[0].samples[1][1].z += 20;
    let pack = cook_contact_epoch_pack(&base, klotho_core::Epoch(7), "swordsman", changed.clone())
        .unwrap();
    assert_eq!(pack.epoch, klotho_core::Epoch(8));
    assert_eq!(pack.from_canon_hash, base.canon_hash);
    assert_ne!(pack.canon_hash, base.canon_hash);
    assert_eq!(pack.canon.contact_tracks.values().next(), Some(&changed));
    let other =
        cook_contact_epoch_pack(&base, klotho_core::Epoch(7), "swordsman", track()).unwrap();
    assert_ne!(pack.canon_hash, other.canon_hash);
    assert!(
        cook_contact_epoch_pack(
            &base,
            klotho_core::Epoch(u32::MAX as u64),
            "missing",
            changed
        )
        .is_err()
    );
}

#[test]
fn channel_and_foot_plant_topology_are_canonical() {
    let mut t = track();
    t.plants.push(klotho_core::FootPlant {
        socket: "weapon_grip".into(),
        start: 0,
        end: 1,
    });
    assert!(encode_contact_track(&t).is_ok());
    t.plants.push(klotho_core::FootPlant {
        socket: "weapon_grip".into(),
        start: 0,
        end: 2,
    });
    assert!(encode_contact_track(&t).is_err());
    let mut d = doc();
    if let SeedFact::ContactTrack { track, .. } = &mut d.seed[1] {
        track.channel = 1;
    }
    assert!(cook_doc(&d).is_err());
    let mut t = track();
    t.roots.resize(66, klotho_core::IVec3::ZERO);
    t.wait_ticks = 65;
    assert!(encode_contact_track(&t).is_err());
}
