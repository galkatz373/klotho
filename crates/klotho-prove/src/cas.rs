//! In-memory content-addressed store. Caps match the `.warp` loader (HLD §4).

use std::collections::BTreeMap;

use klotho_core::BlobId;

use crate::digest::blob_id_of;
use crate::error::ProveError;

/// Maximum distinct blobs (HLD §4).
pub const MAX_BLOBS: usize = 16_384;
/// Maximum size of a single blob in bytes (HLD §4).
pub const MAX_BLOB_BYTES: usize = 32 * 1024 * 1024;

/// In-memory CAS keyed by [`BlobId`]. Iteration is ordered (K25).
#[derive(Clone, Debug, Default)]
pub struct Cas {
    blobs: BTreeMap<BlobId, Vec<u8>>,
}

impl Cas {
    /// Empty store.
    #[must_use]
    pub fn new() -> Self {
        Self {
            blobs: BTreeMap::new(),
        }
    }

    /// Insert `bytes`, returning their content id. Idempotent for the same
    /// bytes. Rejects oversize blobs and a full store (new ids only).
    pub fn put(&mut self, bytes: &[u8]) -> Result<BlobId, ProveError> {
        check_blob_len(bytes.len())?;
        let id = blob_id_of(bytes);
        if self.blobs.contains_key(&id) {
            return Ok(id);
        }
        if self.blobs.len() >= MAX_BLOBS {
            return Err(ProveError::CasFull);
        }
        self.blobs.insert(id, bytes.to_vec());
        Ok(id)
    }

    /// Borrow the bytes for `id`.
    #[must_use]
    pub fn get(&self, id: BlobId) -> Option<&[u8]> {
        self.blobs.get(&id).map(Vec::as_slice)
    }

    /// `true` if `id` is present.
    #[must_use]
    pub fn contains(&self, id: BlobId) -> bool {
        self.blobs.contains_key(&id)
    }

    /// Number of distinct blobs.
    #[must_use]
    pub fn len(&self) -> usize {
        self.blobs.len()
    }

    /// `true` if no blobs are stored.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.blobs.is_empty()
    }

    /// Ordered `(id, bytes)` pairs. BTree, not HashMap (K25).
    pub fn iter(&self) -> impl Iterator<Item = (BlobId, &[u8])> {
        self.blobs.iter().map(|(id, bytes)| (*id, bytes.as_slice()))
    }
}

fn check_blob_len(len: usize) -> Result<(), ProveError> {
    if len > MAX_BLOB_BYTES {
        Err(ProveError::BlobTooLarge { size: len })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_get_round_trip() {
        let mut cas = Cas::new();
        let id = cas.put(b"hull-bytes").unwrap();
        assert_eq!(cas.get(id), Some(&b"hull-bytes"[..]));
        assert_eq!(cas.put(b"hull-bytes").unwrap(), id);
        assert_eq!(cas.len(), 1);
    }

    #[test]
    fn different_bytes_different_ids() {
        let mut cas = Cas::new();
        let a = cas.put(b"a").unwrap();
        let b = cas.put(b"b").unwrap();
        assert_ne!(a, b);
        assert_eq!(cas.len(), 2);
    }

    #[test]
    fn oversize_blob_is_rejected() {
        assert_eq!(MAX_BLOB_BYTES, 32 * 1024 * 1024);
        assert!(check_blob_len(MAX_BLOB_BYTES).is_ok());
        let n = MAX_BLOB_BYTES + 1;
        assert_eq!(check_blob_len(n), Err(ProveError::BlobTooLarge { size: n }));
    }

    #[test]
    fn cas_full_rejects_new_ids() {
        let mut cas = Cas::new();
        for i in 0..MAX_BLOBS {
            cas.put(&(i as u32).to_le_bytes()).unwrap();
        }
        assert_eq!(cas.len(), MAX_BLOBS);
        assert_eq!(
            cas.put(&(MAX_BLOBS as u32).to_le_bytes()),
            Err(ProveError::CasFull)
        );
        // Idempotent put of an existing blob still succeeds.
        assert!(cas.put(&0u32.to_le_bytes()).is_ok());
    }
}
