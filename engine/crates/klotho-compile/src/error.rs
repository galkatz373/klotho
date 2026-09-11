//! Cook-time failures. None of these are `KernelFault` or `RejectReason`.

use core::fmt;

use klotho_canon::CookError;
use klotho_ir::{
    Diagnostic, FailureClass, diagnose_cap, diagnose_hash_drift, diagnose_named, diagnose_package,
    prove_to_diagnostic,
};
use klotho_prove::ProveError;

/// Why an IntentDoc failed to compile against the kitbash.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum CompileError {
    /// Style or bind named a tag the closed library does not have.
    MissingTag(String),
    /// On-disk kitbash bytes do not match the lockfile (supply-chain).
    LockMismatch {
        /// File name under `data/kitbash`.
        file: String,
        /// Lockfile digest.
        expected: String,
        /// blake3 of the bytes on disk.
        actual: String,
    },
    /// Lockfile names a file that is not on disk.
    MissingLockFile(String),
    /// Kitbash catalog failed to parse.
    Catalog(String),
    /// `klotho-canon` cook failed.
    Canon(String),
    /// CAS / provenance failed.
    Prove(String),
    /// Clustered-mesh / grain / hull header is invalid.
    Header(String),
    /// A millimetre extent does not fit in quantized `i16` verts.
    QuantizeOverflow,
    /// Filesystem read failed.
    Io(String),
    /// `.warp` container is truncated, oversize, or has a bad magic/version.
    Warp(String),
    /// DCC ingest failed (duplicate tag, malformed cooked blob from import).
    Gltf(String),
    /// The current epoch is `u64::MAX` and cannot advance.
    EpochOverflow,
    /// Modular Intent failed to flatten (cycle, hash drift, missing module).
    Flatten(String),
    /// Authoring-only artifact in the ship package graph (K70).
    PackageAllowlist(String),
}

impl CompileError {
    pub(crate) fn canon(e: CookError) -> Self {
        Self::Canon(e.to_string())
    }

    pub(crate) fn prove(e: ProveError) -> Self {
        Self::Prove(e.to_string())
    }

    /// Shared diagnostic envelope. [`Display`] of `self` is the message.
    #[must_use]
    pub fn to_diagnostic(&self) -> Diagnostic {
        let message = self.to_string();
        match self {
            Self::PackageAllowlist(path) => diagnose_package(path, "allowlist", message),
            Self::Warp(s) => diagnose_package("warp", s, message),
            Self::MissingLockFile(p) => diagnose_package(p, "missing-lock", message),
            Self::MissingTag(t) => diagnose_named(
                "COMPILE.MissingTag",
                FailureClass::Schema,
                "tag",
                t,
                message,
            ),
            Self::LockMismatch {
                file,
                expected,
                actual,
            } => diagnose_hash_drift(file, expected, actual, message),
            Self::Catalog(s) => diagnose_named(
                "COMPILE.Catalog",
                FailureClass::Schema,
                "catalog",
                s,
                message,
            ),
            Self::Canon(s) => cook_message_to_diagnostic(s, message),
            Self::Prove(s) => prove_message_to_diagnostic(s, message),
            Self::Header(s) if s.contains("cap_steps") => diagnose_cap("rite", 65, 64, message),
            Self::Header(s) => {
                diagnose_named("COMPILE.Header", FailureClass::Schema, "header", s, message)
            }
            Self::QuantizeOverflow => diagnose_named(
                "COMPILE.QuantizeOverflow",
                FailureClass::Schema,
                "mesh",
                "quantize",
                message,
            ),
            Self::Io(s) => diagnose_named("COMPILE.Io", FailureClass::Schema, "io", s, message),
            Self::Gltf(s) => {
                diagnose_named("COMPILE.Gltf", FailureClass::Schema, "gltf", s, message)
            }
            Self::EpochOverflow => diagnose_named(
                "COMPILE.EpochOverflow",
                FailureClass::Schema,
                "epoch",
                "overflow",
                message,
            ),
            Self::Flatten(s) if s.contains("HashDrift") => {
                diagnose_hash_drift("flatten", "", "", message)
            }
            Self::Flatten(s) => diagnose_named(
                "COMPILE.Flatten",
                FailureClass::Schema,
                "module",
                s,
                message,
            ),
        }
    }
}

fn cook_message_to_diagnostic(native: &str, message: String) -> Diagnostic {
    if native.starts_with("Contradiction") || native == "LockableNeedsKeyOrRite" {
        let laws: Vec<&str> = native
            .trim_start_matches("Contradiction(")
            .trim_end_matches(')')
            .split('|')
            .collect();
        klotho_ir::diagnose_contradiction(&laws, message)
    } else if native.starts_with("MissingTarget")
        || native.starts_with("Unreachable")
        || native.starts_with("FallOff")
        || native.starts_with("DuplicatePc")
        || native.starts_with("MissingEntry")
        || native == "MixedLabeling"
        || native == "Cycle"
    {
        klotho_ir::diagnose_cfg("rite", 0, 0, message)
    } else if native == "PredTooLarge" || native == "TableFull" {
        diagnose_cap("pred", 65, 64, message)
    } else {
        diagnose_named(
            "CANON.InvalidDoc",
            FailureClass::Schema,
            "canon",
            native,
            message,
        )
    }
}

fn prove_message_to_diagnostic(native: &str, message: String) -> Diagnostic {
    let mut d = match native {
        "UnknownLicense" => prove_to_diagnostic(&ProveError::UnknownLicense),
        "InvalidLicense" => prove_to_diagnostic(&ProveError::InvalidLicense),
        "CasFull" => prove_to_diagnostic(&ProveError::CasFull),
        _ => prove_to_diagnostic(&ProveError::UnknownLicense),
    };
    d.message = message;
    d
}

/// Reject authoring-only artifacts from the default ship package (K70).
/// K83 firewall: engine source must not name studio crates, so the studio
/// tree is excluded wholesale instead of matching any crate name.
pub fn check_ship_allowlist(path: &str) -> Result<(), CompileError> {
    let normalized = path.replace('\\', "/");
    let banned = normalized.starts_with("models/")
        || normalized.contains("/models/")
        || normalized.starts_with("studio/")
        || normalized.contains("/studio/")
        || normalized.contains("transcript")
        || normalized.ends_with(".gguf");
    if banned {
        Err(CompileError::PackageAllowlist(normalized))
    } else {
        Ok(())
    }
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingTag(t) => write!(f, "MissingTag({t})"),
            Self::LockMismatch {
                file,
                expected,
                actual,
            } => write!(f, "LockMismatch({file}: expected {expected}, got {actual})"),
            Self::MissingLockFile(p) => write!(f, "MissingLockFile({p})"),
            Self::Catalog(s) => write!(f, "Catalog({s})"),
            Self::Canon(s) => write!(f, "Canon({s})"),
            Self::Prove(s) => write!(f, "Prove({s})"),
            Self::Header(s) => write!(f, "Header({s})"),
            Self::QuantizeOverflow => write!(f, "QuantizeOverflow"),
            Self::Io(s) => write!(f, "Io({s})"),
            Self::Warp(s) => write!(f, "Warp({s})"),
            Self::Gltf(s) => write!(f, "Gltf({s})"),
            Self::EpochOverflow => write!(f, "EpochOverflow"),
            Self::Flatten(s) => write!(f, "Flatten({s})"),
            Self::PackageAllowlist(p) => write!(f, "PackageAllowlist({p})"),
        }
    }
}

impl core::error::Error for CompileError {}
