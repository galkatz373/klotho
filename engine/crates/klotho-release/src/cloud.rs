//! Cloud saves with `(canon_hash, epoch, prefix)` identity.
//!
//! Resolution selects a validated whole save. It never merges Projection
//! columns. Corruption falls back to the previous backup slot.

use klotho_core::{Epoch, Hash};
use klotho_ir::Name;
use klotho_save::{SaveBlob, check_load, decode, encode};
use serde::{Deserialize, Serialize};

use crate::ReleaseError;

/// Identity a cloud slot must present before it is eligible.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveIdentity {
    /// Cooked Canon digest.
    pub canon_hash: Hash,
    /// Hull / cook epoch.
    pub epoch: Epoch,
    /// Terminal Trace prefix of the checkpoint.
    pub prefix: Hash,
}

impl SaveIdentity {
    /// Identity of a validated blob.
    #[must_use]
    pub fn of(blob: &SaveBlob) -> Self {
        Self {
            canon_hash: blob.canon_hash,
            epoch: blob.epoch,
            prefix: blob.prefix,
        }
    }
}

/// Which whole save to keep when two devices disagree.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum ConflictChoice {
    /// Keep the local device save.
    Local,
    /// Keep the remote cloud save.
    Remote,
}

/// Outcome of comparing two encoded saves.
#[derive(Clone, Debug)]
pub enum CloudResolution {
    /// Byte-identical validated saves.
    Identical(SaveBlob),
    /// Caller selected one whole save.
    Selected(SaveBlob),
}

/// Encoded cloud slot plus the device that wrote it.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct CloudSlot {
    /// Device that produced the bytes.
    pub device: Name,
    /// Encoded [`SaveBlob`].
    pub bytes: Vec<u8>,
}

/// Live slot plus one backup. Quota is exactly those two records.
#[derive(Clone, Eq, PartialEq, Debug, Default)]
pub struct CloudStore {
    live: Option<CloudSlot>,
    backup: Option<CloudSlot>,
}

impl CloudStore {
    /// Empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Live slot, if any.
    #[must_use]
    pub fn live(&self) -> Option<&CloudSlot> {
        self.live.as_ref()
    }

    /// Previous live slot retained for corruption recovery.
    #[must_use]
    pub fn backup(&self) -> Option<&CloudSlot> {
        self.backup.as_ref()
    }

    /// Encode `blob` into the live slot, shifting the previous live into backup.
    ///
    /// # Errors
    ///
    /// Returns [`ReleaseError::Save`] when the blob cannot be encoded.
    pub fn push(&mut self, device: Name, blob: &SaveBlob) -> Result<(), ReleaseError> {
        let bytes = encode(blob)?;
        if let Some(live) = self.live.take() {
            self.backup = Some(live);
        }
        self.live = Some(CloudSlot { device, bytes });
        Ok(())
    }

    /// Decode the live slot, or the backup if live is corrupt.
    ///
    /// # Errors
    ///
    /// Returns [`ReleaseError::Cloud`] when both slots are missing or corrupt.
    pub fn recover(&self) -> Result<SaveBlob, ReleaseError> {
        if let Some(live) = &self.live {
            if let Ok(blob) = decode(&live.bytes) {
                check_load(&blob, blob.prefix, blob.canon_hash)?;
                return Ok(blob);
            }
        }
        if let Some(backup) = &self.backup {
            if let Ok(blob) = decode(&backup.bytes) {
                check_load(&blob, blob.prefix, blob.canon_hash)?;
                return Ok(blob);
            }
        }
        Err(ReleaseError::cloud("no valid live or backup save"))
    }
}

/// Select one validated whole save. There is no Projection-merge path.
///
/// # Errors
///
/// Returns [`ReleaseError::Cloud`] when the chosen save fails identity checks.
pub fn resolve_cloud(
    local: &SaveBlob,
    remote: &SaveBlob,
    choice: Option<ConflictChoice>,
) -> Result<CloudResolution, ReleaseError> {
    check_load(local, local.prefix, local.canon_hash)?;
    check_load(remote, remote.prefix, remote.canon_hash)?;
    if encode(local)? == encode(remote)? {
        return Ok(CloudResolution::Identical(local.clone()));
    }
    let Some(choice) = choice else {
        return Err(ReleaseError::cloud(format!(
            "conflict {} vs {} requires a whole-save choice",
            SaveIdentity::of(local).prefix,
            SaveIdentity::of(remote).prefix
        )));
    };
    let selected = match choice {
        ConflictChoice::Local => local,
        ConflictChoice::Remote => remote,
    };
    check_load(selected, selected.prefix, selected.canon_hash)?;
    Ok(CloudResolution::Selected(selected.clone()))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use klotho_core::{Epoch, Hash, LocusKind, Sigil, Tick};
    use klotho_world::{SnapRow, WorldSnapshot};

    use super::*;

    fn relic(id: u128) -> Sigil {
        Sigil::pack(LocusKind::Relic, 0, id).unwrap()
    }

    fn snap(prefix: Hash, qty: i32) -> Arc<WorldSnapshot> {
        let mut row = SnapRow::new(relic(1), LocusKind::Relic);
        row.qty = vec![(klotho_core::ResourceId(0), qty)];
        Arc::new(
            WorldSnapshot::from_snap_rows(
                Epoch::ZERO,
                Tick(1),
                Hash::ZERO,
                prefix,
                None,
                vec![row],
            )
            .unwrap(),
        )
    }

    fn blob(prefix: u8, qty: i32) -> SaveBlob {
        let prefix = Hash::from_bytes([prefix; 32]);
        let snap = snap(prefix, qty);
        klotho_save::pause_save(&snap).unwrap()
    }

    #[test]
    fn identical_saves_do_not_require_a_choice() {
        let a = blob(1, 4);
        let b = blob(1, 4);
        match resolve_cloud(&a, &b, None).unwrap() {
            CloudResolution::Identical(got) => assert_eq!(got.prefix, a.prefix),
            CloudResolution::Selected(_) => panic!("identical saves must not select"),
        }
    }

    #[test]
    fn conflict_selects_a_whole_save_and_never_merges_qty() {
        let local = blob(1, 4);
        let remote = blob(2, 9);
        assert!(resolve_cloud(&local, &remote, None).is_err());
        let CloudResolution::Selected(got) =
            resolve_cloud(&local, &remote, Some(ConflictChoice::Remote)).unwrap()
        else {
            panic!("expected a selection");
        };
        assert_eq!(got.prefix, remote.prefix);
        assert_eq!(got.snap.view().qty(relic(1), klotho_core::ResourceId(0)), 9);
        assert_eq!(
            local.snap.view().qty(relic(1), klotho_core::ResourceId(0)),
            4
        );
    }

    #[test]
    fn corrupt_live_recovers_backup() {
        let mut store = CloudStore::new();
        store.push(Name::from("a"), &blob(1, 1)).unwrap();
        store.push(Name::from("b"), &blob(2, 2)).unwrap();
        store.live.as_mut().unwrap().bytes = b"not-a-save".to_vec();
        let recovered = store.recover().unwrap();
        assert_eq!(recovered.prefix, Hash::from_bytes([1; 32]));
    }
}
