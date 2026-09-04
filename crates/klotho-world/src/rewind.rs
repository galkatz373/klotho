//! Server RAM of the last `rewind_ticks` published snapshots. Unhashed.

use std::collections::VecDeque;
use std::sync::Arc;

use klotho_core::Tick;

use crate::WorldSnapshot;

/// Ring of published [`WorldSnapshot`]s, cap `rewind_ticks`.
#[derive(Clone, Debug, Default)]
pub struct RewindRing {
    cap: u16,
    snaps: VecDeque<Arc<WorldSnapshot>>,
}

impl RewindRing {
    /// Empty ring that keeps at most `cap` snapshots. `cap == 0` is a no-op.
    #[must_use]
    pub fn new(cap: u16) -> Self {
        Self {
            cap,
            snaps: VecDeque::new(),
        }
    }

    /// Current cap (`Budget.rewind_ticks`).
    #[must_use]
    pub fn cap(&self) -> u16 {
        self.cap
    }

    /// Snapshots currently held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.snaps.len()
    }

    /// True when no snapshot is stored.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.snaps.is_empty()
    }

    /// Change cap and drop oldest snaps past the new bound.
    pub fn set_cap(&mut self, cap: u16) {
        self.cap = cap;
        self.trim();
    }

    /// Push a published snapshot. Ignored when cap is 0.
    pub fn push(&mut self, snap: Arc<WorldSnapshot>) {
        if self.cap == 0 {
            self.snaps.clear();
            return;
        }
        self.snaps.push_back(snap);
        self.trim();
    }

    /// Snapshot published at exactly `tick`.
    #[must_use]
    pub fn get(&self, tick: Tick) -> Option<&WorldSnapshot> {
        self.snaps
            .iter()
            .find(|s| s.tick == tick)
            .map(std::convert::AsRef::as_ref)
    }

    /// Prefer the snap at `at`; else the newest with `tick <= at` still inside
    /// `[now - cap, now]`. `None` if that snap was never pushed.
    #[must_use]
    pub fn lookup(&self, now: Tick, at: Tick) -> Option<&WorldSnapshot> {
        if self.cap == 0 {
            return None;
        }
        let lo = Tick(now.0.saturating_sub(u64::from(self.cap)));
        if at < lo || at > now {
            return None;
        }
        if let Some(s) = self.get(at) {
            return Some(s);
        }
        self.snaps
            .iter()
            .rev()
            .find(|s| s.tick <= at && s.tick >= lo && s.tick <= now)
            .map(std::convert::AsRef::as_ref)
    }

    fn trim(&mut self) {
        while self.snaps.len() > usize::from(self.cap) {
            self.snaps.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use klotho_canon::cook_diffs;
    use klotho_core::{
        AabbMm, BlobId, Hash, IVec3, LocusKind, Mm, PoseMm, Sigil, Tick, YawMd, look_offset,
    };
    use klotho_ir::{CanonDiff, from_ron};

    use crate::HITSCAN_RANGE_MM;

    use super::*;
    use crate::World;

    fn relic(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Relic, 0, id).unwrap()
    }

    fn empty_world() -> World {
        let diffs: Vec<CanonDiff> = from_ron("[]").unwrap();
        World::new(Arc::new(cook_diffs(&diffs).unwrap()), Hash::ZERO)
    }

    #[test]
    fn cap_zero_is_noop() {
        let mut w = empty_world();
        let mut ring = RewindRing::new(0);
        ring.push(w.snapshot());
        assert!(ring.is_empty());
        assert!(ring.lookup(Tick(0), Tick(0)).is_none());
    }

    #[test]
    fn keeps_last_cap_and_lookup_prefers_exact() {
        let mut w = empty_world();
        let s = relic(1);
        w.mutate().insert_locus(s, LocusKind::Relic).unwrap();
        let mut ring = RewindRing::new(3);
        for i in 1..=5 {
            w.set_tick(Tick(i));
            w.mutate()
                .set_pose(s, PoseMm::new(Mm(i as i32), Mm(0), Mm(0), YawMd(0)))
                .unwrap();
            ring.push(w.snapshot());
        }
        assert_eq!(ring.len(), 3);
        assert!(ring.get(Tick(1)).is_none());
        assert!(ring.get(Tick(2)).is_none());
        assert_eq!(ring.get(Tick(5)).unwrap().view().pose(s).unwrap().x, Mm(5));
        let at4 = ring.lookup(Tick(5), Tick(4)).unwrap();
        assert_eq!(at4.tick, Tick(4));
        assert_eq!(at4.view().pose(s).unwrap().x, Mm(4));
    }

    #[test]
    fn lookup_missing_inside_window_is_none() {
        let mut w = empty_world();
        w.set_tick(Tick(10));
        let mut ring = RewindRing::new(12);
        ring.push(w.snapshot());
        assert!(ring.lookup(Tick(10), Tick(3)).is_none());
        assert!(ring.lookup(Tick(10), Tick(10)).is_some());
    }

    #[test]
    fn hitscan_first_hittable_on_forward_ray() {
        let diffs: Vec<CanonDiff> = from_ron(
            r#"[AddAffordance(Affordance(id: "Hittable", requires: [], grants: [], conflicts: []))]"#,
        )
        .unwrap();
        let canon = cook_diffs(&diffs).unwrap();
        let mut w = World::new(Arc::new(canon), Hash::ZERO);
        let hittable = w.canon().affordance_id("Hittable").unwrap();
        let player = relic(1);
        let dummy = relic(2);
        let miss = relic(3);
        {
            let mut m = w.mutate();
            m.insert_locus(player, LocusKind::Relic).unwrap();
            m.insert_locus(dummy, LocusKind::Relic).unwrap();
            m.insert_locus(miss, LocusKind::Relic).unwrap();
            let hull = AabbMm::new(
                IVec3 {
                    x: -400,
                    y: 0,
                    z: -400,
                },
                IVec3 {
                    x: 400,
                    y: 1800,
                    z: 400,
                },
            );
            for s in [player, dummy, miss] {
                m.set_hull(s, hull, BlobId::ZERO).unwrap();
            }
            m.set_pose(player, PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0)))
                .unwrap();
            m.set_pose(dummy, PoseMm::new(Mm(0), Mm(0), Mm(3000), YawMd(0)))
                .unwrap();
            m.set_pose(miss, PoseMm::new(Mm(8000), Mm(0), Mm(3000), YawMd(0)))
                .unwrap();
            m.set_affordance(dummy, hittable, true).unwrap();
            m.set_affordance(miss, hittable, true).unwrap();
        }
        let view = w.view();
        let dir = look_offset(YawMd::ZERO, YawMd::ZERO, HITSCAN_RANGE_MM);
        assert_eq!(
            view.hitscan(IVec3::ZERO, dir, player, hittable),
            Some(dummy)
        );
        w.mutate()
            .set_pose(dummy, PoseMm::new(Mm(8000), Mm(0), Mm(3000), YawMd(0)))
            .unwrap();
        assert!(
            w.view()
                .hitscan(IVec3::ZERO, dir, player, hittable)
                .is_none()
        );
    }
}
