//! Player-only agency claims (K10).

use serde::{Deserialize, Serialize};

use crate::error::IrError;

/// Skill channel a player may claim on a packet.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
#[repr(u8)]
pub enum Channel {
    /// `WAIT { channel: Timing }` resume.
    Timing = 1,
    /// Aim window (Ash).
    Aim = 2,
    /// Resource spend confirmation.
    ResourceSpend = 3,
    /// Trade accept/refuse.
    DialogueChoice = 4,
}

/// Assist level. Hearth ships with no assist laws; only `None` is valid in v1.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default, Serialize, Deserialize)]
pub enum AssistLevel {
    /// No assist. Default.
    #[default]
    None,
}

/// Claims attached to [`crate::PlayerIntent`] only.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Default, Serialize, Deserialize)]
pub struct Agency {
    /// Channels this packet claims. Duplicates fail validate.
    pub claimed: Vec<Channel>,
    /// Assist. v1 Hearth: [`AssistLevel::None`].
    pub assist: AssistLevel,
}

impl Agency {
    /// Empty claims, no assist.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// `true` if `ch` is in `claimed`.
    #[must_use]
    pub fn claims(&self, ch: Channel) -> bool {
        self.claimed.contains(&ch)
    }

    pub(crate) fn check(&self) -> Result<(), IrError> {
        let mut seen = [false; 5];
        for ch in &self.claimed {
            let i = *ch as usize;
            if seen[i] {
                return Err(IrError::DuplicateChannel);
            }
            seen[i] = true;
        }
        Ok(())
    }
}
