//! Typed asset requests, candidate gates, and semantic Pin separation (K71/K72).

use std::collections::BTreeSet;

use klotho_core::{BlobId, Hash};
use klotho_prove::{LicenseSpan, ReleaseRights, hash_bytes};
use serde::{Deserialize, Serialize};

use crate::{DccError, GltfImport};

/// Stable request identity derived from the complete typed request.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AssetRequestId(pub Hash);

/// Content identity of one materialized candidate.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AssetCandidateId(pub Hash);

/// Production role of an asset.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetRole {
    /// Non-hero prop.
    Prop,
    /// Environment kit or dressing.
    Environment,
    /// Principal or supporting character.
    Character,
    /// Weapon or signature equipment.
    Equipment,
    /// Animation/clip source.
    Animation,
    /// Material or texture set.
    Material,
    /// Audio, VO, or music source.
    Audio,
}

/// Millimetre dimensions, frozen as integers.
#[derive(Copy, Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundsMm {
    /// X extent.
    pub x: u32,
    /// Y extent.
    pub y: u32,
    /// Z extent.
    pub z: u32,
}

/// Visual geometry limits.
#[derive(Copy, Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VisualBudget {
    /// Maximum triangles at LOD0.
    pub triangles: u32,
    /// Maximum vertices at LOD0.
    pub vertices: u32,
    /// Maximum texture bytes across the asset.
    pub texture_bytes: u64,
}

/// Material/shader limits.
#[derive(Copy, Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialBudget {
    /// Maximum material slots.
    pub slots: u16,
    /// Maximum sampled textures.
    pub textures: u16,
    /// Maximum declared shader features.
    pub shader_features: u16,
}

/// Optional rig contract.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RigContract {
    /// Required skeleton/retarget profile.
    pub skeleton: String,
    /// Maximum bones.
    pub bones: u16,
    /// Maximum influences per vertex.
    pub influences: u8,
    /// Required clips.
    pub clips: BTreeSet<String>,
}

/// LOD contract.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LodContract {
    /// Required number of levels, including LOD0.
    pub levels: u8,
    /// Maximum triangle counts per level, in order.
    pub max_triangles: Vec<u32>,
}

/// Requested semantic collision behavior.
#[derive(Copy, Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollisionRequest {
    /// No authoritative collision artifact.
    None,
    /// Candidate must propose a hull; it remains unusable until Pinned.
    ProposedHull,
    /// Candidate must match an already-Pinned semantic hash.
    Preserve(Hash),
}

/// GPU/platform content tier.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuTier {
    /// Minimum desktop tier.
    DesktopMinimum,
    /// High desktop tier.
    DesktopHigh,
    /// Handheld/low-power tier.
    Handheld,
}

/// Allowed intake route.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceRoute {
    /// Approved library retrieval.
    Retrieval,
    /// Local procedural/model production.
    GeneratedLocal,
    /// Approved remote provider.
    GeneratedRemote,
    /// Human vendor drop.
    Vendor,
    /// Direct commissioned baseline.
    Commissioned,
}

/// Complete, serializable request accepted by `asset.request`.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetRequest {
    /// Stable id. Must equal [`Self::derived_id`].
    pub id: AssetRequestId,
    /// Production role.
    pub role: AssetRole,
    /// Required affordance/binding tag.
    pub semantic_tag: String,
    /// Content hashes of approved references.
    pub references: Vec<Hash>,
    /// Physical dimensions.
    pub dimensions_mm: BoundsMm,
    /// Visual limits.
    pub visual_budget: VisualBudget,
    /// Material limits.
    pub material_budget: MaterialBudget,
    /// Optional rig requirements.
    pub rig: Option<RigContract>,
    /// LOD requirements.
    pub lods: LodContract,
    /// Semantic collision request.
    pub collision: CollisionRequest,
    /// Requested variants.
    pub variants: u16,
    /// Target tiers.
    pub platform_tiers: BTreeSet<GpuTier>,
    /// Allowed production routes.
    pub routes: BTreeSet<SourceRoute>,
}

impl AssetRequest {
    /// Derive identity with `id` zeroed, avoiding self-reference.
    pub fn derived_id(&self) -> Result<AssetRequestId, DccError> {
        let mut canonical = self.clone();
        canonical.id = AssetRequestId(Hash::ZERO);
        let bytes = ron::ser::to_string(&canonical)
            .map_err(|error| DccError::Contract(error.to_string()))?;
        Ok(AssetRequestId(hash_bytes(bytes.as_bytes())))
    }

    /// Fail closed on inconsistent or unbounded contracts.
    pub fn validate(&self) -> Result<(), DccError> {
        if self.id != self.derived_id()? {
            return Err(DccError::Contract("asset request id mismatch".into()));
        }
        if self.semantic_tag.trim().is_empty()
            || self.references.is_empty()
            || self.variants == 0
            || self.platform_tiers.is_empty()
            || self.routes.is_empty()
            || [
                self.dimensions_mm.x,
                self.dimensions_mm.y,
                self.dimensions_mm.z,
            ]
            .contains(&0)
        {
            return Err(DccError::Contract("asset request is incomplete".into()));
        }
        if self.lods.levels == 0
            || usize::from(self.lods.levels) != self.lods.max_triangles.len()
            || self.lods.max_triangles.first().copied().unwrap_or(0) > self.visual_budget.triangles
            || self
                .lods
                .max_triangles
                .windows(2)
                .any(|pair| pair[1] > pair[0])
        {
            return Err(DccError::Contract("invalid LOD contract".into()));
        }
        if let Some(rig) = &self.rig
            && (rig.skeleton.trim().is_empty() || rig.bones == 0 || rig.influences == 0)
        {
            return Err(DccError::Contract("invalid rig contract".into()));
        }
        Ok(())
    }
}

/// Objective candidate measurement supplied by trusted parsers/capture tools.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetCandidate {
    /// Content id of the complete candidate record.
    pub id: AssetCandidateId,
    /// Request this candidate answers.
    pub request: AssetRequestId,
    /// Intake route.
    pub route: SourceRoute,
    /// Immutable source bytes hash.
    pub source_hash: Hash,
    /// Cooked visual mesh.
    pub mesh: BlobId,
    /// Cooked semantic hull, when proposed/preserved.
    pub hull: Option<BlobId>,
    /// Hash over all semantic geometry (hulls/sockets/hit/traversal/occlusion).
    pub semantic_hash: Hash,
    /// Hash over visual-only artifacts.
    pub visual_hash: Hash,
    /// Measured LOD triangle counts.
    pub lod_triangles: Vec<u32>,
    /// Measured material slots.
    pub material_slots: u16,
    /// Measured textures.
    pub textures: u16,
    /// Measured bones.
    pub bones: u16,
    /// Maximum measured influences.
    pub influences: u8,
    /// Present clips.
    pub clips: BTreeSet<String>,
    /// Machine-auditable blob license.
    pub license: LicenseSpan,
    /// Full release-rights evidence.
    pub rights: ReleaseRights,
    /// Parser/cook toolchain lock hash.
    pub pipeline_lock: Hash,
}

/// One objective candidate gate.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateGate {
    /// Gate passed.
    Passed(String),
    /// Gate failed and blocks candidate approval.
    Failed(String),
}

/// Trusted validation output.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateReport {
    /// Candidate identity.
    pub candidate: AssetCandidateId,
    /// Ordered gate results.
    pub gates: Vec<CandidateGate>,
}

impl CandidateReport {
    /// True only if every objective gate passed.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.gates
            .iter()
            .all(|gate| matches!(gate, CandidateGate::Passed(_)))
    }
}

/// Human Pin of semantic geometry.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticPin {
    /// Exact approved semantic hash.
    pub semantic_hash: Hash,
    /// Named gameplay/technical owner.
    pub approved_by: String,
    /// Signed Pin/evidence record.
    pub record: Hash,
}

/// Human approval required to turn an untrusted candidate into a binding.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateApproval {
    /// Candidate being approved.
    pub candidate: AssetCandidateId,
    /// Art/asset owner.
    pub visual_approved_by: String,
    /// Exact validation evidence hash.
    pub validation: Hash,
    /// Required whenever semantic geometry exists or changes.
    pub semantic_pin: Option<SemanticPin>,
}

/// Validate candidate measurements, rights, and semantic separation.
#[must_use]
pub fn validate_candidate(request: &AssetRequest, candidate: &AssetCandidate) -> CandidateReport {
    let mut gates = Vec::new();
    gate(
        &mut gates,
        candidate.derived_id().ok() == Some(candidate.id),
        "candidate identity",
    );
    gate(
        &mut gates,
        candidate.request == request.id,
        "request identity",
    );
    gate(
        &mut gates,
        request.routes.contains(&candidate.route),
        "source route",
    );
    gate(
        &mut gates,
        candidate.license.is_exportable(),
        "license span",
    );
    gate(
        &mut gates,
        candidate.rights.validate().is_ok(),
        "release rights",
    );
    gate(
        &mut gates,
        candidate.pipeline_lock != Hash::ZERO,
        "pipeline lock",
    );
    gate(
        &mut gates,
        candidate.lod_triangles.len() == request.lods.max_triangles.len()
            && candidate
                .lod_triangles
                .iter()
                .zip(&request.lods.max_triangles)
                .all(|(actual, cap)| actual <= cap),
        "LOD triangle budget",
    );
    gate(
        &mut gates,
        candidate.material_slots <= request.material_budget.slots
            && candidate.textures <= request.material_budget.textures,
        "material budget",
    );
    if let Some(rig) = &request.rig {
        gate(
            &mut gates,
            candidate.bones <= rig.bones
                && candidate.influences <= rig.influences
                && rig.clips.is_subset(&candidate.clips),
            "rig and clips",
        );
    }
    let collision_ok = match request.collision {
        CollisionRequest::None => candidate.hull.is_none() && candidate.semantic_hash == Hash::ZERO,
        CollisionRequest::ProposedHull => {
            candidate.hull.is_some() && candidate.semantic_hash != Hash::ZERO
        }
        CollisionRequest::Preserve(hash) => {
            candidate.hull.is_some() && candidate.semantic_hash == hash
        }
    };
    gate(&mut gates, collision_ok, "semantic geometry");
    CandidateReport {
        candidate: candidate.id,
        gates,
    }
}

fn gate(gates: &mut Vec<CandidateGate>, passed: bool, label: &str) {
    gates.push(if passed {
        CandidateGate::Passed(label.into())
    } else {
        CandidateGate::Failed(label.into())
    });
}

impl AssetCandidate {
    /// Derive identity with the id field zeroed, avoiding self-reference.
    pub fn derived_id(&self) -> Result<AssetCandidateId, DccError> {
        let mut canonical = self.clone();
        canonical.id = AssetCandidateId(Hash::ZERO);
        let bytes = ron::ser::to_string(&canonical)
            .map_err(|error| DccError::Contract(error.to_string()))?;
        Ok(AssetCandidateId(hash_bytes(bytes.as_bytes())))
    }

    /// Validate approval and release a legacy compile artifact. The semantic
    /// hull is never inferred from the visual mesh here.
    pub fn release(
        &self,
        request: &AssetRequest,
        report: &CandidateReport,
        approval: &CandidateApproval,
        imported: GltfImport,
    ) -> Result<klotho_compile::DccArtifact, DccError> {
        if !report.passed()
            || report.candidate != self.id
            || approval.candidate != self.id
            || approval.visual_approved_by.trim().is_empty()
            || approval.validation == Hash::ZERO
            || imported.source_hash != self.source_hash
            || imported.tag != request.semantic_tag
        {
            return Err(DccError::Contract("candidate approval mismatch".into()));
        }
        match request.collision {
            CollisionRequest::None => {}
            CollisionRequest::ProposedHull | CollisionRequest::Preserve(_) => {
                let pin = approval
                    .semantic_pin
                    .as_ref()
                    .ok_or_else(|| DccError::Contract("semantic geometry requires Pin".into()))?;
                if pin.semantic_hash != self.semantic_hash
                    || pin.approved_by.trim().is_empty()
                    || pin.record == Hash::ZERO
                {
                    return Err(DccError::Contract("semantic Pin mismatch".into()));
                }
            }
        }
        Ok(imported.into())
    }

    /// Visual rebakes are non-semantic only when the semantic hashes match.
    #[must_use]
    pub fn is_visual_only_rebake_of(&self, prior: &Self) -> bool {
        self.request == prior.request
            && self.semantic_hash == prior.semantic_hash
            && self.visual_hash != prior.visual_hash
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use klotho_compile::{blob_of, encode_hull, encode_mesh_i16};
    use klotho_core::{AabbMm, IVec3};
    use klotho_prove::{ReleaseRights, RightsRoute};

    use super::*;

    fn hash(byte: u8) -> Hash {
        Hash([byte; 32])
    }

    fn rights(route: RightsRoute) -> ReleaseRights {
        ReleaseRights {
            route,
            origin: hash(1),
            terms: hash(2),
            ownership: hash(3),
            indemnity: hash(4),
            source_permission: hash(5),
            consent: hash(6),
            restrictions: hash(7),
            approved_by: "legal.owner".into(),
            approval: hash(8),
        }
    }

    fn request() -> AssetRequest {
        let mut request = AssetRequest {
            id: AssetRequestId(Hash::ZERO),
            role: AssetRole::Prop,
            semantic_tag: "prop.test".into(),
            references: vec![hash(10)],
            dimensions_mm: BoundsMm {
                x: 100,
                y: 100,
                z: 100,
            },
            visual_budget: VisualBudget {
                triangles: 100,
                vertices: 100,
                texture_bytes: 1_000,
            },
            material_budget: MaterialBudget {
                slots: 2,
                textures: 4,
                shader_features: 3,
            },
            rig: None,
            lods: LodContract {
                levels: 2,
                max_triangles: vec![100, 50],
            },
            collision: CollisionRequest::ProposedHull,
            variants: 1,
            platform_tiers: [GpuTier::DesktopHigh].into(),
            routes: [
                SourceRoute::Retrieval,
                SourceRoute::GeneratedLocal,
                SourceRoute::Vendor,
            ]
            .into(),
        };
        request.id = request.derived_id().unwrap();
        request
    }

    fn import() -> GltfImport {
        let mesh = encode_mesh_i16(&[[0, 0, 0], [10, 0, 0], [0, 10, 0]], &[0, 1, 2]).unwrap();
        let hull = encode_hull(AabbMm::new(
            IVec3 { x: 0, y: 0, z: 0 },
            IVec3 { x: 10, y: 10, z: 0 },
        ));
        GltfImport {
            tag: "prop.test".into(),
            license: LicenseSpan::spdx("CC0-1.0", "fixture").unwrap(),
            mesh,
            hull,
            skinned: None,
            clips: None,
            source_hash: hash(20),
        }
    }

    fn candidate(request: &AssetRequest, route: SourceRoute) -> AssetCandidate {
        let imported = import();
        let rights_route = match route {
            SourceRoute::Retrieval => RightsRoute::Retrieval,
            SourceRoute::Vendor => RightsRoute::Vendor,
            SourceRoute::Commissioned => RightsRoute::Commissioned,
            SourceRoute::GeneratedLocal | SourceRoute::GeneratedRemote => RightsRoute::Generated,
        };
        let mut candidate = AssetCandidate {
            id: AssetCandidateId(Hash::ZERO),
            request: request.id,
            route,
            source_hash: imported.source_hash,
            mesh: blob_of(&imported.mesh),
            hull: Some(blob_of(&imported.hull)),
            semantic_hash: hash_bytes(&imported.hull),
            visual_hash: hash_bytes(&imported.mesh),
            lod_triangles: vec![1, 1],
            material_slots: 1,
            textures: 1,
            bones: 0,
            influences: 0,
            clips: BTreeSet::new(),
            license: imported.license,
            rights: rights(rights_route),
            pipeline_lock: hash(30),
        };
        candidate.id = candidate.derived_id().unwrap();
        candidate
    }

    #[test]
    fn mixed_routes_validate_but_semantic_geometry_cannot_release_without_pin() {
        let request = request();
        for route in [
            SourceRoute::Retrieval,
            SourceRoute::GeneratedLocal,
            SourceRoute::Vendor,
        ] {
            let candidate = candidate(&request, route);
            let report = validate_candidate(&request, &candidate);
            assert!(report.passed(), "{route:?}: {:?}", report.gates);
            let mut approval = CandidateApproval {
                candidate: candidate.id,
                visual_approved_by: "art.owner".into(),
                validation: hash(40),
                semantic_pin: None,
            };
            assert!(
                candidate
                    .release(&request, &report, &approval, import())
                    .is_err()
            );
            approval.semantic_pin = Some(SemanticPin {
                semantic_hash: candidate.semantic_hash,
                approved_by: "gameplay.owner".into(),
                record: hash(41),
            });
            assert!(
                candidate
                    .release(&request, &report, &approval, import())
                    .is_ok()
            );
        }
    }

    #[test]
    fn visual_rebake_classification_requires_equal_semantics() {
        let request = request();
        let prior = candidate(&request, SourceRoute::Retrieval);
        let mut rebake = prior.clone();
        rebake.visual_hash = hash(99);
        rebake.id = rebake.derived_id().unwrap();
        assert!(rebake.is_visual_only_rebake_of(&prior));
        rebake.semantic_hash = hash(98);
        assert!(!rebake.is_visual_only_rebake_of(&prior));
    }
}
