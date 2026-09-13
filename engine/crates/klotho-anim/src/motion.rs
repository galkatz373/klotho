//! Skeleton, retarget, and MotionDb table query (KAI-17).
//!
//! Matching is a deterministic table lookup. It does not write Projection and
//! does not move Rite `WAIT` windows.

use klotho_core::Hash;
use klotho_ir::Verb;

use crate::clip::{ArtifactAuthority, Clip, ClipSet, EvidenceLane, classify_clip};

/// Named bone in a production skeleton.
#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct Bone {
    /// Authoring name (`hips`, `head`).
    pub name: String,
    /// Parent index, or `None` for the root.
    pub parent: Option<u16>,
}

/// Pinned skeleton contract. Bone count is capped at [`crate::MAX_SKIN_BONES`].
#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct Skeleton {
    /// Content hash of the rest pose / naming.
    pub id: Hash,
    /// Bones in index order.
    pub bones: Vec<Bone>,
}

impl Skeleton {
    /// First-title biped: hips, spine, head.
    #[must_use]
    pub fn biped() -> Self {
        Self {
            id: Hash::from_bytes([0x71; 32]),
            bones: vec![
                Bone {
                    name: "hips".into(),
                    parent: None,
                },
                Bone {
                    name: "spine".into(),
                    parent: Some(0),
                },
                Bone {
                    name: "head".into(),
                    parent: Some(1),
                },
            ],
        }
    }

    /// `true` when every parent index is in range and earlier than its child.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        if self.bones.is_empty() || self.bones.len() > crate::MAX_SKIN_BONES {
            return false;
        }
        if self.id == Hash::ZERO {
            return false;
        }
        for (i, bone) in self.bones.iter().enumerate() {
            if bone.name.trim().is_empty() {
                return false;
            }
            if let Some(p) = bone.parent {
                if usize::from(p) >= i {
                    return false;
                }
            }
        }
        true
    }
}

/// Source→target bone map. Used at cook; runtime samples the target palette.
#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct RetargetProfile {
    /// Source skeleton hash.
    pub source: Hash,
    /// Target skeleton hash.
    pub target: Hash,
    /// `(source_index, target_index)` pairs, sorted by source.
    pub map: Vec<(u16, u16)>,
}

impl RetargetProfile {
    /// Identity map for a skeleton onto itself.
    #[must_use]
    pub fn identity(skel: &Skeleton) -> Self {
        Self {
            source: skel.id,
            target: skel.id,
            map: (0..skel.bones.len() as u16).map(|i| (i, i)).collect(),
        }
    }

    /// `true` when every index is in range for the two skeletons.
    #[must_use]
    pub fn is_valid(&self, source: &Skeleton, target: &Skeleton) -> bool {
        if self.source != source.id || self.target != target.id {
            return false;
        }
        self.map
            .iter()
            .all(|&(s, t)| (s as usize) < source.bones.len() && (t as usize) < target.bones.len())
    }
}

/// One MotionDb row. Velocities are millimetres per tick, inclusive range.
#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct MotionEntry {
    /// Verb this row matches.
    pub verb: Verb,
    /// Inclusive lower speed.
    pub vel_min: i32,
    /// Inclusive upper speed.
    pub vel_max: i32,
    /// Clip table id.
    pub clip: u16,
}

/// Cooked motion-matching table. Query is `F(verb, speed, grounded)`.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Default)]
pub struct MotionDb {
    /// Skeleton this table was cooked against.
    pub skeleton: Hash,
    /// Rows in table order. First match wins.
    pub entries: Vec<MotionEntry>,
}

impl MotionDb {
    /// Hearth walk/idle/use rows over [`ClipSet::hearth`].
    #[must_use]
    pub fn hearth() -> Self {
        Self {
            skeleton: Skeleton::biped().id,
            entries: vec![
                MotionEntry {
                    verb: Verb::Look,
                    vel_min: 0,
                    vel_max: 0,
                    clip: 0,
                },
                MotionEntry {
                    verb: Verb::Move,
                    vel_min: 1,
                    vel_max: i32::MAX,
                    clip: 1,
                },
                MotionEntry {
                    verb: Verb::Use,
                    vel_min: 0,
                    vel_max: i32::MAX,
                    clip: 2,
                },
            ],
        }
    }

    /// First matching clip id. Missing verb falls back to Look / 0.
    #[must_use]
    pub fn query(&self, verb: Verb, speed_mm_per_tick: i32) -> u16 {
        self.entries
            .iter()
            .find(|e| {
                e.verb == verb && speed_mm_per_tick >= e.vel_min && speed_mm_per_tick <= e.vel_max
            })
            .map(|e| e.clip)
            .or_else(|| {
                self.entries
                    .iter()
                    .find(|e| e.verb == Verb::Look)
                    .map(|e| e.clip)
            })
            .unwrap_or(0)
    }
}

/// Hull or root-motion edits stay semantic. Joint/retarget/LOD stay visual.
#[must_use]
pub fn classify_geometry_change(
    clip: Option<&Clip>,
    hull_changed: bool,
    lod_only: bool,
) -> EvidenceLane {
    if hull_changed {
        return EvidenceLane::SemanticJourneys;
    }
    if lod_only {
        return EvidenceLane::VisualOnly;
    }
    match clip {
        Some(c) if classify_clip(c) == ArtifactAuthority::Semantic => {
            EvidenceLane::SemanticJourneys
        }
        _ => EvidenceLane::VisualOnly,
    }
}

/// Selecting a MotionDb row must not rewrite the ClipSet root samples.
#[must_use]
pub fn motiondb_preserves_clipset(db: &MotionDb, clips: &ClipSet, verb: Verb, speed: i32) -> bool {
    let id = db.query(verb, speed);
    clips.clips.iter().any(|c| c.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clip::ClipSet;
    use klotho_ir::Verb;

    #[test]
    fn biped_identity_retarget_is_valid() {
        let s = Skeleton::biped();
        assert!(s.is_valid());
        let r = RetargetProfile::identity(&s);
        assert!(r.is_valid(&s, &s));
    }

    #[test]
    fn motiondb_picks_walk_and_does_not_invent_clips() {
        let db = MotionDb::hearth();
        let clips = ClipSet::hearth();
        assert_eq!(db.query(Verb::Move, 20), 1);
        assert_eq!(db.query(Verb::Look, 0), 0);
        assert!(motiondb_preserves_clipset(&db, &clips, Verb::Move, 20));
        assert_eq!(clips.clips[1].samples[0].z, crate::WALK_MM_PER_TICK);
    }

    #[test]
    fn lod_only_is_visual_hull_is_semantic() {
        let walk = &ClipSet::hearth().clips[1];
        assert_eq!(
            classify_geometry_change(Some(walk), false, true),
            EvidenceLane::VisualOnly
        );
        assert_eq!(
            classify_geometry_change(Some(walk), true, true),
            EvidenceLane::SemanticJourneys
        );
        assert_eq!(
            classify_geometry_change(Some(walk), false, false),
            EvidenceLane::SemanticJourneys
        );
    }
}
