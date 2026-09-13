//! Asset request queue. Privileged execution remains in `klotho-worker`.

use std::collections::BTreeMap;

use klotho_dcc::{AssetCandidateId, AssetRequest, AssetRequestId};

use crate::AiError;

/// Closed queue connecting `asset.request` to a separately privileged worker
/// broker. Registering a candidate is a trusted host action, not an AI tool.
#[derive(Clone, Default)]
pub struct AssetRequestStore {
    requests: BTreeMap<AssetRequestId, AssetRequest>,
    candidates: BTreeMap<AssetRequestId, Vec<AssetCandidateId>>,
}

impl AssetRequestStore {
    /// Validate and enqueue a typed request. Repeating identical content is
    /// idempotent; a mismatched id is rejected by `klotho-dcc`.
    pub fn submit(&mut self, request: AssetRequest) -> Result<Vec<AssetCandidateId>, AiError> {
        request
            .validate()
            .map_err(|error| AiError::Asset(error.to_string()))?;
        let id = request.id;
        if let Some(existing) = self.requests.get(&id)
            && existing != &request
        {
            return Err(AiError::Asset("asset request identity collision".into()));
        }
        self.requests.insert(id, request);
        Ok(self.candidates.get(&id).cloned().unwrap_or_default())
    }

    /// Register an output returned by the privileged broker and validated DCC
    /// pipeline. This method is intentionally absent from [`crate::ToolCall`].
    pub fn register_candidate(
        &mut self,
        request: AssetRequestId,
        candidate: AssetCandidateId,
    ) -> Result<(), AiError> {
        if !self.requests.contains_key(&request) {
            return Err(AiError::Asset("candidate has no queued request".into()));
        }
        let rows = self.candidates.entry(request).or_default();
        if !rows.contains(&candidate) {
            rows.push(candidate);
            rows.sort();
        }
        Ok(())
    }

    /// Inspect a queued request from trusted editor integration.
    #[must_use]
    pub fn request(&self, id: AssetRequestId) -> Option<&AssetRequest> {
        self.requests.get(&id)
    }
}

#[cfg(test)]
mod tests {
    use klotho_core::Hash;
    use klotho_dcc::{
        AssetCandidateId, AssetRole, BoundsMm, CollisionRequest, GpuTier, LodContract,
        MaterialBudget, SourceRoute, VisualBudget,
    };

    use super::*;

    fn request() -> AssetRequest {
        let mut request = AssetRequest {
            id: AssetRequestId(Hash::ZERO),
            role: AssetRole::Prop,
            semantic_tag: "prop.queue".into(),
            references: vec![Hash([1; 32])],
            dimensions_mm: BoundsMm { x: 1, y: 1, z: 1 },
            visual_budget: VisualBudget {
                triangles: 1,
                vertices: 1,
                texture_bytes: 1,
            },
            material_budget: MaterialBudget {
                slots: 1,
                textures: 1,
                shader_features: 1,
            },
            rig: None,
            lods: LodContract {
                levels: 1,
                max_triangles: vec![1],
            },
            collision: CollisionRequest::None,
            variants: 1,
            platform_tiers: [GpuTier::DesktopMinimum].into(),
            routes: [SourceRoute::Retrieval].into(),
        };
        request.id = request.derived_id().unwrap();
        request
    }

    #[test]
    fn queue_is_idempotent_and_candidate_registration_is_host_only() {
        let request = request();
        let id = request.id;
        let mut store = AssetRequestStore::default();
        assert!(store.submit(request.clone()).unwrap().is_empty());
        assert!(store.submit(request).unwrap().is_empty());
        let candidate = AssetCandidateId(Hash([2; 32]));
        store.register_candidate(id, candidate).unwrap();
        assert_eq!(
            store.submit(store.request(id).unwrap().clone()).unwrap(),
            vec![candidate]
        );
        assert!(
            store
                .register_candidate(AssetRequestId(Hash([3; 32])), candidate)
                .is_err()
        );
    }
}
