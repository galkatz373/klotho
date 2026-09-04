//! Grains from Trace, one bed, integer mix to i16 stereo, device output.
//!
//! The mixer is a function of [`SonicManifest`] + [`Observer`] +
//! [`MixBudget`]. Header-validate grains before decode. Sim does not wait
//! on this crate. A later [`Mixer`] may wrap FMOD.
//!
//! Unsafe is allowed (SIMD mix, decoder, device).

#![allow(unsafe_code)]
#![warn(missing_docs)]

mod extract;
mod mix;
mod thread;

pub use extract::extract_sonic;
pub use mix::{
    DeviceMixer, IntegerMixer, MixBudget, MixFrame, Mixer, NullMixer, OCCLUDED_GAIN_MILLI,
    SAMPLES_PER_TICK, TICK_HZ, mix, mix_n,
};
pub use thread::AudioThread;

pub use klotho_compile::GRAIN_HZ;
pub use klotho_manifest::{BedRef, GrainVoice, Observer, SonicManifest};
pub use klotho_platform::{
    AudioDeviceError, AudioSink, CpalDevice, MemorySink, OUTPUT_CHANNELS, OUTPUT_HZ, PCM_QUEUE_CAP,
};

const _: () = assert!(GRAIN_HZ == OUTPUT_HZ);
