//! Grains from Trace, one bed, integer mix to i16 stereo, device output.
//!
//! The mixer is a function of [`SonicManifest`] + [`Observer`] +
//! [`MixBudget`]. Header-validate grains before decode. Sim does not wait
//! on this crate.

#![allow(unsafe_code)]
#![warn(missing_docs)]

mod capture;
mod extract;
mod mix;
mod music;
mod thread;

pub use capture::{AudioStats, loudness_range_milli, true_peak_milli};
pub use extract::extract_sonic;
pub use mix::{
    DeviceMixer, IntegerMixer, MixBudget, MixFrame, Mixer, NullMixer, OCCLUDED_GAIN_MILLI,
    SAMPLES_PER_TICK, TICK_HZ, mix, mix_n,
};
pub use music::{cue_from_events, may_ship_vo, stems_for};
pub use thread::AudioThread;

pub use klotho_compile::GRAIN_HZ;
pub use klotho_manifest::{BedRef, GrainVoice, Observer, SonicManifest};
#[cfg(feature = "audio-device")]
pub use klotho_platform::CpalDevice;
pub use klotho_platform::{
    AudioDeviceError, AudioSink, MemorySink, OUTPUT_CHANNELS, OUTPUT_HZ, PCM_QUEUE_CAP,
};

const _: () = assert!(GRAIN_HZ == OUTPUT_HZ);
