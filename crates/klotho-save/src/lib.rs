//! Epoch snapshots and Trace suffix I/O.
//!
//! Pause save copies the published projection with an empty suffix. Automatic
//! epochs compact on a 30 s clock and refuse a suffix longer than 120 s.
//!
//! `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod blob;
mod codec;
mod epoch;
mod error;

pub use blob::{SaveBlob, check_load, load, pause_save};
pub use codec::{
    MAX_EVENT_BYTES, MAX_SUFFIX_EVENTS, SAVE_CAP, SAVE_MAGIC, SAVE_VERSION, assembled_size,
    check_assembled_size, decode, encode,
};
pub use epoch::{AUTOSAVE_SECS, EpochStore, SUFFIX_SECS, ticks_for_secs};
pub use error::SaveError;

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use klotho_canon::cook_diffs;
    use klotho_core::{
        Epoch, Hash, LocusKind, Mm, PoseMm, ResourceId, Sigil, Tick, Vel3, VelFx, YawMd,
    };
    use klotho_ir::{CanonDiff, Rel, from_ron};
    use klotho_trace::{TraceBody, TraceEvent, TraceLog, encode_event};
    use klotho_world::{SnapRow, World, WorldSnapshot};

    use super::*;

    fn relic(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Relic, 0, id).unwrap()
    }

    fn empty_snap() -> Arc<WorldSnapshot> {
        let d: Vec<CanonDiff> = from_ron("[]").unwrap();
        let canon = cook_diffs(&d).unwrap();
        let mut w = World::new(Arc::new(canon), Hash::ZERO);
        w.snapshot()
    }

    fn snap_at(tick: Tick, prefix: Hash) -> Arc<WorldSnapshot> {
        let row = SnapRow::new(relic(1), LocusKind::Relic);
        Arc::new(
            WorldSnapshot::from_snap_rows(Epoch::ZERO, tick, Hash::ZERO, prefix, None, vec![row])
                .unwrap(),
        )
    }

    fn qty_event(tick: Tick) -> TraceEvent {
        TraceEvent::new(
            tick,
            TraceBody::QtyChanged {
                id: relic(1),
                res: ResourceId(0),
                to: 1,
                quantum: 1,
            },
        )
    }

    fn header(canon: Hash, epoch: Epoch, prefix: Hash, tick: Tick, snap_len: u32) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&SAVE_MAGIC);
        b.push(SAVE_VERSION);
        b.extend_from_slice(&[0, 0, 0]);
        b.extend_from_slice(canon.as_bytes());
        b.extend_from_slice(&epoch.0.to_le_bytes());
        b.extend_from_slice(prefix.as_bytes());
        b.extend_from_slice(&tick.0.to_le_bytes());
        b.extend_from_slice(&snap_len.to_le_bytes());
        b
    }

    #[test]
    fn pause_save_empty_suffix_plumbs_epoch() {
        let snap = empty_snap();
        let blob = pause_save(&snap).unwrap();
        assert!(blob.suffix.is_empty());
        assert_eq!(blob.epoch, snap.epoch);
        assert_eq!(blob.canon_hash, snap.canon_hash);
        assert_eq!(blob.prefix, snap.trace_prefix_hash);
        assert_eq!(blob.trace_from_tick, snap.tick);
        assert!(Arc::ptr_eq(&blob.snap, &snap));
    }

    #[test]
    fn prefix_mismatch_refuses_load() {
        let snap = empty_snap();
        let blob = pause_save(&snap).unwrap();
        assert_eq!(
            check_load(&blob, Hash::from_bytes([1; 32]), None),
            Err(SaveError::PrefixMismatch)
        );
        assert_eq!(check_load(&blob, snap.trace_prefix_hash, None), Ok(()));
        let loaded = load(blob, snap.trace_prefix_hash, None).unwrap();
        assert_eq!(loaded.prefix, snap.trace_prefix_hash);
    }

    #[test]
    fn canon_mismatch_refuses_load() {
        let snap = empty_snap();
        let blob = pause_save(&snap).unwrap();
        assert_eq!(
            check_load(
                &blob,
                snap.trace_prefix_hash,
                Some(Hash::from_bytes([1; 32]))
            ),
            Err(SaveError::CanonMismatch)
        );
        assert_eq!(
            check_load(&blob, snap.trace_prefix_hash, Some(snap.canon_hash)),
            Ok(())
        );
    }

    #[test]
    fn encode_decode_round_trips_pose_columns() {
        let mut pose = PoseMm::new(Mm(10), Mm(50), Mm(20), YawMd(30));
        pose.pitch = YawMd(1_000);
        pose.roll = YawMd(2_000);
        let s = relic(1);
        let mut row = SnapRow::new(s, LocusKind::Relic);
        row.pose = Some(pose);
        row.vel = Vel3::new(VelFx(1), VelFx(2), VelFx(3));
        row.yaw_rate = 4;
        row.pitch_rate = 5;
        row.roll_rate = 6;
        row.support = Some((0, 1, 0, 8));
        row.rels = vec![(Rel::LockedBy, s)];
        row.qty = vec![(ResourceId(1), 7)];
        row.knows = vec![3];
        let snap = Arc::new(
            WorldSnapshot::from_snap_rows(
                Epoch::ZERO,
                Tick(1),
                Hash::ZERO,
                Hash::ZERO,
                None,
                vec![row],
            )
            .unwrap(),
        );
        let blob = pause_save(&snap).unwrap();
        assert!(blob.suffix.is_empty());
        let bytes = encode(&blob).unwrap();
        let back = decode(&bytes).unwrap();
        let v = back.snap.view();
        assert_eq!(v.pose(s), Some(pose));
        assert_eq!(v.vel(s), Some((Vel3::new(VelFx(1), VelFx(2), VelFx(3)), 4)));
        assert_eq!(v.rates(s), Some((4, 5, 6)));
        assert_eq!(v.support(s), Some((0, 1, 0, 8)));
        assert!(v.has_rel(s, Rel::LockedBy, s));
        assert_eq!(v.qty(s, ResourceId(1)), 7);
        assert!(klotho_canon::PredStore::knows(&v, s, 3));
        assert!(back.suffix.is_empty());
    }

    #[test]
    fn declared_oversize_snap_len_refused_before_alloc() {
        let mut b = header(Hash::ZERO, Epoch::ZERO, Hash::ZERO, Tick::ZERO, u32::MAX);
        b.extend_from_slice(&[0u8; 8]);
        assert_eq!(
            decode(&b).unwrap_err(),
            SaveError::Oversize {
                size: u32::MAX as usize,
                cap: SAVE_CAP,
            }
        );
    }

    #[test]
    fn declared_oversize_suffix_len_refused_before_alloc() {
        let snap = empty_snap();
        let blob = pause_save(&snap).unwrap();
        let mut bytes = encode(&blob).unwrap();
        let snap_len = u32::from_le_bytes(bytes[88..92].try_into().unwrap()) as usize;
        let suffix_at = 92 + snap_len;
        bytes[suffix_at..suffix_at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            decode(&bytes).unwrap_err(),
            SaveError::Oversize {
                size: u32::MAX as usize,
                cap: MAX_SUFFIX_EVENTS,
            }
        );
    }

    #[test]
    fn declared_oversize_event_payload_refused_before_alloc() {
        let snap = empty_snap();
        let blob = pause_save(&snap).unwrap();
        let mut bytes = encode(&blob).unwrap();
        let snap_len = u32::from_le_bytes(bytes[88..92].try_into().unwrap()) as usize;
        let suffix_at = 92 + snap_len;
        bytes[suffix_at..suffix_at + 4].copy_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            decode(&bytes).unwrap_err(),
            SaveError::Oversize {
                size: u32::MAX as usize,
                cap: MAX_EVENT_BYTES,
            }
        );
    }

    #[test]
    fn trailing_bytes_refused() {
        let snap = empty_snap();
        let mut bytes = encode(&pause_save(&snap).unwrap()).unwrap();
        bytes.push(0);
        assert_eq!(decode(&bytes).unwrap_err(), SaveError::Trailing);
    }

    #[test]
    fn wrong_magic_refused() {
        let snap = empty_snap();
        let mut bytes = encode(&pause_save(&snap).unwrap()).unwrap();
        bytes[0] = b'X';
        assert_eq!(decode(&bytes).unwrap_err(), SaveError::Magic);
    }

    #[test]
    fn wrong_version_refused() {
        let snap = empty_snap();
        let mut bytes = encode(&pause_save(&snap).unwrap()).unwrap();
        bytes[4] = 9;
        assert_eq!(decode(&bytes).unwrap_err(), SaveError::Version(9));
    }

    #[test]
    fn wrong_pad_refused() {
        let snap = empty_snap();
        let mut bytes = encode(&pause_save(&snap).unwrap()).unwrap();
        bytes[5] = 1;
        assert_eq!(decode(&bytes).unwrap_err(), SaveError::Pad);
    }

    fn encode_suffix_tick(event_tick: Tick) -> SaveError {
        let snap = snap_at(Tick(10), Hash::ZERO);
        let mut blob = pause_save(&snap).unwrap();
        blob.suffix.push(qty_event(event_tick));
        encode(&blob).unwrap_err()
    }

    fn decode_suffix_tick(event_tick: Tick) -> SaveError {
        let snap = snap_at(Tick(10), Hash::ZERO);
        let mut bytes = encode(&pause_save(&snap).unwrap()).unwrap();
        let snap_len = u32::from_le_bytes(bytes[88..92].try_into().unwrap()) as usize;
        let suffix_at = 92 + snap_len;
        bytes[suffix_at..suffix_at + 4].copy_from_slice(&1u32.to_le_bytes());
        let ev = encode_event(&qty_event(event_tick));
        bytes.extend_from_slice(&(u32::try_from(ev.len()).unwrap()).to_le_bytes());
        bytes.extend_from_slice(&ev);
        decode(&bytes).unwrap_err()
    }

    #[test]
    fn suffix_event_before_snap_tick_is_tick_window() {
        assert_eq!(encode_suffix_tick(Tick(9)), SaveError::TickWindow);
        assert_eq!(decode_suffix_tick(Tick(9)), SaveError::TickWindow);
    }

    #[test]
    fn suffix_event_at_snap_tick_is_tick_window() {
        assert_eq!(encode_suffix_tick(Tick(10)), SaveError::TickWindow);
        assert_eq!(decode_suffix_tick(Tick(10)), SaveError::TickWindow);
    }

    #[test]
    fn encode_gate_uses_check_assembled_size() {
        assert_eq!(SAVE_CAP, 64 * 1024 * 1024);
        let size = assembled_size(SAVE_CAP, 1);
        assert_eq!(
            check_assembled_size(size),
            Err(SaveError::Oversize {
                size,
                cap: SAVE_CAP,
            })
        );
        let snap = empty_snap();
        let bytes = encode(&pause_save(&snap).unwrap()).unwrap();
        assert_eq!(check_assembled_size(bytes.len()), Ok(()));
    }

    #[test]
    fn automatic_30s_roll_empties_suffix() {
        let mut store = EpochStore::new();
        let genesis = klotho_trace::genesis_hash();
        store.on_publish(snap_at(Tick(0), genesis), &[], 60);
        store.on_publish(snap_at(Tick(1799), genesis), &[qty_event(Tick(1))], 60);
        let before = store.current().unwrap();
        assert_eq!(before.trace_from_tick, Tick(0));
        assert_eq!(before.suffix.len(), 1);
        assert_eq!(before.suffix[0].tick, Tick(1));
        store.on_publish(snap_at(Tick(1800), genesis), &[], 60);
        let after = store.current().unwrap();
        assert_eq!(after.trace_from_tick, Tick(1800));
        assert!(after.suffix.is_empty());
    }

    #[test]
    fn suffix_120s_forces_compact_without_dropping_chain() {
        let mut log = TraceLog::new();
        let e = qty_event(Tick(121));
        log.append(e.clone());
        let live = log.prefix_hash();
        let mut store = EpochStore::new();
        let genesis = klotho_trace::genesis_hash();
        store.on_publish(snap_at(Tick(0), genesis), &[], 1);
        store.on_publish(snap_at(Tick(20), genesis), std::slice::from_ref(&e), 1);
        let cur = store.current().unwrap();
        assert_eq!(cur.trace_from_tick, Tick(20));
        assert_eq!(cur.suffix.len(), 1);
        assert_eq!(cur.suffix[0].tick, Tick(121));
        let folded = TraceLog::replay_suffix(cur.prefix, &cur.suffix);
        assert_eq!(folded, live);
    }

    #[test]
    fn force_compact_current_snap_empties_suffix() {
        let mut log = TraceLog::new();
        let e = qty_event(Tick(121));
        log.append(e.clone());
        let live = log.prefix_hash();
        let mut store = EpochStore::new();
        let genesis = klotho_trace::genesis_hash();
        store.on_publish(snap_at(Tick(0), genesis), &[], 1);
        store.on_publish(snap_at(Tick(121), live), &[e], 1);
        let cur = store.current().unwrap();
        assert!(cur.suffix.is_empty());
        assert_eq!(cur.prefix, live);
        assert_eq!(cur.trace_from_tick, Tick(121));
    }

    #[test]
    fn events_at_or_before_epoch_tick_are_dropped() {
        let mut store = EpochStore::new();
        let genesis = klotho_trace::genesis_hash();
        store.on_publish(snap_at(Tick(10), genesis), &[], 60);
        store.on_publish(
            snap_at(Tick(10), genesis),
            &[qty_event(Tick(9)), qty_event(Tick(10)), qty_event(Tick(11))],
            60,
        );
        let cur = store.current().unwrap();
        assert_eq!(cur.suffix.len(), 1);
        assert_eq!(cur.suffix[0].tick, Tick(11));
    }

    #[test]
    fn pause_now_ignores_autosave_clock() {
        let mut store = EpochStore::new();
        let genesis = klotho_trace::genesis_hash();
        store.on_publish(snap_at(Tick(0), genesis), &[qty_event(Tick(1))], 60);
        assert_eq!(store.current().unwrap().suffix.len(), 1);
        let now = snap_at(Tick(3), genesis);
        let blob = store.pause_now(Arc::clone(&now));
        assert!(blob.suffix.is_empty());
        assert_eq!(blob.trace_from_tick, Tick(3));
        assert!(store.current().unwrap().suffix.is_empty());
    }

    #[test]
    fn automatic_blob_encode_includes_suffix() {
        let mut store = EpochStore::new();
        let genesis = klotho_trace::genesis_hash();
        store.on_publish(snap_at(Tick(0), genesis), &[qty_event(Tick(1))], 60);
        let bytes = encode(store.current().unwrap()).unwrap();
        let back = decode(&bytes).unwrap();
        assert_eq!(back.suffix.len(), 1);
        assert_eq!(back.suffix[0].tick, Tick(1));
    }
}
