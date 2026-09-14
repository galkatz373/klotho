//! Platform HAL, OS window/file/device events, audio output, and replay evidence.
//! **No world mutation** (HLD K11).
//!
//! winit backends live here. `klotho-input` maps [`klotho_input::DeviceSample`]
//! → `PlayerIntent`. Observer is built from Look analog + snapshot pose
//! ([`LookAccum`]), not by the renderer.
//!
//! Unsafe is allowed in this crate (window / JNI / console SDK / device). The
//! desktop path uses safe winit; proprietary adapters implement [`PlatformHal`]
//! outside the public workspace.

#![allow(unsafe_code)]
#![warn(missing_docs)]

mod audio;
mod console;
mod event;
mod look;
mod window;

#[cfg(feature = "audio-device")]
pub use audio::CpalDevice;
pub use audio::{
    AudioDeviceError, AudioSink, MemorySink, OUTPUT_CHANNELS, OUTPUT_HZ, PCM_QUEUE_CAP,
};
pub use console::{
    AccessibilitySample, AdapterClass, ControllerSample, EvidenceError, GraphicsApi,
    MemoryPlatformHal, MemorySample, NetworkSample, OwnedReplayEvidence, PlatformHal,
    PlatformIdentity, PlatformTarget, PresentSample, REPLAY_EVIDENCE_CAP, ReplayEvidence,
    STORAGE_SLOT_BYTES, STORAGE_SLOT_CAP, StorageSample, SuspendToken,
};
pub use event::{PlatEvent, apply_event, from_device_event, from_window_event};
pub use look::LookAccum;
pub use window::{WindowSpec, read_capped};

pub use klotho_input::{Button, DeviceSample};
pub use klotho_manifest::{EYE_HEIGHT_MM, Observer};
