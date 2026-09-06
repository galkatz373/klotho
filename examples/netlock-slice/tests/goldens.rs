//! Netlock goldens for the full AAA-23 slice contract.

use klotho_core::{Budget, Epoch, Hash, RejectReason, Tick};
use klotho_ir::Verb;
use klotho_net::{NetError, Packet, PoseBlock, SidecarFlag, SnapshotBlob, load_replay};
use netlock_slice::{INTENT_HZ, Netlock, NetlockError, PLAYER_COUNT, intent, off_ray, on_ray};

#[test]
fn golden_01_eight_player_dedicated_at_sixty_hz() {
    let mut netlock = Netlock::boot().unwrap();
    assert_eq!(netlock.server().player_count(), PLAYER_COUNT);
    assert_eq!(netlock.server().intent_hz(), INTENT_HZ);
    assert_eq!(netlock.server().sidecar().rewind_ticks(), 12);
    for index in 0..PLAYER_COUNT {
        assert_eq!(netlock.client(index).intent_hz(), INTENT_HZ);
        assert_eq!(netlock.client(index).player().unwrap().0 as usize, index);
        netlock.submit(index, intent(Verb::Look, Tick(0))).unwrap();
    }
    let frame = netlock.step().unwrap();
    assert_eq!(netlock.server().ingested().len(), PLAYER_COUNT);
    assert_eq!(frame.packets.len(), PLAYER_COUNT);
}

#[test]
fn golden_02_pose_delta_drives_only_the_overlay() {
    let mut netlock = Netlock::boot().unwrap();
    let dummy = netlock.dummy();
    let first = netlock.step().unwrap();
    assert!(
        first
            .packets
            .iter()
            .all(|packets| packets.iter().any(|p| matches!(
                p,
                Packet::PoseDelta {
                    block: PoseBlock::Full(_),
                    ..
                }
            )))
    );
    assert_eq!(netlock.client(0).overlay().pose(dummy), Some(on_ray()));
    let prefix = netlock.kernel().world().trace_prefix_hash();

    netlock.propose_pose(dummy, off_ray());
    let moved = netlock.step().unwrap();
    assert!(
        moved
            .packets
            .iter()
            .all(|packets| packets.iter().any(|p| matches!(
                p,
                Packet::PoseDelta { block: PoseBlock::Delta(d), .. } if !d.is_empty()
            )))
    );
    assert_eq!(netlock.client(0).overlay().pose(dummy), Some(off_ray()));
    assert_eq!(
        netlock.kernel().world().trace_prefix_hash(),
        prefix,
        "physics pose and overlay are not per-tick Trace pose events"
    );
}

#[test]
fn golden_03_bounded_lag_comp_hits_strafing_target() {
    let mut netlock = Netlock::boot().unwrap();
    let dummy = netlock.dummy();
    let health = netlock.kernel().canon().resource_id("health").unwrap();
    netlock.step().unwrap();
    let fire_at = netlock.kernel().world().tick();
    netlock.propose_pose(dummy, off_ray());
    netlock.step().unwrap();
    netlock.submit(0, intent(Verb::Fire, fire_at)).unwrap();
    let frame = netlock.step().unwrap();
    assert!(frame.delta.rejects.is_empty(), "{:?}", frame.delta);
    assert_eq!(netlock.kernel().world().view().qty(dummy, health), 75);
    assert_eq!(netlock.kernel().last_rewind_ticks_used(), 2);
    assert_eq!(netlock.kernel().world().view().pose(dummy), Some(off_ray()));
}

#[test]
fn golden_04_too_old_fire_nacks_without_damage() {
    let mut netlock = Netlock::boot().unwrap();
    let dummy = netlock.dummy();
    let health = netlock.kernel().canon().resource_id("health").unwrap();
    for _ in 0..=Budget::AAA_SHOOTER.rewind_ticks {
        netlock.step().unwrap();
    }
    netlock.submit(0, intent(Verb::Fire, Tick(0))).unwrap();
    assert_eq!(netlock.server().last_sidecar_flag(), SidecarFlag::StaleFire);
    let frame = netlock.step().unwrap();
    assert!(
        frame
            .delta
            .rejects
            .iter()
            .any(|(_, r)| *r == RejectReason::StaleEpoch)
    );
    assert!(frame.packets[0].iter().any(|p| matches!(
        p,
        Packet::Nack {
            reason: RejectReason::StaleEpoch,
            ..
        }
    )));
    assert_eq!(netlock.kernel().world().view().qty(dummy, health), 100);
}

#[test]
fn golden_05_desync_writes_consumed_intent_replay() {
    let mut netlock = Netlock::boot().unwrap();
    netlock.submit(0, intent(Verb::Look, Tick(0))).unwrap();
    netlock.step().unwrap();
    let bad = Packet::Snapshot {
        tick: netlock.kernel().world().tick(),
        epoch: Epoch::ZERO,
        canon_hash: Hash::ZERO,
        trace_prefix_hash: Hash::ZERO,
        place: None,
        blob: SnapshotBlob(Vec::new()),
    };
    assert!(matches!(
        netlock.apply_client_packet(0, bad),
        Err(NetlockError::Net(NetError::Desync))
    ));
    let path = std::env::temp_dir().join(format!(
        "klotho-netlock-desync-{}-{}.ron",
        std::process::id(),
        netlock.kernel().world().tick().0
    ));
    netlock.write_desync_replay(&path).unwrap();
    let replay = load_replay(&path).unwrap();
    assert_eq!(replay.intents, netlock.server().ingested());
    assert_eq!(replay.intents.len(), 1);
    let _ = std::fs::remove_file(path);
}

#[test]
fn golden_06_wire_and_trace_have_no_prediction_flag() {
    let source = include_str!("../src/lib.rs");
    assert!(!source.contains(concat!("Pred", "icted")));
}
