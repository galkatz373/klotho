//! Analog fields on [`crate::PlayerIntent`]: phase, stick, look delta.

use serde::{Deserialize, Serialize};

use klotho_core::YawMd;

use crate::error::IrError;

/// Timing phase, stick, look delta (millidegrees).
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default, Serialize, Deserialize)]
pub struct Analog {
    /// Per-mille of the current WAIT window, `0..=1000`. Ignored when no WAIT.
    pub phase: u16,
    /// Stick X, signed. Runtime maps to a proposed vel; this is not committed vel.
    pub stick_x: i16,
    /// Stick Z (forward), signed.
    pub stick_z: i16,
    /// Look yaw delta this tick, millidegrees.
    pub look_yaw: YawMd,
    /// Look pitch delta this tick, millidegrees. Runtime clamps.
    pub look_pitch: i32,
}

impl Analog {
    pub(crate) fn check(&self) -> Result<(), IrError> {
        if self.phase > 1000 {
            Err(IrError::InvalidPhase(self.phase))
        } else {
            Ok(())
        }
    }
}
