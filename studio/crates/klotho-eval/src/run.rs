//! Run a journey spec against a host.

use crate::error::EvalError;
use crate::evidence::{CheckLayer, EvidenceBuilder, EvidenceBundle};
use crate::host::JourneyHost;
use crate::journey::{CaptureKind, CapturePoint, JourneySpec, JourneyStep};
use klotho_core::Hash;
use klotho_ir::Name;
use klotho_prove::hash_bytes;

/// Result of a successful run.
#[derive(Clone, Debug)]
pub struct JourneyResult {
    /// Sealed evidence.
    pub evidence: EvidenceBundle,
    /// Ticks consumed.
    pub ticks: u32,
}

/// Execute `spec` on `host`. Captures fire after their step index.
pub fn run_journey<H: JourneyHost>(
    host: &mut H,
    spec: &JourneySpec,
    change: Hash,
) -> Result<JourneyResult, EvalError> {
    let mut ticks = 0u32;
    for (i, step) in spec.steps.iter().enumerate() {
        if ticks > spec.max_ticks {
            return Err(EvalError::Budget {
                used: ticks,
                cap: spec.max_ticks,
            });
        }
        match step {
            JourneyStep::Device { action } => {
                host.apply_device(action)?;
            }
            JourneyStep::Fixture {
                player,
                verb,
                target,
                analog,
            } => {
                host.apply_fixture(*player, *verb, target.clone(), *analog)?;
            }
            JourneyStep::Wait { ticks: n } => {
                host.wait(*n)?;
            }
            JourneyStep::Camera { name } => {
                host.camera(name)?;
            }
            JourneyStep::Save { slot } => host.save(slot)?,
            JourneyStep::Load { slot } => host.load(slot)?,
        }
        ticks = host.ticks();
        fire_captures(host, spec, i as u32)?;
    }
    fire_captures(host, spec, u32::MAX)?;
    for assertion in &spec.assertions {
        if let Err(err) = host.check(assertion) {
            return Err(match err {
                EvalError::Unreachable {
                    last_state,
                    blocked,
                    ..
                } => EvalError::Unreachable {
                    journey: spec.id.clone(),
                    last_state,
                    blocked,
                },
                other => other,
            });
        }
    }
    if host.ticks() > spec.max_ticks {
        return Err(EvalError::Budget {
            used: host.ticks(),
            cap: spec.max_ticks,
        });
    }
    let ctx = host.evidence_context(change);
    let mut builder = EvidenceBuilder::new(ctx);
    builder.record_check(
        CheckLayer::Journey,
        Name::from(spec.id.as_str()),
        true,
        hash_bytes(spec.id.as_str().as_bytes()),
    );
    let evidence = builder.seal(None)?;
    Ok(JourneyResult {
        evidence,
        ticks: host.ticks(),
    })
}

fn fire_captures<H: JourneyHost>(
    host: &mut H,
    spec: &JourneySpec,
    after: u32,
) -> Result<(), EvalError> {
    for point in &spec.capture_points {
        if point.after_step == after {
            host.capture(point)?;
        }
    }
    Ok(())
}

/// Capture point at end of journey.
#[must_use]
pub fn end_capture(name: &str) -> CapturePoint {
    CapturePoint {
        name: Name::from(name),
        after_step: u32::MAX,
        kind: CaptureKind::Semantic,
    }
}
