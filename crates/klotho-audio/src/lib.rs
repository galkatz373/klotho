//! Grains from Trace, one bed, header caps.
//!
//! The mixer is a pure function of [`SonicManifest`] + [`Observer`] +
//! [`MixBudget`]. Header-validate grains before decode. Sim does not wait
//! on this crate.
//!
//! Unsafe is allowed (SIMD mix, decoder FFI). v1 uses a safe integer mix
//! to i16 stereo PCM. No device output.

#![allow(unsafe_code)]
#![warn(missing_docs)]

mod extract;
mod mix;

pub use extract::extract_sonic;
pub use mix::{
    IntegerMixer, MixBudget, MixFrame, Mixer, NullMixer, SAMPLES_PER_TICK, TICK_HZ, mix, mix_n,
};

pub use klotho_compile::GRAIN_HZ;
pub use klotho_manifest::{BedRef, GrainVoice, Observer, SonicManifest};
