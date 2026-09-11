//! Automated player. Public device actions only (K74, K87).

use std::collections::BTreeSet;

use klotho_core::PlayerId;
use klotho_input::Button;
use klotho_ir::IntentTarget;

use crate::error::EvalError;
use crate::host::JourneyHost;
use crate::ids::JourneyId;
use crate::journey::DeviceAction;

/// Journeys whose reachability is a hard gate for the bot.
#[derive(Clone, Debug, Default)]
pub struct BotManifest {
    /// Declared capabilities.
    pub capabilities: BTreeSet<JourneyId>,
}

impl BotManifest {
    /// `true` when failure to solve `id` is a hard gate.
    #[must_use]
    pub fn is_hard_gate(&self, id: &JourneyId) -> bool {
        self.capabilities.contains(id)
    }
}

/// Test agent. Cannot construct [`klotho_ir::Agency`] or write Projection.
#[derive(Clone, Debug, Default)]
pub struct AutomatedPlayer {
    player: PlayerId,
}

impl AutomatedPlayer {
    /// Local player slot.
    #[must_use]
    pub fn new(player: PlayerId) -> Self {
        Self { player }
    }

    /// Hold a button at `target`.
    #[must_use]
    pub fn press(&self, button: Button, target: IntentTarget) -> DeviceAction {
        DeviceAction::press(self.player, button, target)
    }

    /// Idle look.
    #[must_use]
    pub fn look(&self) -> DeviceAction {
        DeviceAction::new(self.player)
    }

    /// Bounded public-input search. Advisory unless `id` is in `manifest`.
    pub fn search<H: JourneyHost + Clone>(
        &self,
        host: &H,
        actions: &[DeviceAction],
        max_ticks: u32,
        id: &JourneyId,
        manifest: &BotManifest,
        goal: impl Fn(&H) -> bool,
    ) -> Result<Vec<DeviceAction>, EvalError> {
        let _ = max_ticks;
        let mut path = Vec::new();
        let mut cur = host.clone();
        for action in actions {
            cur.apply_device(action)?;
            path.push(action.clone());
            if goal(&cur) {
                return Ok(path);
            }
        }
        if manifest.is_hard_gate(id) {
            Err(EvalError::Unreachable {
                journey: id.clone(),
                last_state: cur.last_state(),
                blocked: cur.blocked_affordance(),
            })
        } else {
            Ok(path)
        }
    }
}
