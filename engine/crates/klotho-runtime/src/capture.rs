//! Named capture markers from public snapshots. Presentation-only; no Trace append.

use klotho_core::{Hash, Tick};
use klotho_ir::Name;
use klotho_world::WorldSnapshot;

/// What a capture recorded.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum CaptureKind {
    /// Semantic snapshot hashes (default KAI-06).
    Semantic,
    /// Pixel/audio/UI captures land in later PRs; the marker is reserved.
    Presentation,
}

/// One named capture. Hashes come from the snapshot, not a model.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct CaptureRecord {
    /// Authoring name of the capture point.
    pub name: Name,
    /// Snapshot tick.
    pub tick: Tick,
    /// Frozen Canon hash.
    pub canon_hash: Hash,
    /// Trace prefix at capture.
    pub prefix: Hash,
    /// Capture class.
    pub kind: CaptureKind,
}

/// Ordered capture log. Trusted tools append; agents may only read.
#[derive(Clone, Debug, Default)]
pub struct CaptureLog {
    records: Vec<CaptureRecord>,
}

impl CaptureLog {
    /// Empty log.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a mark from a published snapshot. Does not write Trace or Projection.
    pub fn mark(&mut self, name: Name, snap: &WorldSnapshot, kind: CaptureKind) -> &CaptureRecord {
        self.records.push(CaptureRecord {
            name,
            tick: snap.tick,
            canon_hash: snap.canon_hash,
            prefix: snap.trace_prefix_hash,
            kind,
        });
        self.records.last().expect("just pushed")
    }

    /// Recorded marks, capture order.
    #[must_use]
    pub fn records(&self) -> &[CaptureRecord] {
        &self.records
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use klotho_canon::cook_diffs;
    use klotho_commit::CommitKernel;
    use klotho_core::{Hash, LocusKind, PlayerId, Sigil};
    use klotho_ir::{CanonDiff, from_ron};
    use klotho_world::World;

    use super::*;

    #[test]
    fn mark_reads_snapshot_and_does_not_need_trace() {
        let d: Vec<CanonDiff> = from_ron("[]").unwrap();
        let canon = cook_diffs(&d).unwrap();
        let mut k = CommitKernel::new(World::new(Arc::new(canon), Hash::ZERO));
        let s = Sigil::pack(LocusKind::Actor, 0, 1).unwrap();
        k.bind_player(PlayerId(0), s);
        k.world_mut().insert_locus(s, LocusKind::Actor).unwrap();
        let before = k.world().trace().events().len();
        let snap = k.snapshot();
        let mut log = CaptureLog::new();
        let rec = log
            .mark(Name::from("intro"), &snap, CaptureKind::Semantic)
            .clone();
        assert_eq!(k.world().trace().events().len(), before);
        assert_eq!(rec.canon_hash, snap.canon_hash);
        assert_eq!(rec.prefix, snap.trace_prefix_hash);
        assert_eq!(log.records().len(), 1);
    }
}
