//! Multiplayer and live-service production boundary (KAI-24).
//!
//! This crate owns out-of-band matchmaking, signed configuration, moderation,
//! network-emulation gates, and deployment rehearsal. It has no world or commit
//! dependency. Assignments and telemetry are observations only: they never
//! mutate Projection, append Trace, or mint player Agency.
//!
//! Public adapters and fixtures prove P0 only. P1 requires signed confidential
//! service/security/moderation/scale evidence; P2 requires external acceptance.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod config;
mod error;
mod evidence;
mod matchmaker;
mod ops;
mod profile;
mod safety;

pub use config::{
    ConfigApproval, ConfigAuthority, ConfigPrincipal, ConfigRole, EconomyConfig, Experiment,
    ExperimentAssignment, LiveConfig, LiveConfigSet, SignedLiveConfig, TelemetryAggregate,
};
pub use error::LiveError;
pub use evidence::{
    EvidenceLevel, ExternalAcceptance, PrivateEvidence, PublicEvidence, achieved_level,
    forbid_overclaim,
};
pub use matchmaker::{
    Lobby, Matchmaker, PlayerTicket, ReconnectGrant, RegionStatus, ServiceSession,
};
pub use ops::{
    Deployment, FailureKind, IncidentRunbook, MatrixReport, NetworkCase, NetworkMatrix, PublicGate,
    Rehearsal,
};
pub use profile::{ApprovalRole, MultiplayerProfile, ProfileApproval, ReplicatedEncounterPattern};
pub use safety::{
    AntiCheatAction, ModerationCategory, ModerationQueue, ModerationReport, PrivacyPolicy,
    sidecar_action,
};
