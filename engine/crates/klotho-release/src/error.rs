//! Fail-closed release and platform-service errors. Not `RejectReason`.

use core::fmt;

use klotho_compile::CompileError;
use klotho_save::SaveError;
use klotho_world::SnapError;

/// Why a release factory or platform adapter refused to proceed.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum ReleaseError {
    /// Package, rights, or privacy scan failed.
    Scan(String),
    /// Named human approval missing, duplicated, or bound to the wrong package.
    Approval(String),
    /// Signing authority rejected or signature did not verify.
    Signature(String),
    /// Save identity, codec, or load check failed.
    Save(String),
    /// Desktop pack/install/repair failed.
    Package(String),
    /// Cloud-save conflict or corruption recovery failed.
    Cloud(String),
    /// Achievement declaration or sink failure.
    Achievement(String),
    /// Save/epoch/DLC migration could not remap a packed id.
    Migrate(String),
    /// Storefront install/update/rollback failed.
    Store(String),
    /// Crash/replay/symbol mapping failed.
    Crash(String),
    /// Claim level, overclaim language, or evidence binding failed.
    Claim(String),
    /// Console certification suite or device farm failed.
    Cert(String),
    /// Filesystem I/O failed.
    Io(String),
}

impl ReleaseError {
    pub(crate) fn scan(msg: impl Into<String>) -> Self {
        Self::Scan(msg.into())
    }

    pub(crate) fn approval(msg: impl Into<String>) -> Self {
        Self::Approval(msg.into())
    }

    pub(crate) fn signature(msg: impl Into<String>) -> Self {
        Self::Signature(msg.into())
    }

    pub(crate) fn cloud(msg: impl Into<String>) -> Self {
        Self::Cloud(msg.into())
    }

    pub(crate) fn migrate(msg: impl Into<String>) -> Self {
        Self::Migrate(msg.into())
    }

    pub(crate) fn store(msg: impl Into<String>) -> Self {
        Self::Store(msg.into())
    }

    pub(crate) fn crash(msg: impl Into<String>) -> Self {
        Self::Crash(msg.into())
    }

    pub(crate) fn claim(msg: impl Into<String>) -> Self {
        Self::Claim(msg.into())
    }

    pub(crate) fn cert(msg: impl Into<String>) -> Self {
        Self::Cert(msg.into())
    }
}

impl fmt::Display for ReleaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scan(s) => write!(f, "Scan({s})"),
            Self::Approval(s) => write!(f, "Approval({s})"),
            Self::Signature(s) => write!(f, "Signature({s})"),
            Self::Save(s) => write!(f, "Save({s})"),
            Self::Package(s) => write!(f, "Package({s})"),
            Self::Cloud(s) => write!(f, "Cloud({s})"),
            Self::Achievement(s) => write!(f, "Achievement({s})"),
            Self::Migrate(s) => write!(f, "Migrate({s})"),
            Self::Store(s) => write!(f, "Store({s})"),
            Self::Crash(s) => write!(f, "Crash({s})"),
            Self::Claim(s) => write!(f, "Claim({s})"),
            Self::Cert(s) => write!(f, "Cert({s})"),
            Self::Io(s) => write!(f, "Io({s})"),
        }
    }
}

impl core::error::Error for ReleaseError {}

impl From<CompileError> for ReleaseError {
    fn from(e: CompileError) -> Self {
        Self::Package(e.to_string())
    }
}

impl From<SaveError> for ReleaseError {
    fn from(e: SaveError) -> Self {
        Self::Save(e.to_string())
    }
}

impl From<SnapError> for ReleaseError {
    fn from(e: SnapError) -> Self {
        Self::Save(e.to_string())
    }
}
