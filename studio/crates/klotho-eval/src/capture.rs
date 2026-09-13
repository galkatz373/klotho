//! Aligned capture sets and backend baselines (KAI-18).

use serde::{Deserialize, Serialize};

use klotho_core::Hash;
use klotho_ir::{Name, to_ron};
use klotho_prove::hash_bytes;

use crate::error::EvalError;
use crate::metrics::{MetricLock, RgbaFrame, lpips_milli, ssim_milli};

/// Pinned capture policy. Cross-device byte hashes are never compared.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapturePolicy {
    /// Policy version.
    pub version: u32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Color transform (`rec709-v1`).
    pub color_transform: Name,
    /// Camera / shot.
    pub camera: Name,
    /// Discarded frames before measurement.
    pub warmup_frames: u32,
    /// Driver from the machine manifest.
    pub driver: Name,
    /// Graphics backend.
    pub backend: Name,
    /// Temporal-jitter sequence id.
    pub jitter_seq: u8,
    /// Pinned SSIM/LPIPS pair.
    pub metrics: MetricLock,
}

impl CapturePolicy {
    /// First-title 1080p High lane.
    #[must_use]
    pub fn first_title() -> Self {
        Self {
            version: 1,
            width: 1_920,
            height: 1_080,
            color_transform: Name::from("rec709-v1"),
            camera: Name::from("hero"),
            warmup_frames: 8,
            driver: Name::from("macos-metal-25b78"),
            backend: Name::from("metal"),
            jitter_seq: 1,
            metrics: MetricLock::first_title(),
        }
    }

    /// Compact synthetic lane for unit tests.
    #[must_use]
    pub fn test_lane() -> Self {
        Self {
            version: 1,
            width: 16,
            height: 16,
            color_transform: Name::from("rec709-v1"),
            camera: Name::from("hero"),
            warmup_frames: 0,
            driver: Name::from("ci"),
            backend: Name::from("null"),
            jitter_seq: 0,
            metrics: MetricLock::first_title(),
        }
    }

    /// Content hash of the policy. Used as the baseline identity.
    pub fn hash(&self) -> Result<Hash, EvalError> {
        let bytes = to_ron(self).map_err(|e| EvalError::Host(e.to_string()))?;
        Ok(hash_bytes(bytes.as_bytes()))
    }

    /// Same lane: size, color, camera, driver, backend, jitter, metrics.
    #[must_use]
    pub fn aligned_with(&self, other: &Self) -> bool {
        self.version == other.version
            && self.width == other.width
            && self.height == other.height
            && self.color_transform == other.color_transform
            && self.camera == other.camera
            && self.warmup_frames == other.warmup_frames
            && self.driver == other.driver
            && self.backend == other.backend
            && self.jitter_seq == other.jitter_seq
            && self.metrics == other.metrics
    }
}

/// Driver/backend baseline. Separate from other GPUs.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackendBaseline {
    /// Backend id.
    pub backend: Name,
    /// Driver id.
    pub driver: Name,
    /// Hash of the approved reference capture set.
    pub reference: Hash,
}

/// One aligned view in a capture set.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct CaptureView {
    /// View name (`hero`, `ui.pause`).
    pub name: Name,
    /// RGBA frame plus optional mask.
    pub frame: RgbaFrame,
}

/// Capture set produced under one policy.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct CaptureSet {
    /// Policy the frames were produced under.
    pub policy: CapturePolicy,
    /// Named views, sorted by name at construction.
    pub views: Vec<CaptureView>,
}

impl CaptureSet {
    /// Build a set. Views must match the policy size.
    pub fn new(policy: CapturePolicy, mut views: Vec<CaptureView>) -> Result<Self, EvalError> {
        policy.metrics.verify()?;
        views.sort_by(|a, b| a.name.as_str().cmp(b.name.as_str()));
        if views.windows(2).any(|pair| pair[0].name == pair[1].name) {
            return Err(EvalError::Capture("duplicate capture view".into()));
        }
        for view in &views {
            if view.frame.width != policy.width || view.frame.height != policy.height {
                return Err(EvalError::Capture("view size does not match policy".into()));
            }
        }
        Ok(Self { policy, views })
    }

    /// Content hash of view names and pixels. Not compared across backends.
    pub fn hash(&self) -> Hash {
        let mut bytes = Vec::new();
        for view in &self.views {
            bytes.extend_from_slice(view.name.as_str().as_bytes());
            bytes.extend_from_slice(&view.frame.rgba);
        }
        hash_bytes(&bytes)
    }
}

/// Numeric comparison of two aligned capture sets.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct CaptureDelta {
    /// Per-view SSIM milli (higher is closer).
    pub ssim: Vec<(Name, i32)>,
    /// Per-view LPIPS milli (lower is closer).
    pub lpips: Vec<(Name, i32)>,
}

/// Compare `candidate` to `reference`. Different backends fail closed.
pub fn compare_captures(
    reference: &CaptureSet,
    candidate: &CaptureSet,
) -> Result<CaptureDelta, EvalError> {
    if !reference.policy.aligned_with(&candidate.policy) {
        return Err(EvalError::Capture(
            "captures are not aligned; backend baselines stay separate".into(),
        ));
    }
    if reference.views.len() != candidate.views.len() {
        return Err(EvalError::Capture("capture view count mismatch".into()));
    }
    let mut ssim = Vec::new();
    let mut lpips = Vec::new();
    for (left, right) in reference.views.iter().zip(candidate.views.iter()) {
        if left.name != right.name {
            return Err(EvalError::Capture("capture view order mismatch".into()));
        }
        ssim.push((
            left.name.clone(),
            ssim_milli(&reference.policy.metrics.ssim, &left.frame, &right.frame)?,
        ));
        lpips.push((
            left.name.clone(),
            lpips_milli(&reference.policy.metrics.lpips, &left.frame, &right.frame)?,
        ));
    }
    Ok(CaptureDelta { ssim, lpips })
}
