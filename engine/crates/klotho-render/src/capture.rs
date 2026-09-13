//! CPU capture hooks for aligned pixel evaluation (KAI-18).
//!
//! Presenters fill an RGBA8 buffer. This module never writes sim state and
//! does not decide artistic pass/fail; SSIM/LPIPS live in `klotho-eval`.

use crate::{GOLDEN_HEIGHT, GOLDEN_WIDTH};

/// First-title capture surface from the pinned GPU machine manifest.
pub const CAPTURE_WIDTH: u32 = 1_920;
/// First-title capture height.
pub const CAPTURE_HEIGHT: u32 = 1_080;
/// Histogram bins over luma `0..=255`.
pub const HIST_BINS: usize = 16;

/// Pinned capture identity. Cross-backend byte hashes are not compared.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct CaptureDesc {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Color transform id (`rec709-v1`).
    pub color_transform: &'static str,
    /// Camera / shot name.
    pub camera: &'static str,
    /// Discarded frames before the measured capture.
    pub warmup_frames: u32,
    /// Driver string from the machine manifest.
    pub driver: &'static str,
    /// Graphics backend (`metal`, `vulkan`, `dx12`).
    pub backend: &'static str,
    /// Temporal-jitter sequence id.
    pub jitter_seq: u8,
}

impl CaptureDesc {
    /// Pinned first-title 1080p High capture.
    #[must_use]
    pub const fn first_title() -> Self {
        Self {
            width: CAPTURE_WIDTH,
            height: CAPTURE_HEIGHT,
            color_transform: "rec709-v1",
            camera: "hero",
            warmup_frames: 8,
            driver: "macos-metal-25b78",
            backend: "metal",
            jitter_seq: 1,
        }
    }

    /// Offscreen golden size used by Hearth pixel tests.
    #[must_use]
    pub const fn golden() -> Self {
        Self {
            width: GOLDEN_WIDTH,
            height: GOLDEN_HEIGHT,
            color_transform: "rec709-v1",
            camera: "golden",
            warmup_frames: 0,
            driver: "ci",
            backend: "null",
            jitter_seq: 0,
        }
    }

    /// `true` when two captures may be compared (same lane, not merely same size).
    #[must_use]
    pub const fn aligned_with(&self, other: &Self) -> bool {
        self.width == other.width
            && self.height == other.height
            && equal(self.color_transform, other.color_transform)
            && equal(self.camera, other.camera)
            && self.warmup_frames == other.warmup_frames
            && equal(self.driver, other.driver)
            && equal(self.backend, other.backend)
            && self.jitter_seq == other.jitter_seq
    }
}

const fn equal(a: &str, b: &str) -> bool {
    a.len() == b.len() && {
        let ab = a.as_bytes();
        let bb = b.as_bytes();
        let mut i = 0;
        while i < ab.len() {
            if ab[i] != bb[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// Objective pixel measurements. Not a visual-quality score.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct PixelStats {
    /// Capture description the pixels were produced under.
    pub desc: CaptureDesc,
    /// Luma histogram, 16 bins.
    pub hist: [u32; HIST_BINS],
    /// Pixels with luma 0.
    pub underexposed: u32,
    /// Pixels with luma 255.
    pub overexposed: u32,
    /// Non-finite pixels. Integer RGBA is always 0.
    pub nan_pixels: u32,
    /// Overdraw samples reported by the presenter.
    pub overdraw: u32,
    /// Missing CAS / kitbash binds.
    pub missing_assets: u32,
    /// Debug / fallback materials.
    pub fallback_assets: u32,
}

impl PixelStats {
    /// Measure an RGBA8 buffer. Length must be `width * height * 4`.
    #[must_use]
    pub fn measure(
        desc: CaptureDesc,
        rgba: &[u8],
        overdraw: u32,
        missing: u32,
        fallback: u32,
    ) -> Self {
        let mut hist = [0u32; HIST_BINS];
        let mut underexposed = 0u32;
        let mut overexposed = 0u32;
        let expected = (desc.width as usize)
            .saturating_mul(desc.height as usize)
            .saturating_mul(4);
        let pixels = if rgba.len() == expected { rgba } else { &[] };
        for px in pixels.chunks_exact(4) {
            let y = luma(px[0], px[1], px[2]);
            hist[(y as usize) / (256 / HIST_BINS)] += 1;
            if y == 0 {
                underexposed += 1;
            }
            if y == 255 {
                overexposed += 1;
            }
        }
        Self {
            desc,
            hist,
            underexposed,
            overexposed,
            nan_pixels: 0,
            overdraw,
            missing_assets: missing,
            fallback_assets: fallback,
        }
    }
}

/// Rec.601 integer luma, 0..=255.
#[must_use]
pub const fn luma(r: u8, g: u8, b: u8) -> u8 {
    ((77u16 * r as u16 + 150u16 * g as u16 + 29u16 * b as u16) >> 8) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_title_is_not_cross_backend_aligned() {
        let a = CaptureDesc::first_title();
        let mut b = a;
        b.backend = "vulkan";
        assert!(!a.aligned_with(&b));
        assert!(a.aligned_with(&CaptureDesc::first_title()));
    }

    #[test]
    fn missing_assets_and_exposure_are_counted() {
        let desc = CaptureDesc::golden();
        let mut rgba = vec![0u8; (GOLDEN_WIDTH * GOLDEN_HEIGHT * 4) as usize];
        rgba[0] = 255;
        rgba[1] = 255;
        rgba[2] = 255;
        rgba[3] = 255;
        let stats = PixelStats::measure(desc, &rgba, 12, 1, 2);
        assert_eq!(stats.missing_assets, 1);
        assert_eq!(stats.fallback_assets, 2);
        assert_eq!(stats.overdraw, 12);
        assert_eq!(stats.nan_pixels, 0);
        assert_eq!(stats.overexposed, 1);
        assert!(stats.underexposed > 0);
        assert_eq!(stats.hist.iter().sum::<u32>(), GOLDEN_WIDTH * GOLDEN_HEIGHT);
    }
}
