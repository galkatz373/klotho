//! Domain quality gates over aligned captures (KAI-18 / K73).

use klotho_anim::{
    FOOT_SLIDE_CAP_MM, ROOT_DISCONTINUITY_CAP_MM, foot_slide_mm, root_discontinuity_mm,
    wait_timing_preserved,
};
use klotho_audio::AudioStats;
use klotho_cinematic::CameraCapture;
use klotho_core::IVec3;
use klotho_ir::Name;
use klotho_ui::{CaptureCell, CaptureReport};

use crate::capture::{CaptureSet, compare_captures};
use crate::critic::{GateResult, QualityVerdict};
use crate::error::EvalError;
use crate::metrics::RgbaFrame;

/// SSIM milliperceptual floor for an aligned same-backend pair.
pub const SSIM_FLOOR_MILLI: i32 = 900;
/// LPIPS milliperceptual ceiling (lower is closer).
pub const LPIPS_CEILING_MILLI: i32 = 80;
/// Camera cut continuity cap, millimetres.
pub const CUT_CAP_MM: i32 = 500;
/// Hero on-screen half-width, millimetres.
pub const HERO_HALF_W_MM: i32 = 2_000;
/// Hero on-screen half-depth, millimetres.
pub const HERO_HALF_D_MM: i32 = 1_200;

/// Evaluate a candidate capture set against an approved reference.
pub fn visual_gates(
    reference: &CaptureSet,
    candidate: &CaptureSet,
    missing_assets: u32,
    fallback_assets: u32,
    nan_pixels: u32,
) -> Result<QualityVerdict, EvalError> {
    let delta = compare_captures(reference, candidate)?;
    let mut verdict = QualityVerdict::default();
    verdict.gates.push(gate(
        "missing_assets",
        missing_assets == 0,
        missing_assets as i32,
        0,
    ));
    verdict.gates.push(gate(
        "fallback_assets",
        fallback_assets == 0,
        fallback_assets as i32,
        0,
    ));
    verdict
        .gates
        .push(gate("nan_pixels", nan_pixels == 0, nan_pixels as i32, 0));
    for (name, value) in &delta.ssim {
        verdict.gates.push(gate(
            &format!("ssim.{}", name.as_str()),
            *value >= SSIM_FLOOR_MILLI,
            *value,
            SSIM_FLOOR_MILLI,
        ));
    }
    for (name, value) in &delta.lpips {
        verdict.gates.push(gate(
            &format!("lpips.{}", name.as_str()),
            *value <= LPIPS_CEILING_MILLI,
            *value,
            LPIPS_CEILING_MILLI,
        ));
    }
    Ok(verdict)
}

/// Animation foot-slide, root pop, and WAIT timing.
pub fn animation_gates(
    planted: bool,
    foot_from: IVec3,
    foot_to: IVec3,
    root_prev: IVec3,
    root_next: IVec3,
    wait_before: u32,
    wait_after: u32,
) -> QualityVerdict {
    let slide = foot_slide_mm(planted, foot_from, foot_to);
    let pop = root_discontinuity_mm(root_prev, root_next);
    let mut verdict = QualityVerdict::default();
    verdict.gates.push(gate(
        "foot_slide_mm",
        slide <= FOOT_SLIDE_CAP_MM,
        slide,
        FOOT_SLIDE_CAP_MM,
    ));
    verdict.gates.push(gate(
        "root_discontinuity_mm",
        pop <= ROOT_DISCONTINUITY_CAP_MM,
        pop,
        ROOT_DISCONTINUITY_CAP_MM,
    ));
    verdict.gates.push(gate(
        "wait_timing",
        wait_timing_preserved(wait_before, wait_after),
        wait_after as i32,
        wait_before as i32,
    ));
    verdict
}

/// Camera visibility, hull, shake, and cut continuity.
pub fn camera_gates(
    capture: CameraCapture,
    response: klotho_ir::CameraResponse,
    reduce_shake: bool,
) -> QualityVerdict {
    let mut verdict = QualityVerdict::default();
    verdict.gates.push(gate(
        "hero_visible",
        capture.hero_visible(HERO_HALF_W_MM, HERO_HALF_D_MM),
        i32::from(capture.hero_visible(HERO_HALF_W_MM, HERO_HALF_D_MM)),
        1,
    ));
    verdict.gates.push(gate(
        "camera_hull",
        capture.hull_clear(),
        capture.hull_hits as i32,
        0,
    ));
    let shake = capture
        .shake
        .x
        .unsigned_abs()
        .max(capture.shake.y.unsigned_abs())
        .max(capture.shake.z.unsigned_abs());
    verdict.gates.push(gate(
        "camera_shake",
        capture.shake_ok(response, reduce_shake),
        shake as i32,
        i32::from(response.presented_shake_mm(reduce_shake)),
    ));
    verdict.gates.push(gate(
        "cut_continuity_mm",
        capture.cut_ok(CUT_CAP_MM),
        capture.cut_delta_mm,
        CUT_CAP_MM,
    ));
    verdict
}

/// Audio peak, missing cues, voices, subtitle alignment.
pub fn audio_gates(stats: &AudioStats, max_voices: u16) -> QualityVerdict {
    let mut verdict = QualityVerdict::default();
    verdict.gates.push(gate(
        "true_peak_milli",
        stats.true_peak_milli <= 1_000,
        i32::from(stats.true_peak_milli),
        1_000,
    ));
    verdict.gates.push(gate(
        "missing_cues",
        stats.missing_cues == 0,
        stats.missing_cues as i32,
        0,
    ));
    verdict.gates.push(gate(
        "voice_concurrency",
        stats.voices <= max_voices,
        i32::from(stats.voices),
        i32::from(max_voices),
    ));
    verdict.gates.push(gate(
        "subtitle_alignment_ms",
        stats.subtitle_alignment_ms <= 80,
        stats.subtitle_alignment_ms as i32,
        80,
    ));
    verdict
}

/// Locale / a11y capture matrix: overflow, overlap, focus.
pub fn loc_a11y_gates(report: &CaptureReport) -> QualityVerdict {
    let mut verdict = QualityVerdict::default();
    for (i, cell) in report.cells.iter().enumerate() {
        push_cell(&mut verdict, i, cell);
    }
    verdict
}

/// Evaluate one seeded loc/a11y cell.
pub fn loc_a11y_cell(cell: &CaptureCell) -> QualityVerdict {
    let mut verdict = QualityVerdict::default();
    push_cell(&mut verdict, 0, cell);
    verdict
}

fn push_cell(verdict: &mut QualityVerdict, index: usize, cell: &CaptureCell) {
    verdict.gates.push(gate(
        &format!("overflow.{index}"),
        cell.overflow == 0,
        cell.overflow as i32,
        0,
    ));
    verdict.gates.push(gate(
        &format!("overlap.{index}"),
        cell.overlap == 0,
        cell.overlap as i32,
        0,
    ));
    verdict.gates.push(gate(
        &format!("focus.{index}"),
        cell.focus_complete,
        i32::from(cell.focus_complete),
        1,
    ));
}

fn gate(id: &str, passed: bool, used: i32, cap: i32) -> GateResult {
    GateResult {
        id: Name::from(id),
        passed,
        used,
        cap,
    }
}

/// Tiny synthetic frame used by seeded visual faults.
#[must_use]
pub fn test_frame(rgb: [u8; 3]) -> RgbaFrame {
    RgbaFrame::solid(16, 16, [rgb[0], rgb[1], rgb[2], 255])
}
