//! Human-approved signed economy, balance, and experiment configuration.

use std::collections::{BTreeMap, BTreeSet};

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use klotho_core::Hash;
use klotho_prove::hash_bytes;
use serde::{Deserialize, Serialize};

use crate::LiveError;

/// Human approval role for a live configuration.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigRole {
    /// Economy/balance owner.
    Design,
    /// Privacy owner for telemetry and assignment.
    Privacy,
    /// Operations owner for rollout and rollback.
    Operations,
}

/// Human or forbidden agent principal.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigPrincipal {
    /// Named human.
    Human(String),
    /// Model/agent identity. Cannot approve.
    Agent(String),
}

/// Approval bound to the unsigned config hash.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigApproval {
    /// Required role.
    pub role: ConfigRole,
    /// Named principal.
    pub principal: ConfigPrincipal,
    /// Exact config hash.
    pub config_hash: Hash,
}

/// Versioned integer economy. Values are presentation/store quantities; a
/// gameplay-affecting change still requires a separately approved Canon epoch.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EconomyConfig {
    /// Stable config id.
    pub id: String,
    /// Monotonic version.
    pub version: u64,
    /// Currency/item values in smallest integer units.
    pub values: BTreeMap<String, i64>,
    /// Canon epoch hash required before gameplay-affecting values are exposed.
    pub canon_epoch: Hash,
}

/// One deterministic experiment declaration.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Experiment {
    /// Stable id.
    pub id: String,
    /// Variant labels in bucket order.
    pub variants: Vec<String>,
    /// Permille weights; must total 1000.
    pub weights_permille: Vec<u16>,
}

/// Entire signed service configuration.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveConfig {
    /// Economy and balance values.
    pub economy: EconomyConfig,
    /// Experiments. Assignment is observational and never writes Projection.
    pub experiments: Vec<Experiment>,
}

impl LiveConfig {
    /// Structural and bounded validation.
    pub fn validate(&self) -> Result<(), LiveError> {
        if self.economy.id.trim().is_empty()
            || self.economy.version == 0
            || self.economy.values.is_empty()
            || self.economy.canon_epoch == Hash::ZERO
        {
            return Err(LiveError::Config(
                "economy configuration is incomplete".into(),
            ));
        }
        if self.economy.values.keys().any(|key| key.trim().is_empty()) {
            return Err(LiveError::Config("economy key is empty".into()));
        }
        let mut ids = BTreeSet::new();
        for experiment in &self.experiments {
            if experiment.id.trim().is_empty()
                || !ids.insert(&experiment.id)
                || experiment.variants.len() < 2
                || experiment.variants.len() != experiment.weights_permille.len()
                || experiment
                    .variants
                    .iter()
                    .any(|variant| variant.trim().is_empty())
                || experiment
                    .weights_permille
                    .iter()
                    .map(|weight| u32::from(*weight))
                    .sum::<u32>()
                    != 1_000
            {
                return Err(LiveError::Config(
                    "experiment declaration is invalid".into(),
                ));
            }
        }
        Ok(())
    }

    /// Canonical content hash over sorted map order and declared vector order.
    #[must_use]
    pub fn content_hash(&self) -> Hash {
        let mut bytes = Vec::new();
        put_str(&mut bytes, &self.economy.id);
        bytes.extend_from_slice(&self.economy.version.to_le_bytes());
        bytes.extend_from_slice(self.economy.canon_epoch.as_bytes());
        bytes.extend_from_slice(&(self.economy.values.len() as u32).to_le_bytes());
        for (key, value) in &self.economy.values {
            put_str(&mut bytes, key);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&(self.experiments.len() as u32).to_le_bytes());
        for experiment in &self.experiments {
            put_str(&mut bytes, &experiment.id);
            bytes.extend_from_slice(&(experiment.variants.len() as u32).to_le_bytes());
            for (variant, weight) in experiment.variants.iter().zip(&experiment.weights_permille) {
                put_str(&mut bytes, variant);
                bytes.extend_from_slice(&weight.to_le_bytes());
            }
        }
        hash_bytes(&bytes)
    }
}

fn put_str(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
    bytes.extend_from_slice(value.as_bytes());
}

/// Offline human signing authority. Production keys live outside agent access.
pub struct ConfigAuthority {
    signing: SigningKey,
}

impl ConfigAuthority {
    /// Construct from externally supplied secret bytes.
    #[must_use]
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self {
            signing: SigningKey::from_bytes(&bytes),
        }
    }

    /// Sign only after all independent human approvals match the config hash.
    pub fn sign(
        &self,
        config: LiveConfig,
        approvals: &[ConfigApproval],
    ) -> Result<SignedLiveConfig, LiveError> {
        config.validate()?;
        let hash = config.content_hash();
        check_approvals(approvals, hash)?;
        let signature = self.signing.sign(hash.as_bytes()).to_bytes();
        Ok(SignedLiveConfig {
            config,
            hash,
            signature,
            verifying_key: self.signing.verifying_key().to_bytes(),
        })
    }
}

fn check_approvals(approvals: &[ConfigApproval], hash: Hash) -> Result<(), LiveError> {
    let mut roles = BTreeSet::new();
    let mut humans = BTreeSet::new();
    for approval in approvals {
        let ConfigPrincipal::Human(name) = &approval.principal else {
            return Err(LiveError::Config("agent cannot approve live config".into()));
        };
        if name.trim().is_empty()
            || approval.config_hash != hash
            || !roles.insert(approval.role)
            || !humans.insert(name)
        {
            return Err(LiveError::Config("live config approval is invalid".into()));
        }
    }
    for role in [
        ConfigRole::Design,
        ConfigRole::Privacy,
        ConfigRole::Operations,
    ] {
        if !roles.contains(&role) {
            return Err(LiveError::Config(
                "live config approval role is missing".into(),
            ));
        }
    }
    Ok(())
}

/// Authenticated immutable live configuration.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct SignedLiveConfig {
    /// Parsed config.
    pub config: LiveConfig,
    /// Canonical content hash.
    pub hash: Hash,
    /// Ed25519 signature over `hash`.
    pub signature: [u8; 64],
    /// Verifying key.
    pub verifying_key: [u8; 32],
}

impl SignedLiveConfig {
    /// Verify structure, content hash, key, and signature.
    pub fn verify(&self) -> Result<(), LiveError> {
        self.config.validate()?;
        if self.config.content_hash() != self.hash {
            return Err(LiveError::Config("live config hash drifted".into()));
        }
        let key = VerifyingKey::from_bytes(&self.verifying_key)
            .map_err(|_| LiveError::Config("live config key is invalid".into()))?;
        if key.is_weak() {
            return Err(LiveError::Config("live config key is weak".into()));
        }
        key.verify_strict(
            self.hash.as_bytes(),
            &Signature::from_bytes(&self.signature),
        )
        .map_err(|_| LiveError::Config("live config signature is invalid".into()))
    }

    /// Deterministically assign a pseudonymous account to an experiment.
    pub fn assign(
        &self,
        experiment_id: &str,
        pseudonymous_account: Hash,
    ) -> Result<ExperimentAssignment, LiveError> {
        self.verify()?;
        let experiment = self
            .config
            .experiments
            .iter()
            .find(|experiment| experiment.id == experiment_id)
            .ok_or_else(|| LiveError::Config("experiment is not declared".into()))?;
        let mut input = Vec::new();
        input.extend_from_slice(self.hash.as_bytes());
        input.extend_from_slice(pseudonymous_account.as_bytes());
        put_str(&mut input, experiment_id);
        let digest = hash_bytes(&input);
        let bucket = u16::from_le_bytes([digest.0[0], digest.0[1]]) % 1_000;
        let mut ceiling = 0u16;
        for (variant, weight) in experiment.variants.iter().zip(&experiment.weights_permille) {
            ceiling = ceiling.saturating_add(*weight);
            if bucket < ceiling {
                return Ok(ExperimentAssignment {
                    experiment: experiment.id.clone(),
                    variant: variant.clone(),
                    bucket,
                    config_hash: self.hash,
                });
            }
        }
        Err(LiveError::Config(
            "experiment weights left a bucket gap".into(),
        ))
    }
}

/// Observational A/B assignment. It is not Canon, Trace, or Projection.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct ExperimentAssignment {
    /// Experiment id.
    pub experiment: String,
    /// Selected variant.
    pub variant: String,
    /// Stable bucket 0..999.
    pub bucket: u16,
    /// Signed config identity.
    pub config_hash: Hash,
}

/// Aggregated/redacted telemetry accepted by the authoring feedback plane.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TelemetryAggregate {
    /// Metric id.
    pub metric: String,
    /// Cohort size; raw player rows are forbidden.
    pub cohort: u32,
    /// Integer aggregate in the metric's declared unit.
    pub value: i64,
    /// Experiment/config identity.
    pub config_hash: Hash,
}

impl TelemetryAggregate {
    /// Require a privacy-preserving cohort and non-empty metric.
    pub fn validate(&self, minimum_cohort: u32) -> Result<(), LiveError> {
        if self.metric.trim().is_empty()
            || self.cohort < minimum_cohort
            || self.config_hash == Hash::ZERO
        {
            return Err(LiveError::Config(
                "telemetry aggregate is not privacy bounded".into(),
            ));
        }
        Ok(())
    }
}

/// Current and previous signed configuration for atomic rollback.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct LiveConfigSet {
    /// Current config.
    pub current: SignedLiveConfig,
    /// Previous config retained for rollback.
    pub previous: Option<SignedLiveConfig>,
}

impl LiveConfigSet {
    /// Start from a verified config.
    pub fn new(current: SignedLiveConfig) -> Result<Self, LiveError> {
        current.verify()?;
        Ok(Self {
            current,
            previous: None,
        })
    }

    /// Atomically install a newer verified version.
    pub fn install(&mut self, next: SignedLiveConfig) -> Result<(), LiveError> {
        next.verify()?;
        if next.config.economy.version <= self.current.config.economy.version {
            return Err(LiveError::Config(
                "live config version did not increase".into(),
            ));
        }
        let previous = core::mem::replace(&mut self.current, next);
        self.previous = Some(previous);
        Ok(())
    }

    /// Restore the previous whole config. Partial value merges do not exist.
    pub fn rollback(&mut self) -> Result<Hash, LiveError> {
        let previous = self
            .previous
            .take()
            .ok_or_else(|| LiveError::Config("no previous live config".into()))?;
        self.current = previous;
        Ok(self.current.hash)
    }
}
