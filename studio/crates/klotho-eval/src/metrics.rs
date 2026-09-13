//! Pinned SSIM and LPIPS metric plugins (KAI-18).
//!
//! Plugins are content-addressed. An unpinned or swapped plugin cannot
//! produce a gate result. Scores are milliperceptual (`0..=1000`).

use serde::{Deserialize, Serialize};

use klotho_core::Hash;
use klotho_ir::Name;
use klotho_prove::hash_bytes;

use crate::error::EvalError;

/// Pinned SSIM algorithm identity.
pub const SSIM_ID: &str = "klotho-ssim-v1";
/// Pinned LPIPS-slot algorithm identity. Integer patch L2; not a neural weight file.
pub const LPIPS_ID: &str = "klotho-lpips-v1";

const SSIM_WINDOW: usize = 8;
const SSIM_C1: i64 = 6;
const SSIM_C2: i64 = 58;
const LPIPS_GRID: usize = 4;
const LPIPS_SCALE: i64 = 65;

/// Locked metric plugin. Identity is the algorithm hash, not a file path.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricPlugin {
    /// Catalog id (`klotho-ssim-v1`).
    pub id: Name,
    /// Blake3 of the pinned algorithm constants.
    pub lock: Hash,
}

impl MetricPlugin {
    /// Pinned integer SSIM over Rec.601 luma, 8×8 windows.
    #[must_use]
    pub fn ssim_v1() -> Self {
        Self {
            id: Name::from(SSIM_ID),
            lock: ssim_lock(),
        }
    }

    /// Pinned 4×4 mean-RGB L2 occupying the LPIPS comparison slot.
    #[must_use]
    pub fn lpips_v1() -> Self {
        Self {
            id: Name::from(LPIPS_ID),
            lock: lpips_lock(),
        }
    }

    /// Reject plugins whose lock does not match the pinned algorithm.
    pub fn verify(&self) -> Result<(), EvalError> {
        let expected = if self.id.as_str() == SSIM_ID {
            ssim_lock()
        } else if self.id.as_str() == LPIPS_ID {
            lpips_lock()
        } else {
            return Err(EvalError::Capture("unknown metric plugin".into()));
        };
        if self.lock != expected {
            return Err(EvalError::Capture("unpinned metric plugin".into()));
        }
        Ok(())
    }
}

/// Pair of pinned plugins used by a capture policy.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricLock {
    /// SSIM plugin.
    pub ssim: MetricPlugin,
    /// LPIPS-slot plugin.
    pub lpips: MetricPlugin,
}

impl MetricLock {
    /// First-title pinned pair.
    #[must_use]
    pub fn first_title() -> Self {
        Self {
            ssim: MetricPlugin::ssim_v1(),
            lpips: MetricPlugin::lpips_v1(),
        }
    }

    /// Both plugins must match the pinned locks.
    pub fn verify(&self) -> Result<(), EvalError> {
        self.ssim.verify()?;
        self.lpips.verify()
    }
}

/// RGBA8 frame. Length is `width * height * 4`.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct RgbaFrame {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Packed RGBA8.
    pub rgba: Vec<u8>,
    /// Optional per-pixel mask (`0` = ignore). Same pixel count when present.
    pub mask: Option<Vec<u8>>,
}

impl RgbaFrame {
    /// Solid-color frame.
    #[must_use]
    pub fn solid(width: u32, height: u32, rgba: [u8; 4]) -> Self {
        let n = (width as usize) * (height as usize);
        let mut buf = Vec::with_capacity(n * 4);
        for _ in 0..n {
            buf.extend_from_slice(&rgba);
        }
        Self {
            width,
            height,
            rgba: buf,
            mask: None,
        }
    }

    fn pixel_count(&self) -> usize {
        (self.width as usize) * (self.height as usize)
    }
}

/// Rec.601 luma, 0..=255.
#[must_use]
pub const fn luma(r: u8, g: u8, b: u8) -> u8 {
    ((77u16 * r as u16 + 150u16 * g as u16 + 29u16 * b as u16) >> 8) as u8
}

/// Structural similarity, milliperceptual (`1000` = identical).
pub fn ssim_milli(plugin: &MetricPlugin, a: &RgbaFrame, b: &RgbaFrame) -> Result<i32, EvalError> {
    plugin.verify()?;
    if plugin.id.as_str() != SSIM_ID {
        return Err(EvalError::Capture("ssim plugin expected".into()));
    }
    same_size(a, b)?;
    let w = a.width as usize;
    let h = a.height as usize;
    if w < SSIM_WINDOW || h < SSIM_WINDOW {
        return Err(EvalError::Capture("ssim window exceeds frame".into()));
    }
    let mut acc = 0i64;
    let mut n = 0i64;
    let mut y = 0;
    while y + SSIM_WINDOW <= h {
        let mut x = 0;
        while x + SSIM_WINDOW <= w {
            acc += ssim_window(a, b, x, y);
            n += 1;
            x += SSIM_WINDOW;
        }
        y += SSIM_WINDOW;
    }
    if n == 0 {
        return Ok(1_000);
    }
    Ok((acc / n).clamp(0, 1_000) as i32)
}

/// Patch L2 milliperceptual distance (`0` = identical). Higher is worse.
pub fn lpips_milli(plugin: &MetricPlugin, a: &RgbaFrame, b: &RgbaFrame) -> Result<i32, EvalError> {
    plugin.verify()?;
    if plugin.id.as_str() != LPIPS_ID {
        return Err(EvalError::Capture("lpips plugin expected".into()));
    }
    same_size(a, b)?;
    let w = a.width as usize;
    let h = a.height as usize;
    let pw = (w / LPIPS_GRID).max(1);
    let ph = (h / LPIPS_GRID).max(1);
    let mut dist = 0i64;
    for gy in 0..LPIPS_GRID {
        for gx in 0..LPIPS_GRID {
            let (ar, ag, ab) = mean_rgb(a, gx * pw, gy * ph, pw, ph);
            let (br, bg, bb) = mean_rgb(b, gx * pw, gy * ph, pw, ph);
            dist += (ar - br) * (ar - br) + (ag - bg) * (ag - bg) + (ab - bb) * (ab - bb);
        }
    }
    Ok((dist / LPIPS_SCALE).clamp(0, 1_000) as i32)
}

fn ssim_lock() -> Hash {
    hash_bytes(b"klotho-ssim-v1|window=8|c1=6|c2=58|luma=bt601")
}

fn lpips_lock() -> Hash {
    hash_bytes(b"klotho-lpips-v1|grid=4|l2-mean-rgb|scale=65")
}

fn same_size(a: &RgbaFrame, b: &RgbaFrame) -> Result<(), EvalError> {
    if a.width != b.width || a.height != b.height || a.rgba.len() != b.rgba.len() {
        return Err(EvalError::Capture("capture size mismatch".into()));
    }
    if a.rgba.len() != a.pixel_count() * 4 {
        return Err(EvalError::Capture("rgba length mismatch".into()));
    }
    Ok(())
}

fn masked(frame: &RgbaFrame, x: usize, y: usize) -> bool {
    match &frame.mask {
        None => true,
        Some(mask) => mask.get(y * frame.width as usize + x).copied().unwrap_or(0) != 0,
    }
}

fn ssim_window(a: &RgbaFrame, b: &RgbaFrame, x0: usize, y0: usize) -> i64 {
    let mut sx = 0i64;
    let mut sy = 0i64;
    let mut sx2 = 0i64;
    let mut sy2 = 0i64;
    let mut sxy = 0i64;
    let mut n = 0i64;
    for dy in 0..SSIM_WINDOW {
        for dx in 0..SSIM_WINDOW {
            let x = x0 + dx;
            let y = y0 + dy;
            if !masked(a, x, y) || !masked(b, x, y) {
                continue;
            }
            let ia = (y * a.width as usize + x) * 4;
            let ib = (y * b.width as usize + x) * 4;
            let xa = i64::from(luma(a.rgba[ia], a.rgba[ia + 1], a.rgba[ia + 2]));
            let ya = i64::from(luma(b.rgba[ib], b.rgba[ib + 1], b.rgba[ib + 2]));
            sx += xa;
            sy += ya;
            sx2 += xa * xa;
            sy2 += ya * ya;
            sxy += xa * ya;
            n += 1;
        }
    }
    if n == 0 {
        return 1_000;
    }
    let mx = sx / n;
    let my = sy / n;
    let vx = (sx2 / n - mx * mx).max(0);
    let vy = (sy2 / n - my * my).max(0);
    let cv = sxy / n - mx * my;
    let num = (2 * mx * my + SSIM_C1) * (2 * cv + SSIM_C2);
    let den = (mx * mx + my * my + SSIM_C1) * (vx + vy + SSIM_C2);
    if den == 0 {
        1_000
    } else {
        (1_000 * num / den).clamp(0, 1_000)
    }
}

fn mean_rgb(frame: &RgbaFrame, x0: usize, y0: usize, pw: usize, ph: usize) -> (i64, i64, i64) {
    let mut r = 0i64;
    let mut g = 0i64;
    let mut b = 0i64;
    let mut n = 0i64;
    let x1 = (x0 + pw).min(frame.width as usize);
    let y1 = (y0 + ph).min(frame.height as usize);
    for y in y0..y1 {
        for x in x0..x1 {
            if !masked(frame, x, y) {
                continue;
            }
            let i = (y * frame.width as usize + x) * 4;
            r += i64::from(frame.rgba[i]);
            g += i64::from(frame.rgba[i + 1]);
            b += i64::from(frame.rgba[i + 2]);
            n += 1;
        }
    }
    if n == 0 {
        return (0, 0, 0);
    }
    (r / n, g / n, b / n)
}
