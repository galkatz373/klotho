//! Console platform boundary, replay evidence, and cert-sample HAL.
//!
//! Proprietary SDK adapters implement [`PlatformHal`] outside the public
//! workspace. The kernel never depends on this module. Public constructors
//! mint [`AdapterClass::PublicMock`] identities; a mock never becomes P1/P2
//! evidence.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use klotho_core::{Epoch, Hash};

/// Largest replay artifact accepted as one certification evidence item.
pub const REPLAY_EVIDENCE_CAP: usize = 64 * 1024 * 1024;

/// Save slots retained by a mock console adapter.
pub const STORAGE_SLOT_CAP: u8 = 16;

/// Bytes accepted in one mock save slot.
pub const STORAGE_SLOT_BYTES: usize = 4 * 1024 * 1024;

/// Runtime platform selected by the executable.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformTarget {
    /// Windows, Linux, or macOS through the existing desktop stack.
    Desktop,
    /// Microsoft console devkit through GDK.
    Gdk,
    /// PlayStation 5 devkit through the platform SDK.
    Prospero,
}

impl PlatformTarget {
    /// Stable identifier used in SKU and evidence records.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::Gdk => "gdk",
            Self::Prospero => "prospero",
        }
    }

    /// True when this target is a console SKU, not the desktop path.
    #[must_use]
    pub const fn is_console(self) -> bool {
        matches!(self, Self::Gdk | Self::Prospero)
    }
}

/// Native graphics API owned by a platform/render adapter.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphicsApi {
    /// Desktop development presenter. This is not a console certification path.
    DesktopWgpu,
    /// GDK D3D12 backend.
    GdkD3d12,
    /// Prospero Gnm/AGC backend.
    ProsperoGnmAgc,
}

impl GraphicsApi {
    /// Stable identifier used in SKU records.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DesktopWgpu => "desktop-wgpu",
            Self::GdkD3d12 => "gdk-d3d12",
            Self::ProsperoGnmAgc => "prospero-gnm-agc",
        }
    }
}

/// Whether the adapter is the in-tree mock or a proprietary SDK.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterClass {
    /// In-tree [`MemoryPlatformHal`] / public fixtures. Max claim is P0.
    PublicMock,
    /// Access-controlled GDK/Prospero adapter. Still not P2 without holder
    /// acceptance, and never public reproduction by itself.
    Proprietary,
}

impl AdapterClass {
    /// Stable identifier.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PublicMock => "public_mock",
            Self::Proprietary => "proprietary",
        }
    }
}

/// Immutable identity reported by a platform adapter at bring-up.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformIdentity {
    /// Platform serviced by the adapter.
    pub target: PlatformTarget,
    /// Native graphics path paired with that platform.
    pub graphics: GraphicsApi,
    /// SDK or desktop backend revision included in evidence reports.
    pub sdk_revision: String,
    /// Mock versus proprietary adapter class.
    pub adapter_class: AdapterClass,
}

impl PlatformIdentity {
    /// Describe the existing desktop development path.
    #[must_use]
    pub fn desktop(sdk_revision: impl Into<String>) -> Self {
        Self {
            target: PlatformTarget::Desktop,
            graphics: GraphicsApi::DesktopWgpu,
            sdk_revision: sdk_revision.into(),
            adapter_class: AdapterClass::PublicMock,
        }
    }

    /// Describe a public GDK mock adapter.
    #[must_use]
    pub fn gdk(sdk_revision: impl Into<String>) -> Self {
        Self {
            target: PlatformTarget::Gdk,
            graphics: GraphicsApi::GdkD3d12,
            sdk_revision: sdk_revision.into(),
            adapter_class: AdapterClass::PublicMock,
        }
    }

    /// Describe a public Prospero mock adapter.
    #[must_use]
    pub fn prospero(sdk_revision: impl Into<String>) -> Self {
        Self {
            target: PlatformTarget::Prospero,
            graphics: GraphicsApi::ProsperoGnmAgc,
            sdk_revision: sdk_revision.into(),
            adapter_class: AdapterClass::PublicMock,
        }
    }

    /// Describe a proprietary GDK adapter from an access-controlled workspace.
    #[must_use]
    pub fn gdk_proprietary(sdk_revision: impl Into<String>) -> Self {
        Self {
            target: PlatformTarget::Gdk,
            graphics: GraphicsApi::GdkD3d12,
            sdk_revision: sdk_revision.into(),
            adapter_class: AdapterClass::Proprietary,
        }
    }

    /// Describe a proprietary Prospero adapter from an access-controlled workspace.
    #[must_use]
    pub fn prospero_proprietary(sdk_revision: impl Into<String>) -> Self {
        Self {
            target: PlatformTarget::Prospero,
            graphics: GraphicsApi::ProsperoGnmAgc,
            sdk_revision: sdk_revision.into(),
            adapter_class: AdapterClass::Proprietary,
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

    /// Desktop wgpu is never a console certification path.
    #[must_use]
    pub const fn is_console_native(&self) -> bool {
        self.target.is_console()
            && self.is_valid()
            && !matches!(self.graphics, GraphicsApi::DesktopWgpu)
    }
}

/// One replay artifact captured as console certification evidence.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
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
    /// The adapter is suspended; mutating calls are refused.
    Suspended,
    /// Resume was requested while the adapter was running.
    NotSuspended,
    /// Resume token does not match the outstanding suspend.
    ResumeMismatch,
    /// Save slot is out of range or over the byte cap.
    Storage,
    /// Requested save slot is empty.
    NoStorage,
    /// Platform trophy/achievement submission failed. Gameplay is unchanged.
    Trophy(String),
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
            Self::Suspended => f.write_str("platform adapter is suspended"),
            Self::NotSuspended => f.write_str("platform adapter is not suspended"),
            Self::ResumeMismatch => f.write_str("suspend token does not match"),
            Self::Storage => f.write_str("console storage slot rejected"),
            Self::NoStorage => f.write_str("console storage slot is empty"),
            Self::Trophy(message) => write!(f, "platform trophy failure: {message}"),
            Self::Platform(message) => write!(f, "platform evidence failure: {message}"),
        }
    }
}

impl std::error::Error for EvidenceError {}

/// Opaque suspend generation used to resume the same adapter state.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct SuspendToken {
    generation: u32,
    record_count: u32,
}

impl SuspendToken {
    /// Suspend generation. Exposed for evidence records, not for mutation.
    #[must_use]
    pub const fn generation(self) -> u32 {
        self.generation
    }
}

/// Storage occupancy sampled for the cert suite.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct StorageSample {
    /// Occupied slots.
    pub used_slots: u8,
    /// Occupied bytes across all slots.
    pub used_bytes: u64,
}

/// Resident-memory sample. Bytes, never floats.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct MemorySample {
    /// Adapter-reported resident bytes.
    pub resident_bytes: u64,
}

/// Presentation sample. Microseconds and bytes, never floats.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct PresentSample {
    /// CPU-side present cost in microseconds.
    pub frame_us: u32,
    /// GPU-side present cost in microseconds.
    pub gpu_us: u32,
    /// Adapter-reported video memory bytes.
    pub vram_bytes: u64,
}

/// Controller / input sample.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct ControllerSample {
    /// A gamepad is connected.
    pub connected: bool,
    /// Button count reported by the adapter.
    pub buttons: u8,
    /// Remapping is available.
    pub remap: bool,
}

/// Network compliance sample. Loss is parts-per-million.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct NetworkSample {
    /// Round-trip latency in microseconds.
    pub latency_us: u32,
    /// Packet loss in parts-per-million.
    pub loss_ppm: u32,
}

/// Accessibility sample from the platform UI layer.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct AccessibilitySample {
    /// Remapping available.
    pub remap: bool,
    /// Subtitles available.
    pub subtitles: bool,
    /// Text scale in milli-units (`1000` = 1.0).
    pub text_scale_milli: u32,
}

/// Platform/SDK seam used by a title executable.
///
/// Console implementations own proprietary lifecycle, file, and device code.
/// They receive replay bytes as evidence; they never receive a mutable World.
pub trait PlatformHal: Send {
    /// Identity included with every bring-up report.
    fn identity(&self) -> &PlatformIdentity;

    /// Persist or upload a replay evidence artifact.
    fn record_replay(&mut self, evidence: ReplayEvidence<'_>) -> Result<(), EvidenceError>;

    /// Enter a suspend state. Mutating calls fail until [`Self::resume`].
    fn suspend(&mut self) -> Result<SuspendToken, EvidenceError>;

    /// Leave suspend. `token` must be the outstanding suspend.
    fn resume(&mut self, token: SuspendToken) -> Result<(), EvidenceError>;

    /// Occupied save-slot sample.
    fn sample_storage(&self) -> Result<StorageSample, EvidenceError>;

    /// Resident-memory sample.
    fn sample_memory(&self) -> Result<MemorySample, EvidenceError>;

    /// Presentation sample.
    fn sample_present(&self) -> Result<PresentSample, EvidenceError>;

    /// Controller sample.
    fn sample_controller(&self) -> Result<ControllerSample, EvidenceError>;

    /// Network sample.
    fn sample_network(&self) -> Result<NetworkSample, EvidenceError>;

    /// Accessibility sample from platform UI.
    fn sample_accessibility(&self) -> Result<AccessibilitySample, EvidenceError>;

    /// Write a bounded save slot. Failure never mutates Projection.
    fn write_storage(&mut self, slot: u8, bytes: &[u8]) -> Result<(), EvidenceError>;

    /// Read a save slot.
    fn read_storage(&self, slot: u8) -> Result<Vec<u8>, EvidenceError>;

    /// Submit a platform trophy. Failure never clears replay evidence.
    fn submit_trophy(&mut self, id: &str) -> Result<(), EvidenceError>;
}

/// Host-testable evidence collector used by desktop bring-up and CI.
#[derive(Clone, Debug)]
pub struct MemoryPlatformHal {
    identity: PlatformIdentity,
    records: Vec<OwnedReplayEvidence>,
    suspended: Option<SuspendToken>,
    generation: u32,
    slots: BTreeMap<u8, Vec<u8>>,
    trophies: Vec<String>,
    fail_trophies: bool,
    present_frame_us: u32,
    present_gpu_us: u32,
    resident_bytes: u64,
    vram_bytes: u64,
    network_latency_us: u32,
    network_loss_ppm: u32,
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
            suspended: None,
            generation: 0,
            slots: BTreeMap::new(),
            trophies: Vec::new(),
            fail_trophies: false,
            present_frame_us: 8_000,
            present_gpu_us: 6_000,
            resident_bytes: 1_073_741_824,
            vram_bytes: 512 * 1024 * 1024,
            network_latency_us: 8_000,
            network_loss_ppm: 0,
        })
    }

    /// Evidence captured so far, in submission order.
    #[must_use]
    pub fn records(&self) -> &[OwnedReplayEvidence] {
        &self.records
    }

    /// Trophy ids submitted so far. Order is submission order.
    #[must_use]
    pub fn trophies(&self) -> &[String] {
        &self.trophies
    }

    /// Force subsequent trophy submissions to fail. Replay records stay.
    pub fn fail_trophies(&mut self, fail: bool) {
        self.fail_trophies = fail;
    }

    /// Override the present-frame sample for budget-failure tests.
    pub fn set_present_frame_us(&mut self, us: u32) {
        self.present_frame_us = us;
    }

    fn refuse_if_suspended(&self) -> Result<(), EvidenceError> {
        if self.suspended.is_some() {
            Err(EvidenceError::Suspended)
        } else {
            Ok(())
        }
    }
}

impl PlatformHal for MemoryPlatformHal {
    fn identity(&self) -> &PlatformIdentity {
        &self.identity
    }

    fn record_replay(&mut self, evidence: ReplayEvidence<'_>) -> Result<(), EvidenceError> {
        self.refuse_if_suspended()?;
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

    fn suspend(&mut self) -> Result<SuspendToken, EvidenceError> {
        self.refuse_if_suspended()?;
        self.generation = self.generation.saturating_add(1);
        let token = SuspendToken {
            generation: self.generation,
            record_count: self.records.len() as u32,
        };
        self.suspended = Some(token);
        Ok(token)
    }

    fn resume(&mut self, token: SuspendToken) -> Result<(), EvidenceError> {
        match self.suspended {
            None => Err(EvidenceError::NotSuspended),
            Some(current) if current != token => Err(EvidenceError::ResumeMismatch),
            Some(_) => {
                self.suspended = None;
                Ok(())
            }
        }
    }

    fn sample_storage(&self) -> Result<StorageSample, EvidenceError> {
        let used_bytes = self.slots.values().map(|b| b.len() as u64).sum();
        Ok(StorageSample {
            used_slots: self.slots.len() as u8,
            used_bytes,
        })
    }

    fn sample_memory(&self) -> Result<MemorySample, EvidenceError> {
        Ok(MemorySample {
            resident_bytes: self.resident_bytes,
        })
    }

    fn sample_present(&self) -> Result<PresentSample, EvidenceError> {
        Ok(PresentSample {
            frame_us: self.present_frame_us,
            gpu_us: self.present_gpu_us,
            vram_bytes: self.vram_bytes,
        })
    }

    fn sample_controller(&self) -> Result<ControllerSample, EvidenceError> {
        Ok(ControllerSample {
            connected: true,
            buttons: 16,
            remap: true,
        })
    }

    fn sample_network(&self) -> Result<NetworkSample, EvidenceError> {
        Ok(NetworkSample {
            latency_us: self.network_latency_us,
            loss_ppm: self.network_loss_ppm,
        })
    }

    fn sample_accessibility(&self) -> Result<AccessibilitySample, EvidenceError> {
        Ok(AccessibilitySample {
            remap: true,
            subtitles: true,
            text_scale_milli: 1_000,
        })
    }

    fn write_storage(&mut self, slot: u8, bytes: &[u8]) -> Result<(), EvidenceError> {
        self.refuse_if_suspended()?;
        if slot >= STORAGE_SLOT_CAP || bytes.len() > STORAGE_SLOT_BYTES {
            return Err(EvidenceError::Storage);
        }
        self.slots.insert(slot, bytes.to_vec());
        Ok(())
    }

    fn read_storage(&self, slot: u8) -> Result<Vec<u8>, EvidenceError> {
        self.slots
            .get(&slot)
            .cloned()
            .ok_or(EvidenceError::NoStorage)
    }

    fn submit_trophy(&mut self, id: &str) -> Result<(), EvidenceError> {
        self.refuse_if_suspended()?;
        if id.trim().is_empty() {
            return Err(EvidenceError::Trophy("empty trophy id".into()));
        }
        if self.fail_trophies {
            return Err(EvidenceError::Trophy(id.to_owned()));
        }
        self.trophies.push(id.to_owned());
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
        assert!(PlatformIdentity::gdk("public-mock").is_valid());
        assert!(PlatformIdentity::prospero("public-mock").is_valid());
        assert!(PlatformIdentity::gdk("public-mock").is_console_native());
        assert!(PlatformIdentity::prospero("public-mock").is_console_native());
        assert!(!PlatformIdentity::desktop("wgpu-27").is_console_native());
    }

    #[test]
    fn wgpu_is_rejected_for_console_identities() {
        let bad = PlatformIdentity {
            target: PlatformTarget::Gdk,
            graphics: GraphicsApi::DesktopWgpu,
            sdk_revision: "wgpu".into(),
            adapter_class: AdapterClass::PublicMock,
        };
        assert!(!bad.is_valid());
        assert!(!bad.is_console_native());
        assert_eq!(
            MemoryPlatformHal::new(bad).unwrap_err(),
            EvidenceError::BackendMismatch
        );
    }

    #[test]
    fn public_constructors_are_mocks() {
        assert_eq!(
            PlatformIdentity::gdk("public-mock").adapter_class,
            AdapterClass::PublicMock
        );
        assert_eq!(
            PlatformIdentity::gdk_proprietary("licensed-gdk").adapter_class,
            AdapterClass::Proprietary
        );
        assert_eq!(
            PlatformIdentity::prospero_proprietary("licensed-prospero").adapter_class,
            AdapterClass::Proprietary
        );
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

    #[test]
    fn suspend_resume_preserves_records_and_blocks_mutation() {
        let mut hal = MemoryPlatformHal::new(PlatformIdentity::prospero("public-mock")).unwrap();
        hal.record_replay(evidence("kernel", b"replay")).unwrap();
        let token = hal.suspend().unwrap();
        assert_eq!(
            hal.record_replay(evidence("after", b"nope")).unwrap_err(),
            EvidenceError::Suspended
        );
        assert_eq!(
            hal.write_storage(0, b"save").unwrap_err(),
            EvidenceError::Suspended
        );
        assert_eq!(hal.records().len(), 1);
        hal.resume(token).unwrap();
        hal.write_storage(0, b"save").unwrap();
        assert_eq!(hal.read_storage(0).unwrap(), b"save");
        assert_eq!(hal.resume(token).unwrap_err(), EvidenceError::NotSuspended);
    }

    #[test]
    fn trophy_failure_does_not_clear_replay() {
        let mut hal = MemoryPlatformHal::new(PlatformIdentity::gdk("public-mock")).unwrap();
        hal.record_replay(evidence("kernel", b"replay")).unwrap();
        hal.fail_trophies(true);
        assert!(hal.submit_trophy("opened").is_err());
        assert_eq!(hal.records().len(), 1);
        assert!(hal.trophies().is_empty());
    }

    #[test]
    fn storage_is_bounded() {
        let mut hal = MemoryPlatformHal::new(PlatformIdentity::gdk("public-mock")).unwrap();
        assert_eq!(
            hal.write_storage(STORAGE_SLOT_CAP, b"x").unwrap_err(),
            EvidenceError::Storage
        );
        let too_large = vec![0; STORAGE_SLOT_BYTES + 1];
        assert_eq!(
            hal.write_storage(0, &too_large).unwrap_err(),
            EvidenceError::Storage
        );
        assert_eq!(hal.read_storage(0).unwrap_err(), EvidenceError::NoStorage);
    }
}
