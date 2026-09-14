//! Fail-closed live-service errors. These are not kernel reject reasons.

use core::fmt;

/// Why a service-plane operation was refused.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum LiveError {
    /// The selected multiplayer profile is incomplete or unapproved.
    Profile(String),
    /// Matchmaking, lobby, session, or reconnect failure.
    Session(String),
    /// Signed configuration failed validation or authentication.
    Config(String),
    /// Network or service-failure matrix did not meet its declared envelope.
    Gate(String),
    /// Moderation or privacy policy failure.
    Safety(String),
    /// Evidence binding or claim-level failure.
    Evidence(String),
    /// Deployment or rollback rehearsal failure.
    Deploy(String),
}

impl fmt::Display for LiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Profile(message) => write!(f, "Profile({message})"),
            Self::Session(message) => write!(f, "Session({message})"),
            Self::Config(message) => write!(f, "Config({message})"),
            Self::Gate(message) => write!(f, "Gate({message})"),
            Self::Safety(message) => write!(f, "Safety({message})"),
            Self::Evidence(message) => write!(f, "Evidence({message})"),
            Self::Deploy(message) => write!(f, "Deploy({message})"),
        }
    }
}

impl core::error::Error for LiveError {}
