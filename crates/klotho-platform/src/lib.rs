//! OS window, file, and device events. **No world mutation** (HLD K11).
//!
//! winit backends live here. `klotho-input` maps [`klotho_input::DeviceSample`]
//! → `PlayerIntent`. Observer is built from Look analog + snapshot pose
//! ([`LookAccum`]), not by the renderer.
//!
//! Unsafe is allowed in this crate (window / JNI). v1 desktop uses the safe
//! winit API.

#![allow(unsafe_code)]
#![warn(missing_docs)]

mod event;
mod look;
mod window;

pub use event::{PlatEvent, apply_event, from_device_event, from_window_event};
pub use look::LookAccum;
pub use window::{WindowSpec, read_capped};

pub use klotho_input::{Button, DeviceSample};
pub use klotho_manifest::{EYE_HEIGHT_MM, Observer};
