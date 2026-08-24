//! wgpu presenter: clustered static meshes, one shader family, render thread.
//!
//! The renderer is a pure function of [`VisualManifest`] + [`Observer`] +
//! [`GpuBudget`]. Header-validate kitbash meshes before GPU upload. Sim does
//! not wait on this crate (Q2 dedicated thread).
//!
//! Unsafe is allowed (wgpu/hal). v1 uses the safe wgpu API.

#![allow(unsafe_code)]
#![warn(missing_docs)]

mod extract;
mod gpu;
mod math;
mod overlay;
mod palette;
mod presenter;
mod surface;
mod thread;

pub use extract::{VisualBind, binds_from_cooked, extract_visual};
pub use gpu::{GOLDEN_HEIGHT, GOLDEN_WIDTH, WgpuPresenter};
pub use overlay::{overlay_hud, write_bmp};
pub use palette::albedo;
pub use presenter::{NullPresenter, Presenter, draw_list};
pub use surface::WindowedPresenter;
pub use thread::RenderThread;

pub use klotho_manifest::{GpuBudget, Observer, VisualManifest};

/// WGSL source for the single unlit+lambert family.
pub const SHADER_WGSL: &str = include_str!("shader.wgsl");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shader_is_one_family() {
        assert!(SHADER_WGSL.contains("fn vs"));
        assert!(SHADER_WGSL.contains("fn fs"));
        assert!(SHADER_WGSL.contains("TAG_EMISSIVE"));
        assert!(!SHADER_WGSL.contains("meshlet"));
    }
}
