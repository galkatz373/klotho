//! Crash bundles bound to symbols and replay evidence.
//!
//! Symbol files never enter the game package. Upload requires crash-upload
//! consent from the privacy manifest.

use std::collections::BTreeMap;

use klotho_core::Hash;
use klotho_prove::hash_bytes;
use serde::{Deserialize, Serialize};

use crate::ReleaseError;

/// Debug symbols for one package hash. Stored beside, never inside, the ship.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SymbolStore {
    /// Package these offsets describe.
    pub package_hash: Hash,
    /// Instruction offset → symbol name.
    pub entries: BTreeMap<u64, String>,
}

/// Privacy-gated crash capture plus the replay that produced it.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrashBundle {
    /// Package the process was running.
    pub package_hash: Hash,
    /// Desktop SKU id.
    pub sku: String,
    /// Faulting instruction offset.
    pub pc_offset: u64,
    /// Recorded replay bytes (Intent script or Trace dump).
    pub replay: Vec<u8>,
    /// Whether the player consented to upload.
    pub crash_upload_consent: bool,
}

/// Symbolized crash plus the replay hash that must accompany it.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct MappedCrash {
    /// Package the crash belongs to.
    pub package_hash: Hash,
    /// SKU.
    pub sku: String,
    /// Mapped symbol.
    pub symbol: String,
    /// blake3 of the replay bytes.
    pub replay_hash: Hash,
}

impl CrashBundle {
    /// blake3 of the replay payload.
    #[must_use]
    pub fn replay_hash(&self) -> Hash {
        hash_bytes(&self.replay)
    }
}

/// Map `bundle.pc_offset` through `symbols` and bind the replay hash.
///
/// # Errors
///
/// Returns [`ReleaseError::Crash`] when the package hash disagrees, consent is
/// missing, the offset is unknown, or the replay is empty.
pub fn map_crash(bundle: &CrashBundle, symbols: &SymbolStore) -> Result<MappedCrash, ReleaseError> {
    if !bundle.crash_upload_consent {
        return Err(ReleaseError::crash("crash upload consent missing"));
    }
    if bundle.package_hash != symbols.package_hash {
        return Err(ReleaseError::crash(
            "crash package hash does not match symbols",
        ));
    }
    if bundle.replay.is_empty() {
        return Err(ReleaseError::crash("crash replay evidence missing"));
    }
    let symbol = symbols
        .entries
        .get(&bundle.pc_offset)
        .ok_or_else(|| ReleaseError::crash("pc offset has no symbol"))?;
    Ok(MappedCrash {
        package_hash: bundle.package_hash,
        sku: bundle.sku.clone(),
        symbol: symbol.clone(),
        replay_hash: bundle.replay_hash(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symbols(hash: Hash) -> SymbolStore {
        let mut entries = BTreeMap::new();
        entries.insert(0x10, "klotho_runtime::step".into());
        SymbolStore {
            package_hash: hash,
            entries,
        }
    }

    #[test]
    fn crash_maps_to_symbol_and_replay_hash() {
        let hash = hash_bytes(b"pkg");
        let bundle = CrashBundle {
            package_hash: hash,
            sku: "win-d3d12-high".into(),
            pc_offset: 0x10,
            replay: b"intent-script".to_vec(),
            crash_upload_consent: true,
        };
        let mapped = map_crash(&bundle, &symbols(hash)).unwrap();
        assert_eq!(mapped.symbol, "klotho_runtime::step");
        assert_eq!(mapped.replay_hash, hash_bytes(b"intent-script"));
    }

    #[test]
    fn missing_consent_or_replay_fails_closed() {
        let hash = hash_bytes(b"pkg");
        let mut bundle = CrashBundle {
            package_hash: hash,
            sku: "mac-metal-high".into(),
            pc_offset: 0x10,
            replay: b"replay".to_vec(),
            crash_upload_consent: false,
        };
        assert!(map_crash(&bundle, &symbols(hash)).is_err());
        bundle.crash_upload_consent = true;
        bundle.replay.clear();
        assert!(map_crash(&bundle, &symbols(hash)).is_err());
        bundle.replay = b"replay".to_vec();
        bundle.pc_offset = 0x99;
        assert!(map_crash(&bundle, &symbols(hash)).is_err());
    }
}
