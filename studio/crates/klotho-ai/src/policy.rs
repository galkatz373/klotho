//! Closed capabilities, disclosure policy, and the secret boundary.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::agent::AgentRole;
use crate::error::AiError;

/// Capabilities exposed by the authoring protocol. There is deliberately no
/// shell, arbitrary path, Trace append, release, credential, or epoch action.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// Read a bounded semantic project description.
    ProjectDescribe,
    /// Query the generated schema catalog.
    SchemaQuery,
    /// Create an isolated change.
    ChangeCreate,
    /// Apply typed semantic operations.
    ChangeApply,
    /// Inspect a semantic diff.
    ChangeDiff,
    /// Search registered patterns.
    PatternSearch,
    /// Run structural validation.
    ValidateRun,
    /// Read sealed evidence.
    EvidenceRead,
    /// Submit a candidate to human review.
    ChangeSubmit,
}

impl Capability {
    /// Complete closed set.
    pub const ALL: [Self; 9] = [
        Self::ProjectDescribe,
        Self::SchemaQuery,
        Self::ChangeCreate,
        Self::ChangeApply,
        Self::ChangeDiff,
        Self::PatternSearch,
        Self::ValidateRun,
        Self::EvidenceRead,
        Self::ChangeSubmit,
    ];
}

/// Classes of data that may cross a model boundary.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextClass {
    /// Generated public schema.
    Schema,
    /// Project anchors and structural facts.
    ProjectStructure,
    /// Human-approved memory tied to source hashes.
    ApprovedMemory,
    /// The creative request text.
    RequestText,
}

/// Per-request disclosure rule.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum DisclosurePolicy {
    /// Nothing may leave the local machine.
    LocalOnly,
    /// Only named classes may be sent to a remote backend.
    RemoteAllow(BTreeSet<ContextClass>),
}

impl DisclosurePolicy {
    /// Whether `class` may be disclosed to a remote backend.
    #[must_use]
    pub fn permits_remote(&self, class: ContextClass) -> bool {
        matches!(self, Self::RemoteAllow(classes) if classes.contains(&class))
    }
}

/// Capabilities granted to one worker role.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolProfile {
    /// Worker role.
    pub role: AgentRole,
    /// Closed allowed set.
    pub capabilities: BTreeSet<Capability>,
}

impl ToolProfile {
    /// Standard least-privilege profile for a role.
    #[must_use]
    pub fn standard(role: AgentRole) -> Self {
        use Capability as C;
        let capabilities = match role {
            AgentRole::Planner => [C::ProjectDescribe, C::SchemaQuery, C::PatternSearch]
                .into_iter()
                .collect(),
            AgentRole::Gameplay | AgentRole::World | AgentRole::Narrative => [
                C::ProjectDescribe,
                C::SchemaQuery,
                C::PatternSearch,
                C::ChangeCreate,
                C::ChangeApply,
                C::ChangeDiff,
                C::ValidateRun,
            ]
            .into_iter()
            .collect(),
            AgentRole::Test | AgentRole::Critic => {
                [C::ProjectDescribe, C::EvidenceRead].into_iter().collect()
            }
            AgentRole::Asset => [C::ProjectDescribe, C::SchemaQuery].into_iter().collect(),
            AgentRole::Optimizer => [C::ProjectDescribe, C::EvidenceRead].into_iter().collect(),
        };
        Self { role, capabilities }
    }

    /// Fail closed if a capability was not granted.
    pub fn require(&self, capability: Capability) -> Result<(), AiError> {
        if self.capabilities.contains(&capability) {
            Ok(())
        } else {
            Err(AiError::CapabilityDenied(capability))
        }
    }
}

/// Global execution policy.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct ExecutionPolicy {
    /// Maximum encoded backend request.
    pub max_request_bytes: u64,
    /// Maximum encoded backend response.
    pub max_response_bytes: u64,
    /// Registered profiles by role.
    pub profiles: BTreeMap<AgentRole, ToolProfile>,
}

impl Default for ExecutionPolicy {
    fn default() -> Self {
        let profiles = AgentRole::ALL
            .into_iter()
            .map(|role| (role, ToolProfile::standard(role)))
            .collect();
        Self {
            max_request_bytes: 2 * 1024 * 1024,
            max_response_bytes: 2 * 1024 * 1024,
            profiles,
        }
    }
}

/// Opaque credential store. Values cannot be serialized, enumerated, placed in
/// context, or read through an agent tool.
#[derive(Default)]
pub struct SecretStore {
    values: BTreeMap<String, Vec<u8>>,
}

impl SecretStore {
    /// Install a host-owned credential.
    pub fn insert(&mut self, key: impl Into<String>, value: Vec<u8>) {
        self.values.insert(key.into(), value);
    }

    /// Number of installed credentials; values and names remain opaque.
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// True when no credentials are installed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}
