//! Cook-time lowering to existing IR and Manifest descriptors (K76).

use klotho_core::{Epoch, LocusKind};
use klotho_ir::{
    Affordance, Beat, BindSrc, CanonDiff, Law, LawBody, Name, Pred, Rel, RiteGraph, RiteNode,
    RiteOp, SeedFact, Slot, Status,
};
use klotho_manifest::{ClosedCaptionCue, LocManifest, SubtitleCue, VoCue};
use klotho_prove::hash_bytes;

use crate::dialogue::DialogueModule;
use crate::error::DialogueError;
use crate::loc::LocaleCatalog;
use crate::quest::QuestGraph;

/// Locale-independent compiled narrative. Text lives in loc catalogs.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct LoweredNarrative {
    /// Beats, Laws, Rites, Affordances.
    pub canon_diffs: Vec<CanonDiff>,
    /// Speaker loci and initial Knows.
    pub seed: Vec<SeedFact>,
    /// Presentation cues for one locale. Empty until [`Self::with_locale`].
    pub loc: LocManifest,
}

impl LoweredNarrative {
    /// Content hash of the authoritative lowering (no locale text).
    #[must_use]
    pub fn branch_hash(&self) -> klotho_core::Hash {
        let mut buf = Vec::new();
        for d in &self.canon_diffs {
            buf.extend_from_slice(format!("{d:?}").as_bytes());
        }
        buf.push(0);
        for s in &self.seed {
            buf.extend_from_slice(format!("{s:?}").as_bytes());
        }
        hash_bytes(&buf)
    }

    /// Bind a locale catalog to presentation cues. Does not change [`Self::branch_hash`].
    #[must_use]
    pub fn with_locale(&self, catalog: &LocaleCatalog, module: &DialogueModule) -> Self {
        let mut subtitles = Vec::new();
        let mut captions = Vec::new();
        let mut vo = Vec::new();
        for line in &module.lines {
            let body = catalog
                .strings
                .get(&line.key)
                .map(|m| m.pattern.clone())
                .unwrap_or_default();
            subtitles.push(SubtitleCue {
                key: line.key.as_str().to_owned(),
                speaker: line.speaker.as_str().to_owned(),
                body: body.clone(),
                start: line.timing.start,
                duration: line.timing.duration,
                sdh: line.cc.sdh,
            });
            captions.push(ClosedCaptionCue {
                key: line.key.as_str().to_owned(),
                speaker: line.cc.speaker.as_str().to_owned(),
                body: if catalog.locale.as_str() == "en" {
                    line.cc.body.clone()
                } else {
                    body
                },
                sdh: line.cc.sdh,
            });
            if let Some(binding) = &line.vo {
                vo.push(VoCue {
                    key: line.key.as_str().to_owned(),
                    blob: binding.blob,
                    start: line.timing.start,
                    duration: line.timing.duration,
                });
            }
        }
        Self {
            canon_diffs: self.canon_diffs.clone(),
            seed: self.seed.clone(),
            loc: LocManifest::from_cues(
                Epoch::ZERO,
                catalog.locale.as_str(),
                subtitles,
                captions,
                vo,
            ),
        }
    }
}

/// Lower dialogue and quests to predicates, Rites/Beats, Knows, and Manifest cues.
pub fn lower(
    module: &DialogueModule,
    quests: &QuestGraph,
) -> Result<LoweredNarrative, DialogueError> {
    let mut canon_diffs = Vec::new();
    let mut seed = Vec::new();
    let mut speakers: Vec<Name> = Vec::new();
    for line in &module.lines {
        if !speakers.iter().any(|s| s == &line.speaker) {
            speakers.push(line.speaker.clone());
            seed.push(SeedFact::Locus {
                name: line.speaker.clone(),
                kind: LocusKind::Actor,
            });
            seed.push(SeedFact::Rel {
                a: line.speaker.clone(),
                rel: Rel::Knows,
                b: Name::from("Talkable"),
            });
        }
    }
    canon_diffs.push(CanonDiff::AddBeat(Beat {
        id: Name::from(format!("beat_{}", module.id).as_str()),
        notes: module.id.as_str().to_owned(),
    }));
    canon_diffs.push(CanonDiff::AddAffordance(Affordance {
        id: Name::from(format!("aff_{}", module.id).as_str()),
        requires: vec![Pred::Knows(Slot::This, Name::from("Talkable"))],
        grants: vec![Name::from("Talk")],
        conflicts: Vec::new(),
    }));
    for q in &quests.quests {
        for g in &q.grants {
            canon_diffs.push(CanonDiff::AddLaw(Law {
                id: Name::from(format!("grant_{}_{}", q.id, g).as_str()),
                when: Pred::Knows(Slot::This, g.clone()),
                body: LawBody::Pred {
                    must: Pred::Knows(Slot::This, g.clone()),
                    ought: None,
                },
            }));
        }
    }
    for line in &module.lines {
        let rite_id = Name::from(format!("dlg_{}", line.key).as_str());
        let mut nodes = vec![
            RiteNode::Op(RiteOp::Bind(BindSrc::This)),
            RiteNode::Op(RiteOp::Halt(Status::Fail)),
            RiteNode::Op(RiteOp::Emit(line.key.clone())),
        ];
        for g in &line.grants {
            nodes.push(RiteNode::Op(RiteOp::RelAdd(
                Slot::This,
                Rel::Knows,
                Slot::Name(g.clone()),
            )));
        }
        nodes.push(RiteNode::Op(RiteOp::Halt(Status::Success)));
        nodes.push(RiteNode::Op(RiteOp::Halt(Status::Fail)));
        let fail = (nodes.len() - 1) as u16;
        nodes[1] = RiteNode::Op(RiteOp::Guard(
            Pred::Knows(Slot::This, Name::from("Talkable")),
            fail,
        ));
        canon_diffs.push(CanonDiff::AddRite(RiteGraph {
            id: rite_id,
            cap_steps: 16,
            cap_ticks: 64,
            entry: 0,
            nodes,
        }));
    }
    canon_diffs.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
    seed.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
    Ok(LoweredNarrative {
        canon_diffs,
        seed,
        loc: LocManifest::empty(Epoch::ZERO, "und"),
    })
}

/// Presentation hash of a locale binding. Must not affect [`LoweredNarrative::branch_hash`].
#[must_use]
pub fn presentation_hash(
    lowered: &LoweredNarrative,
    catalog: &LocaleCatalog,
    module: &DialogueModule,
) -> klotho_core::Hash {
    let bound = lowered.with_locale(catalog, module);
    let mut buf = Vec::new();
    buf.extend_from_slice(catalog.locale.as_str().as_bytes());
    for cue in &bound.loc.subtitles {
        buf.extend_from_slice(cue.key.as_bytes());
        buf.extend_from_slice(cue.body.as_bytes());
    }
    hash_bytes(&buf)
}
