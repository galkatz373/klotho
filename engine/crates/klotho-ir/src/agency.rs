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

impl Channel {
    /// Frozen wire discriminant. Append-only; never reuse or reorder a value:
    /// Trace shards, warp shards, and net packets all decode it.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        match self {
            Self::Timing => 1,
            Self::Aim => 2,
            Self::ResourceSpend => 3,
            Self::DialogueChoice => 4,
        }
    }

    /// Inverse of [`Self::as_u8`]. `None` for unknown future values.
    #[must_use]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Self::Timing),
            2 => Some(Self::Aim),
            3 => Some(Self::ResourceSpend),
            4 => Some(Self::DialogueChoice),
            _ => None,
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_discriminants_are_frozen() {
        assert_eq!(Channel::Timing.as_u8(), 1);
        assert_eq!(Channel::Aim.as_u8(), 2);
        assert_eq!(Channel::ResourceSpend.as_u8(), 3);
        assert_eq!(Channel::DialogueChoice.as_u8(), 4);
        assert_eq!(Channel::from_u8(1), Some(Channel::Timing));
        assert_eq!(Channel::from_u8(4), Some(Channel::DialogueChoice));
        assert_eq!(Channel::from_u8(0), None);
        assert_eq!(Channel::from_u8(5), None);
    }
}
