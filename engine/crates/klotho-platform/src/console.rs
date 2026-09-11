//! Console platform boundary and replay evidence.
//!
//! Proprietary SDK adapters implement [`PlatformHal`] outside the public
//! workspace. The kernel never depends on this module.

use klotho_core::{Epoch, Hash};

/// Largest replay artifact accepted as one certification evidence item.
pub const REPLAY_EVIDENCE_CAP: usize = 64 * 1024 * 1024;

/// Runtime platform selected by the executable.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum PlatformTarget {
    /// Windows, Linux, or macOS through the existing desktop stack.
    Desktop,
    /// Microsoft console devkit through GDK.
    Gdk,
    /// PlayStation 5 devkit through the platform SDK.
    Prospero,
}

/// Native graphics API owned by a platform/render adapter.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum GraphicsApi {
    /// Desktop development presenter. This is not a console certification path.
    DesktopWgpu,
    /// GDK D3D12 backend.
    GdkD3d12,
    /// Prospero Gnm/AGC backend.
    ProsperoGnmAgc,
}

/// Immutable identity reported by a platform adapter at bring-up.
#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct PlatformIdentity {
    /// Platform serviced by the adapter.
    pub target: PlatformTarget,
    /// Native graphics path paired with that platform.
    pub graphics: GraphicsApi,
    /// SDK or desktop backend revision included in evidence reports.
    pub sdk_revision: String,
}

impl PlatformIdentity {
    /// Describe the existing desktop development path.
    #[must_use]
    pub fn desktop(sdk_revision: impl Into<String>) -> Self {
        Self {
            target: PlatformTarget::Desktop,
            graphics: GraphicsApi::DesktopWgpu,
            sdk_revision: sdk_revision.into(),
        }
    }

    /// Describe a GDK devkit adapter.
    #[must_use]
    pub fn gdk(sdk_revision: impl Into<String>) -> Self {
        Self {
            target: PlatformTarget::Gdk,
            graphics: GraphicsApi::GdkD3d12,
            sdk_revision: sdk_revision.into(),
        }
    }

    /// Describe a Prospero devkit adapter.
    #[must_use]
    pub fn prospero(sdk_revision: impl Into<String>) -> Self {
        Self {
            target: PlatformTarget::Prospero,
            graphics: GraphicsApi::ProsperoGnmAgc,
            sdk_revision: sdk_revision.into(),
        }
    }

    /// Whether the target and graphics API are a supported pairing.
    #[must_use]
    pub const fn is_valid(&self) -> bool {
        matches!(
            (self.target, self.graphics),
            (PlatformTarget::Desktop, GraphicsApi::DesktopWgpu)
                | (PlatformTarget::Gdk, GraphicsApi::GdkD3d12)
                | (PlatformTarget::Prospero, GraphicsApi::ProsperoGnmAgc)
        )
    }

    /// Whether `graphics` is the native API selected for this target.
    #[must_use]
    pub const fn accepts_graphics(&self, graphics: GraphicsApi) -> bool {
        matches!(
            (self.target, self.graphics, graphics),
            (
                PlatformTarget::Desktop,
                GraphicsApi::DesktopWgpu,
                GraphicsApi::DesktopWgpu
            ) | (
                PlatformTarget::Gdk,
                GraphicsApi::GdkD3d12,
                GraphicsApi::GdkD3d12
            ) | (
                PlatformTarget::Prospero,
                GraphicsApi::ProsperoGnmAgc,
                GraphicsApi::ProsperoGnmAgc
            )
        )
    }
}

/// One replay artifact captured as console certification evidence.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct ReplayEvidence<'a> {
    /// Portable label used by the evidence collector, without path separators.
    pub label: &'a str,
    /// Canon identity used by the replay.
    pub canon_hash: Hash,
    /// Canon epoch used by the replay.
    pub epoch: Epoch,
    /// Terminal Trace prefix stored in the replay artifact.
    pub expected_trace_prefix_hash: Hash,
    /// Terminal Trace prefix observed after replay on this target.
    pub observed_trace_prefix_hash: Hash,
    /// Opaque replay file bytes. The replay owner remains the net/debug crate.
    pub replay: &'a [u8],
}

impl ReplayEvidence<'_> {
    /// Validate path safety and the bounded artifact contract.
    pub fn validate(&self) -> Result<(), EvidenceError> {
        if self.label.is_empty()
            || self.label == "."
            || self.label == ".."
            || self.label.bytes().any(|b| matches!(b, b'/' | b'\\' | 0))
        {
            return Err(EvidenceError::InvalidLabel);
        }
        if self.replay.is_empty() {
            return Err(EvidenceError::EmptyReplay);
        }
        if self.replay.len() > REPLAY_EVIDENCE_CAP {
            return Err(EvidenceError::ReplayTooLarge);
        }
        if self.expected_trace_prefix_hash != self.observed_trace_prefix_hash {
            return Err(EvidenceError::ReplayMismatch);
        }
        Ok(())
    }
}

/// Replay evidence capture failure.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum EvidenceError {
    /// Evidence labels cannot be empty or contain path separators/NUL.
    InvalidLabel,
    /// A zero-byte file cannot demonstrate replay behavior.
    EmptyReplay,
    /// The artifact exceeds [`REPLAY_EVIDENCE_CAP`].
    ReplayTooLarge,
    /// The target replay did not produce the recorded terminal Trace prefix.
    ReplayMismatch,
    /// The target/backend pairing is invalid.
    BackendMismatch,
    /// A platform SDK or evidence store rejected the operation.
    Platform(String),
}

impl std::fmt::Display for EvidenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLabel => f.write_str("invalid replay evidence label"),
            Self::EmptyReplay => f.write_str("replay evidence is empty"),
            Self::ReplayTooLarge => f.write_str("replay evidence exceeds the 64 MiB cap"),
            Self::ReplayMismatch => f.write_str("target replay Trace prefix mismatch"),
            Self::BackendMismatch => f.write_str("platform target and graphics backend mismatch"),
            Self::Platform(message) => write!(f, "platform evidence failure: {message}"),
        }
    }
}

impl std::error::Error for EvidenceError {}

/// Platform/SDK seam used by a title executable.
///
/// Console implementations own proprietary lifecycle, file, and device code.
/// They receive replay bytes as evidence; they never receive a mutable World.
pub trait PlatformHal: Send {
    /// Identity included with every bring-up report.
    fn identity(&self) -> &PlatformIdentity;

    /// Persist or upload a replay evidence artifact.
    fn record_replay(&mut self, evidence: ReplayEvidence<'_>) -> Result<(), EvidenceError>;
}

/// Host-testable evidence collector used by desktop bring-up and CI.
#[derive(Clone, Debug)]
pub struct MemoryPlatformHal {
    identity: PlatformIdentity,
    records: Vec<OwnedReplayEvidence>,
}

/// Owned evidence retained by [`MemoryPlatformHal`].
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct OwnedReplayEvidence {
    /// Evidence label.
    pub label: String,
    /// Canon identity.
    pub canon_hash: Hash,
    /// Canon epoch.
    pub epoch: Epoch,
    /// Terminal Trace prefix stored in the replay artifact.
    pub expected_trace_prefix_hash: Hash,
    /// Matching terminal Trace prefix observed on the target.
    pub observed_trace_prefix_hash: Hash,
    /// Replay artifact bytes.
    pub replay: Vec<u8>,
}

impl MemoryPlatformHal {
    /// Empty collector for `identity`.
    pub fn new(identity: PlatformIdentity) -> Result<Self, EvidenceError> {
        if !identity.is_valid() {
            return Err(EvidenceError::BackendMismatch);
        }
        Ok(Self {
            identity,
            records: Vec::new(),
        })
    }

    /// Evidence captured so far, in submission order.
    #[must_use]
    pub fn records(&self) -> &[OwnedReplayEvidence] {
        &self.records
    }
}

impl PlatformHal for MemoryPlatformHal {
    fn identity(&self) -> &PlatformIdentity {
        &self.identity
    }

    fn record_replay(&mut self, evidence: ReplayEvidence<'_>) -> Result<(), EvidenceError> {
        evidence.validate()?;
        self.records.push(OwnedReplayEvidence {
            label: evidence.label.to_owned(),
            canon_hash: evidence.canon_hash,
            epoch: evidence.epoch,
            expected_trace_prefix_hash: evidence.expected_trace_prefix_hash,
            observed_trace_prefix_hash: evidence.observed_trace_prefix_hash,
            replay: evidence.replay.to_vec(),
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence<'a>(label: &'a str, replay: &'a [u8]) -> ReplayEvidence<'a> {
        ReplayEvidence {
            label,
            canon_hash: Hash::from_bytes([1; 32]),
            epoch: Epoch(4),
            expected_trace_prefix_hash: Hash::from_bytes([2; 32]),
            observed_trace_prefix_hash: Hash::from_bytes([2; 32]),
            replay,
        }
    }

    #[test]
    fn target_backend_pairs_are_explicit() {
        assert!(PlatformIdentity::desktop("wgpu-27").is_valid());
        assert!(PlatformIdentity::gdk("private-sdk").is_valid());
        assert!(PlatformIdentity::prospero("private-sdk").is_valid());
    }

    #[test]
    fn collector_retains_replay_and_trace_identity() {
        let mut hal = MemoryPlatformHal::new(PlatformIdentity::gdk("test-sdk")).unwrap();
        hal.record_replay(evidence("netlock-8p", b"replay bytes"))
            .unwrap();
        let record = &hal.records()[0];
        assert_eq!(record.label, "netlock-8p");
        assert_eq!(record.epoch, Epoch(4));
        assert_eq!(
            record.expected_trace_prefix_hash,
            record.observed_trace_prefix_hash
        );
        assert_eq!(record.replay, b"replay bytes");
    }

    #[test]
    fn evidence_is_bounded_and_path_safe() {
        assert_eq!(
            evidence("../escape", b"replay").validate(),
            Err(EvidenceError::InvalidLabel)
        );
        assert_eq!(
            evidence("empty", b"").validate(),
            Err(EvidenceError::EmptyReplay)
        );
        let too_large = vec![0; REPLAY_EVIDENCE_CAP + 1];
        assert_eq!(
            evidence("large", &too_large).validate(),
            Err(EvidenceError::ReplayTooLarge)
        );
        let mut mismatch = evidence("mismatch", b"replay");
        mismatch.observed_trace_prefix_hash = Hash::from_bytes([3; 32]);
        assert_eq!(mismatch.validate(), Err(EvidenceError::ReplayMismatch));
    }
}
