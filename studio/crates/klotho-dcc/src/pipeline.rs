//! Pinned DCC/interchange/color pipeline contract.

use std::collections::{BTreeMap, BTreeSet};

use klotho_core::Hash;
use klotho_prove::hash_bytes;
use serde::{Deserialize, Serialize};

use crate::DccError;

/// Supported DCC applications. Vendor GUI work may remain human-operated.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DccApplication {
    /// Blender worker.
    Blender,
    /// Autodesk Maya worker.
    Maya,
    /// SideFX Houdini worker.
    Houdini,
    /// Motion-capture processor.
    Mocap,
    /// Media/texture/audio encoder.
    Media,
}

/// Locked interchange format.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterchangeFormat {
    /// glTF 2.0.
    Gltf,
    /// Universal Scene Description.
    Usd,
    /// MaterialX.
    MaterialX,
    /// Human-operated FBX import worker.
    Fbx,
}

/// One pinned executable/plugin/container.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPin {
    /// Human-readable tool id.
    pub id: String,
    /// Exact version string.
    pub version: String,
    /// Executable or bundle hash.
    pub hash: Hash,
}

/// Project-wide deterministic DCC lock.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineLock {
    /// Required interchange versions.
    pub interchange: BTreeMap<InterchangeFormat, String>,
    /// OpenColorIO config hash.
    pub ocio_config: Hash,
    /// DCC/plug-in/encoder pins.
    pub tools: BTreeMap<DccApplication, Vec<ToolPin>>,
    /// Skeleton/retarget profile hashes.
    pub skeletons: BTreeSet<Hash>,
    /// Platform compression tool hashes.
    pub compressors: BTreeSet<Hash>,
}

impl PipelineLock {
    /// Validate and return the content hash of the lock.
    pub fn validate_and_hash(&self) -> Result<Hash, DccError> {
        if self.interchange.is_empty()
            || self.ocio_config == Hash::ZERO
            || self.tools.is_empty()
            || self.compressors.is_empty()
        {
            return Err(DccError::Policy("incomplete DCC pipeline lock".into()));
        }
        for pins in self.tools.values() {
            if pins.is_empty()
                || pins.iter().any(|pin| {
                    pin.id.trim().is_empty()
                        || pin.version.trim().is_empty()
                        || pin.hash == Hash::ZERO
                })
            {
                return Err(DccError::Policy("invalid DCC tool pin".into()));
            }
        }
        let text =
            ron::ser::to_string(self).map_err(|error| DccError::Policy(error.to_string()))?;
        Ok(hash_bytes(text.as_bytes()))
    }
}

/// Hash of the fixed neutral asset capture scene. A change invalidates visual
/// comparisons rather than silently moving the baseline.
#[must_use]
pub fn neutral_capture_scene_hash() -> Hash {
    hash_bytes(
        b"Klotho neutral capture v1|D65|18% gray|35mm|front,back,left,right,top|turntable120",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_in_pipeline_lock_parses_and_is_complete() {
        let lock: PipelineLock =
            ron::from_str(include_str!("../../../data/dcc/pipeline.lock.ron")).unwrap();
        assert_ne!(lock.validate_and_hash().unwrap(), Hash::ZERO);
        assert_ne!(neutral_capture_scene_hash(), Hash::ZERO);
    }
}
