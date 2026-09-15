//! Render HAL and wgpu presenter: clustered forward+ PBR, goldens, render thread.
//!
//! The renderer is a pure function of [`VisualManifest`] + [`Observer`] +
//! [`GpuBudget`]. Header-validate kitbash meshes before GPU upload. Sim does
//! not wait on this crate (Q2 dedicated thread).
//!
//! Unsafe is allowed (wgpu/hal). The unlit path stays the Hearth golden path.

#![allow(unsafe_code)]
#![warn(missing_docs)]

mod capture;
mod cluster;
mod extract;
mod gpu;
mod hal;
mod math;
mod overlay;
mod palette;
mod pbr_pass;
mod perm;
mod presenter;
mod probes;
mod surface;
mod thread;

pub use capture::{CAPTURE_HEIGHT, CAPTURE_WIDTH, CaptureDesc, HIST_BINS, PixelStats, luma};
pub use cluster::{PointLight, TileAssign, assign_tiles};
pub use extract::{
    VisualBind, binds_from_cooked, extract_visual, extract_visual_with_clips,
    extract_visual_with_clips_between, skinned_instance,
};
pub use gpu::{GOLDEN_HEIGHT, GOLDEN_WIDTH, WgpuPresenter};
pub use hal::{HalPresenter, RenderHal, RenderHalError};
pub use overlay::{overlay_hud, write_bmp};
pub use palette::{albedo, metalness_roughness};
pub use perm::{
    PresentPlan, PresenterPerm, cascade_count, clamp_gpu_vfx, fallback_tier, gi_enabled,
    permutation, post_for_tier, present_plan, ssgi_enabled,
};
pub use presenter::{NullPresenter, Presenter, draw_list};
pub use probes::{DEFAULT_SPACING_MM, probe_grid_ready, probe_sample};
pub use surface::WindowedPresenter;
pub use thread::RenderThread;

pub use klotho_manifest::{GpuBudget, Observer, VisualManifest};
pub use klotho_platform::GraphicsApi;

/// WGSL source for the unlit+lambert family (Hearth goldens).
pub const SHADER_WGSL: &str = include_str!("shader.wgsl");
/// WGSL source for clustered forward+ PBR.
pub const PBR_WGSL: &str = include_str!("pbr.wgsl");
/// WGSL source for SSGI / bloom / history / blit.
pub const POST_WGSL: &str = include_str!("post.wgsl");

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

    #[test]
    fn pbr_shader_is_forward_family() {
        assert!(PBR_WGSL.contains("metalness"));
        assert!(PBR_WGSL.contains("roughness"));
        assert!(PBR_WGSL.contains("fn vs"));
        assert!(PBR_WGSL.contains("fn vs_skinned"));
        assert!(PBR_WGSL.contains("fn fs"));
        assert!(!PBR_WGSL.contains("meshlet"));
        assert!(!PBR_WGSL.to_ascii_lowercase().contains("sdf"));
        assert!(!POST_WGSL.contains("meshlet"));
    }
}
