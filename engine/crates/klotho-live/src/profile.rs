//! Approved multiplayer title profile and replicated encounter pattern.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::LiveError;

/// Human discipline required to select a multiplayer title profile.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalRole {
    /// Game-design owner.
    Design,
    /// Networking owner.
    Network,
    /// Security owner.
    Security,
    /// Privacy/moderation owner.
    Trust,
    /// Release/operations owner.
    Release,
}

/// Named human approval of the selected profile.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileApproval {
    /// Discipline.
    pub role: ApprovalRole,
    /// Named human owner. Empty names and agent principals fail.
    pub human: String,
}

/// Explicit post-title multiplayer profile. It does not alter the frozen
/// `RuntimeProfile::AaaAdventure` first-title default.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MultiplayerProfile {
    /// Stable profile id.
    pub id: String,
    /// Authoritative simulation rate.
    pub auth_hz: u16,
    /// Signed intent rate.
    pub intent_hz: u16,
    /// Declared lobby cap.
    pub max_players: u16,
    /// Minimum players needed to form a lobby.
    pub min_players: u16,
    /// Maximum accepted RTT.
    pub max_rtt_ms: u16,
    /// Maximum accepted packet loss in permille.
    pub max_loss_permille: u16,
    /// Bounded rewind window.
    pub rewind_ticks: u16,
    /// Reconnect grant lifetime.
    pub reconnect_seconds: u32,
    /// Ordered deployment regions.
    pub regions: Vec<String>,
    /// Ordered platform pool.
    pub platforms: Vec<String>,
    /// Required named approvals.
    pub approvals: Vec<ProfileApproval>,
}

impl MultiplayerProfile {
    /// Validate the declared profile and its five independent human owners.
    pub fn validate(&self) -> Result<(), LiveError> {
        if self.id.trim().is_empty() {
            return Err(LiveError::Profile("profile id is empty".into()));
        }
        if self.auth_hz != 30 && self.auth_hz != 60 {
            return Err(LiveError::Profile("auth_hz must be 30 or 60".into()));
        }
        if self.intent_hz == 0 || self.intent_hz > self.auth_hz {
            return Err(LiveError::Profile("intent_hz exceeds auth_hz".into()));
        }
        if self.min_players < 2 || self.min_players > self.max_players || self.max_players > 64 {
            return Err(LiveError::Profile("player envelope is invalid".into()));
        }
        if self.max_rtt_ms == 0
            || self.max_loss_permille > 1_000
            || self.rewind_ticks == 0
            || self.reconnect_seconds == 0
        {
            return Err(LiveError::Profile("network envelope is invalid".into()));
        }
        if self.regions.is_empty() || self.platforms.is_empty() {
            return Err(LiveError::Profile("region/platform matrix is empty".into()));
        }
        unique_nonempty("regions", &self.regions)?;
        unique_nonempty("platforms", &self.platforms)?;
        let mut roles = BTreeSet::new();
        let mut humans = BTreeSet::new();
        for approval in &self.approvals {
            let human = approval.human.trim();
            if human.is_empty() || human.to_ascii_lowercase().contains("agent") {
                return Err(LiveError::Profile(
                    "profile approval is not a named human".into(),
                ));
            }
            if !roles.insert(approval.role) || !humans.insert(human) {
                return Err(LiveError::Profile(
                    "profile approvals are not independent".into(),
                ));
            }
        }
        let required = [
            ApprovalRole::Design,
            ApprovalRole::Network,
            ApprovalRole::Security,
            ApprovalRole::Trust,
            ApprovalRole::Release,
        ];
        if required.iter().any(|role| !roles.contains(role)) {
            return Err(LiveError::Profile(
                "profile approval role is missing".into(),
            ));
        }
        Ok(())
    }
}

fn unique_nonempty(label: &str, values: &[String]) -> Result<(), LiveError> {
    let mut seen = BTreeSet::new();
    for value in values {
        if value.trim().is_empty() || !seen.insert(value) {
            return Err(LiveError::Profile(format!(
                "{label} must be unique and non-empty"
            )));
        }
    }
    Ok(())
}

/// Typed authoring/evaluation pattern for a replicated encounter. It expands
/// through ordinary Canon/Rites in title authoring; this service record only
/// declares replication and balance acceptance.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplicatedEncounterPattern {
    /// Stable pattern id.
    pub id: String,
    /// Maximum concurrent players covered by evidence.
    pub players: u16,
    /// Integer balance knobs; no float reaches the commit path.
    pub balance: std::collections::BTreeMap<String, i32>,
    /// Required public-input journey ids.
    pub journeys: Vec<String>,
    /// Required network matrix case ids.
    pub network_cases: Vec<String>,
}

impl ReplicatedEncounterPattern {
    /// Validate against the selected profile.
    pub fn validate(&self, profile: &MultiplayerProfile) -> Result<(), LiveError> {
        profile.validate()?;
        if self.id.trim().is_empty() || self.players < 2 || self.players > profile.max_players {
            return Err(LiveError::Profile(
                "replicated encounter player envelope is invalid".into(),
            ));
        }
        if self.balance.is_empty() || self.journeys.is_empty() || self.network_cases.is_empty() {
            return Err(LiveError::Profile(
                "replicated encounter acceptance is incomplete".into(),
            ));
        }
        if self.balance.keys().any(|key| key.trim().is_empty())
            || self.journeys.iter().any(|id| id.trim().is_empty())
            || self.network_cases.iter().any(|id| id.trim().is_empty())
        {
            return Err(LiveError::Profile(
                "replicated encounter contains an empty id".into(),
            ));
        }
        Ok(())
    }
}
