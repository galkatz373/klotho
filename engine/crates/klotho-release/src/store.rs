//! Storefront install, repair, uninstall, update, DLC overlay, and rollback.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use klotho_compile::{
    DesktopPackage, InstallRecord, check_ship_allowlist, install_package, repair_package,
    uninstall_package,
};
use klotho_core::Epoch;
use klotho_ir::Name;
use serde::{Deserialize, Serialize};

use crate::ReleaseError;

/// P0 storefront identity. Entitlement is optional offline.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoreIdentity {
    /// Storefront account id, empty when playing offline.
    pub account: String,
    /// Entitlement token, empty when the SKU is owned locally.
    pub entitlement: String,
}

impl StoreIdentity {
    /// Offline play: no account, no entitlement check.
    #[must_use]
    pub fn offline() -> Self {
        Self {
            account: String::new(),
            entitlement: String::new(),
        }
    }

    /// True when no storefront session is required.
    #[must_use]
    pub fn is_offline(&self) -> bool {
        self.account.is_empty() && self.entitlement.is_empty()
    }
}

/// Installed candidate plus the previous package used for rollback.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct InstalledRelease {
    /// Install root.
    pub dest: PathBuf,
    /// Current package.
    pub current: DesktopPackage,
    /// Previous package, if an update has been applied.
    pub previous: Option<DesktopPackage>,
    /// Files written by the last successful install.
    pub record: InstallRecord,
}

/// DLC overlay. Files are allowlisted the same way as the base package.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DlcPack {
    /// Stable DLC id.
    pub id: Name,
    /// Epoch the overlay may apply to.
    pub required_epoch: Epoch,
    /// Relative path → bytes.
    pub files: BTreeMap<String, Vec<u8>>,
}

/// In-memory storefront adapter over the desktop installer lane.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct DesktopStorefront {
    identity: StoreIdentity,
}

impl DesktopStorefront {
    /// Offline storefront.
    #[must_use]
    pub fn offline() -> Self {
        Self {
            identity: StoreIdentity::offline(),
        }
    }

    /// Current identity.
    #[must_use]
    pub fn identity(&self) -> &StoreIdentity {
        &self.identity
    }

    /// Install `package` into `dest`. Offline identity is always accepted.
    ///
    /// # Errors
    ///
    /// Returns [`ReleaseError::Store`] when the installer lane fails.
    pub fn install(
        &self,
        package: &DesktopPackage,
        dest: &Path,
    ) -> Result<InstalledRelease, ReleaseError> {
        let _ = &self.identity;
        let record =
            install_package(package, dest).map_err(|e| ReleaseError::store(e.to_string()))?;
        Ok(InstalledRelease {
            dest: dest.to_path_buf(),
            current: package.clone(),
            previous: None,
            record,
        })
    }

    /// Restore drifted files from the current package.
    ///
    /// # Errors
    ///
    /// Returns [`ReleaseError::Store`] when repair fails.
    pub fn repair(&self, installed: &mut InstalledRelease) -> Result<(), ReleaseError> {
        installed.record = repair_package(&installed.current, &installed.dest)
            .map_err(|e| ReleaseError::store(e.to_string()))?;
        Ok(())
    }

    /// Remove installed files listed in the record.
    ///
    /// # Errors
    ///
    /// Returns [`ReleaseError::Store`] when uninstall fails.
    pub fn uninstall(&self, installed: InstalledRelease) -> Result<(), ReleaseError> {
        uninstall_package(&installed.dest, &installed.record)
            .map_err(|e| ReleaseError::store(e.to_string()))
    }

    /// Replace the current package, retaining the previous one for rollback.
    ///
    /// On a failed install the previous package is restored.
    ///
    /// # Errors
    ///
    /// Returns [`ReleaseError::Store`] when both the update and the rollback fail.
    pub fn update(
        &self,
        installed: &mut InstalledRelease,
        next: &DesktopPackage,
    ) -> Result<(), ReleaseError> {
        if next.sku != installed.current.sku {
            return Err(ReleaseError::store("update SKU does not match install"));
        }
        let previous = installed.current.clone();
        let previous_record = installed.record.clone();
        if let Err(e) = uninstall_package(&installed.dest, &installed.record) {
            return Err(ReleaseError::store(e.to_string()));
        }
        match install_package(next, &installed.dest) {
            Ok(record) => {
                installed.previous = Some(previous);
                installed.current = next.clone();
                installed.record = record;
                Ok(())
            }
            Err(e) => {
                let restored = install_package(&previous, &installed.dest).map_err(|re| {
                    ReleaseError::store(format!("update failed ({e}); rollback {re}"))
                })?;
                installed.current = previous;
                installed.record = restored;
                installed.previous = None;
                let _ = previous_record;
                Err(ReleaseError::store(format!("update rolled back: {e}")))
            }
        }
    }

    /// Restore the previous package after a failed or rehearsed update.
    ///
    /// # Errors
    ///
    /// Returns [`ReleaseError::Store`] when no previous package exists.
    pub fn rollback(&self, installed: &mut InstalledRelease) -> Result<(), ReleaseError> {
        let Some(previous) = installed.previous.clone() else {
            return Err(ReleaseError::store("no previous package to roll back"));
        };
        uninstall_package(&installed.dest, &installed.record)
            .map_err(|e| ReleaseError::store(e.to_string()))?;
        let record = install_package(&previous, &installed.dest)
            .map_err(|e| ReleaseError::store(e.to_string()))?;
        installed.current = previous;
        installed.record = record;
        installed.previous = None;
        Ok(())
    }

    /// Overlay DLC files onto the installed package when the epoch matches.
    ///
    /// # Errors
    ///
    /// Returns [`ReleaseError::Store`] when the epoch disagrees or a path is banned.
    pub fn apply_dlc(
        &self,
        installed: &mut InstalledRelease,
        dlc: &DlcPack,
        current_epoch: Epoch,
    ) -> Result<(), ReleaseError> {
        if dlc.required_epoch != current_epoch {
            return Err(ReleaseError::store("DLC epoch mismatch"));
        }
        for (path, bytes) in &dlc.files {
            check_ship_allowlist(path).map_err(|e| ReleaseError::store(e.to_string()))?;
            installed.current.files.insert(path.clone(), bytes.clone());
        }
        installed.record = install_package(&installed.current, &installed.dest)
            .map_err(|e| ReleaseError::store(e.to_string()))?;
        Ok(())
    }
}

/// Staged percent rollout of a signed candidate hash.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct StagedRollout {
    /// Inclusive percents, e.g. `1, 10, 100`.
    pub stages: Vec<u8>,
    /// Index of the active stage.
    pub index: usize,
    /// Candidate currently rolling out.
    pub candidate: klotho_core::Hash,
    /// Previous candidate to restore on rollback.
    pub previous: Option<klotho_core::Hash>,
}

impl StagedRollout {
    /// Start at the first stage.
    ///
    /// # Errors
    ///
    /// Returns [`ReleaseError::Store`] when `stages` is empty or unsorted.
    pub fn start(
        stages: Vec<u8>,
        candidate: klotho_core::Hash,
        previous: Option<klotho_core::Hash>,
    ) -> Result<Self, ReleaseError> {
        if stages.is_empty()
            || stages.windows(2).any(|w| w[0] >= w[1])
            || *stages.last().unwrap() != 100
        {
            return Err(ReleaseError::store("rollout stages must increase to 100"));
        }
        Ok(Self {
            stages,
            index: 0,
            candidate,
            previous,
        })
    }

    /// Advance to the next percent stage.
    ///
    /// # Errors
    ///
    /// Returns [`ReleaseError::Store`] when already at 100%.
    pub fn advance(&mut self) -> Result<u8, ReleaseError> {
        let next = self
            .index
            .checked_add(1)
            .filter(|i| *i < self.stages.len())
            .ok_or_else(|| ReleaseError::store("rollout already complete"))?;
        self.index = next;
        Ok(self.stages[self.index])
    }

    /// Restore the previous candidate hash and freeze at stage 0 of that hash.
    ///
    /// # Errors
    ///
    /// Returns [`ReleaseError::Store`] when no previous candidate exists.
    pub fn rollback(&mut self) -> Result<klotho_core::Hash, ReleaseError> {
        let previous = self
            .previous
            .ok_or_else(|| ReleaseError::store("no previous candidate"))?;
        self.candidate = previous;
        self.previous = None;
        self.index = 0;
        Ok(previous)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use klotho_compile::{DESKTOP_SKUS, DesktopPackage, cook_doc, pack_desktop};
    use klotho_core::Epoch;
    use klotho_prove::hash_bytes;

    use super::*;
    use crate::tests_support::ship_content;

    fn scratch() -> PathBuf {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "klotho-release-store-{}-{nanos}-{serial}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn packed() -> DesktopPackage {
        let cooked = cook_doc(&hearth_slice::hearth_doc()).unwrap();
        pack_desktop(&cooked, &DESKTOP_SKUS[0], &ship_content()).unwrap()
    }

    #[test]
    fn install_repair_update_rollback_offline() {
        let store = DesktopStorefront::offline();
        assert!(store.identity().is_offline());
        let dest = scratch();
        let v1 = packed();
        let mut installed = store.install(&v1, &dest).unwrap();
        assert!(dest.join("game.warp").is_file());
        fs::write(dest.join("game.warp"), b"corrupt").unwrap();
        store.repair(&mut installed).unwrap();
        assert_eq!(
            fs::read(dest.join("game.warp")).unwrap(),
            installed.current.files["game.warp"]
        );

        let mut v2 = v1.clone();
        v2.files
            .insert("NOTICE".into(), b"Updated notice.".to_vec());
        store.update(&mut installed, &v2).unwrap();
        assert_eq!(fs::read(dest.join("NOTICE")).unwrap(), b"Updated notice.");
        store.rollback(&mut installed).unwrap();
        assert_eq!(fs::read(dest.join("NOTICE")).unwrap(), v1.files["NOTICE"]);
        store.uninstall(installed).unwrap();
        let _ = fs::remove_dir_all(&dest);
    }

    #[test]
    fn dlc_epoch_mismatch_fails() {
        let store = DesktopStorefront::offline();
        let dest = scratch();
        let v1 = packed();
        let mut installed = store.install(&v1, &dest).unwrap();
        let mut files = BTreeMap::new();
        files.insert("dlc/extra.ron".into(), b"(ok:true)".to_vec());
        let dlc = DlcPack {
            id: Name::from("coast-pack"),
            required_epoch: Epoch(2),
            files,
        };
        assert!(store.apply_dlc(&mut installed, &dlc, Epoch(0)).is_err());
        store.uninstall(installed).unwrap();
        let _ = fs::remove_dir_all(&dest);
    }

    #[test]
    fn staged_rollout_advances_and_rolls_back() {
        let prev = hash_bytes(b"v1");
        let next = hash_bytes(b"v2");
        let mut roll = StagedRollout::start(vec![1, 10, 100], next, Some(prev)).unwrap();
        assert_eq!(roll.advance().unwrap(), 10);
        assert_eq!(roll.rollback().unwrap(), prev);
        assert_eq!(roll.candidate, prev);
    }
}
