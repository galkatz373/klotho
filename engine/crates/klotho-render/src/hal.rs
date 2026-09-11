//! Render backend seam for desktop and proprietary console presenters.

use klotho_manifest::{GpuBudget, Observer, VisualManifest};
use klotho_platform::{GraphicsApi, PlatformIdentity};

use crate::presenter::Presenter;

/// A failure reported by a native render backend.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum RenderHalError {
    /// The selected platform/API adapter is unavailable in this build.
    Unavailable,
    /// The graphics device was lost.
    DeviceLost,
    /// The backend could not allocate required presentation memory.
    OutOfMemory,
    /// Backend-specific failure safe to expose in diagnostics.
    Backend(String),
    /// The render backend does not match the executable's platform target.
    PlatformMismatch,
}

impl std::fmt::Display for RenderHalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable => f.write_str("render HAL is unavailable in this build"),
            Self::DeviceLost => f.write_str("render device lost"),
            Self::OutOfMemory => f.write_str("render HAL out of memory"),
            Self::Backend(message) => write!(f, "render HAL failure: {message}"),
            Self::PlatformMismatch => f.write_str("render HAL does not match the platform target"),
        }
    }
}

impl std::error::Error for RenderHalError {}

/// Native render backend contract.
///
/// This boundary consumes disposable Manifest data. It does not expose GPU
/// results to simulation and it is intentionally above D3D12/Gnm command APIs.
pub trait RenderHal: Send {
    /// Native API used by this backend.
    fn graphics_api(&self) -> GraphicsApi;

    /// Present one disposable Manifest frame.
    fn present_manifest(
        &mut self,
        vis: &VisualManifest,
        observer: Observer,
        budget: GpuBudget,
    ) -> Result<(), RenderHalError>;
}

/// Adapts a fallible native HAL to the existing fire-and-forget presenter API.
///
/// Presentation failures are retained for diagnostics and never feed the
/// authoritative simulation.
pub struct HalPresenter<H> {
    hal: H,
    last_error: Option<RenderHalError>,
}

impl<H: RenderHal> HalPresenter<H> {
    /// Wrap a native backend.
    #[must_use]
    pub const fn new(hal: H) -> Self {
        Self {
            hal,
            last_error: None,
        }
    }

    /// Wrap a backend after checking it against the platform adapter identity.
    pub fn for_platform(platform: &PlatformIdentity, hal: H) -> Result<Self, RenderHalError> {
        if !platform.accepts_graphics(hal.graphics_api()) {
            return Err(RenderHalError::PlatformMismatch);
        }
        Ok(Self::new(hal))
    }

    /// Native API used by the wrapped backend.
    #[must_use]
    pub fn graphics_api(&self) -> GraphicsApi {
        self.hal.graphics_api()
    }

    /// Last presentation error, if any.
    #[must_use]
    pub fn last_error(&self) -> Option<&RenderHalError> {
        self.last_error.as_ref()
    }

    /// Borrow the native backend.
    #[must_use]
    pub const fn hal(&self) -> &H {
        &self.hal
    }

    /// Mutably borrow the native backend.
    #[must_use]
    pub const fn hal_mut(&mut self) -> &mut H {
        &mut self.hal
    }
}

impl<H: RenderHal> Presenter for HalPresenter<H> {
    fn present(&mut self, vis: &VisualManifest, observer: Observer, budget: GpuBudget) {
        self.last_error = self.hal.present_manifest(vis, observer, budget).err();
    }
}

#[cfg(test)]
mod tests {
    use klotho_core::Epoch;

    use super::*;

    struct FailingHal;

    impl RenderHal for FailingHal {
        fn graphics_api(&self) -> GraphicsApi {
            GraphicsApi::GdkD3d12
        }

        fn present_manifest(
            &mut self,
            _vis: &VisualManifest,
            _observer: Observer,
            _budget: GpuBudget,
        ) -> Result<(), RenderHalError> {
            Err(RenderHalError::DeviceLost)
        }
    }

    #[test]
    fn native_failure_stays_in_presentation() {
        let mut presenter = HalPresenter::new(FailingHal);
        presenter.present(
            &VisualManifest::empty(Epoch::ZERO),
            Observer::origin(),
            GpuBudget::HEARTH,
        );
        assert_eq!(presenter.graphics_api(), GraphicsApi::GdkD3d12);
        assert_eq!(presenter.last_error(), Some(&RenderHalError::DeviceLost));
    }

    #[test]
    fn desktop_wgpu_cannot_claim_a_console_target() {
        struct DesktopHal;
        impl RenderHal for DesktopHal {
            fn graphics_api(&self) -> GraphicsApi {
                GraphicsApi::DesktopWgpu
            }

            fn present_manifest(
                &mut self,
                _vis: &VisualManifest,
                _observer: Observer,
                _budget: GpuBudget,
            ) -> Result<(), RenderHalError> {
                Ok(())
            }
        }

        let result = HalPresenter::for_platform(&PlatformIdentity::gdk("test-sdk"), DesktopHal);
        assert!(matches!(result, Err(RenderHalError::PlatformMismatch)));
    }
}
