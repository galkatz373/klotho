//! Read-only anti-cheat integration, moderation, and privacy boundaries.

use std::collections::{BTreeMap, BTreeSet};

use klotho_core::Hash;
use klotho_net::SidecarFlag;
use serde::{Deserialize, Serialize};

use crate::LiveError;

/// Service action derived from the existing read-only network sidecar.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum AntiCheatAction {
    /// Accept the intent.
    Allow,
    /// Nack stale Fire through the normal server path.
    RejectStaleIntent,
    /// Disconnect after a bounded analog or command-rate violation.
    Disconnect,
}

/// Translate a sidecar observation without access to World or Projection.
#[must_use]
pub const fn sidecar_action(flag: SidecarFlag) -> AntiCheatAction {
    match flag {
        SidecarFlag::None => AntiCheatAction::Allow,
        SidecarFlag::StaleFire => AntiCheatAction::RejectStaleIntent,
        SidecarFlag::AnalogRange | SidecarFlag::CmdRate => AntiCheatAction::Disconnect,
    }
}

/// Moderation report category. Exploit details belong in P1 evidence.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModerationCategory {
    /// Abusive communication.
    Harassment,
    /// Deliberate match disruption.
    Griefing,
    /// Suspected cheating.
    Cheating,
    /// Unsafe user-generated identifier/content.
    Content,
}

/// Privacy-bounded moderation submission.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModerationReport {
    /// Stable report id.
    pub id: u64,
    /// Pseudonymous reporter.
    pub reporter: Hash,
    /// Pseudonymous subject.
    pub subject: Hash,
    /// Category.
    pub category: ModerationCategory,
    /// Commitment to protected evidence stored in a private service.
    pub evidence_commitment: Hash,
    /// Retention requested for protected evidence.
    pub retention_days: u16,
}

/// Allowed telemetry/moderation data policy.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrivacyPolicy {
    /// Maximum protected-data retention.
    pub max_retention_days: u16,
    /// Minimum cohort for authoring telemetry.
    pub minimum_cohort: u32,
    /// Explicit aggregate field allowlist.
    pub aggregate_fields: Vec<String>,
}

impl PrivacyPolicy {
    /// Validate bounded retention and aggregate-only fields.
    pub fn validate(&self) -> Result<(), LiveError> {
        if self.max_retention_days == 0 || self.minimum_cohort < 10 {
            return Err(LiveError::Safety("privacy limits are invalid".into()));
        }
        let mut fields = BTreeSet::new();
        for field in &self.aggregate_fields {
            let lower = field.to_ascii_lowercase();
            if field.trim().is_empty()
                || lower.contains("email")
                || lower.contains("ip_address")
                || lower.contains("raw_chat")
                || !fields.insert(field)
            {
                return Err(LiveError::Safety(
                    "privacy allowlist contains PII or duplicates".into(),
                ));
            }
        }
        if fields.is_empty() {
            return Err(LiveError::Safety(
                "privacy aggregate allowlist is empty".into(),
            ));
        }
        Ok(())
    }
}

/// Stable moderation queue. Resolution is an out-of-band service action and
/// never a game-state write.
#[derive(Clone, Eq, PartialEq, Debug, Default)]
pub struct ModerationQueue {
    pending: BTreeMap<u64, ModerationReport>,
    resolved: BTreeSet<u64>,
}

impl ModerationQueue {
    /// Submit a report under the privacy policy.
    pub fn submit(
        &mut self,
        report: ModerationReport,
        policy: &PrivacyPolicy,
    ) -> Result<(), LiveError> {
        policy.validate()?;
        if report.reporter == Hash::ZERO
            || report.subject == Hash::ZERO
            || report.evidence_commitment == Hash::ZERO
            || report.reporter == report.subject
            || report.retention_days == 0
            || report.retention_days > policy.max_retention_days
            || self.pending.contains_key(&report.id)
            || self.resolved.contains(&report.id)
        {
            return Err(LiveError::Safety("moderation report is invalid".into()));
        }
        self.pending.insert(report.id, report);
        Ok(())
    }

    /// Resolve one report by id. Case contents are deliberately absent.
    pub fn resolve(&mut self, id: u64) -> Result<(), LiveError> {
        self.pending
            .remove(&id)
            .ok_or_else(|| LiveError::Safety("moderation report is not pending".into()))?;
        self.resolved.insert(id);
        Ok(())
    }

    /// Pending count.
    #[must_use]
    pub fn pending(&self) -> usize {
        self.pending.len()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn source_has_no_authoritative_write_path() {
        let source = include_str!("safety.rs");
        for forbidden in [
            concat!("World", "Mut"),
            concat!("Commit", "Kernel"),
            concat!(".ing", "est("),
            concat!("Trace", "Log"),
        ] {
            assert!(!source.contains(forbidden));
        }
    }
}
