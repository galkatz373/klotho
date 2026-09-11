//! blake3 wrappers onto [`klotho_core::Hash`] / [`klotho_core::BlobId`].

use klotho_core::{BlobId, Hash};

/// blake3 of `bytes`. Caller must already have canonical little-endian content.
#[must_use]
pub fn hash_bytes(bytes: &[u8]) -> Hash {
    Hash(*blake3::hash(bytes).as_bytes())
}

/// Content-address of opaque blob bytes. Same digest as [`hash_bytes`],
/// distinct type so hull ids cannot be confused with Canon / Trace hashes.
#[must_use]
pub fn blob_id_of(bytes: &[u8]) -> BlobId {
    BlobId(*blake3::hash(bytes).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_bytes_same_hash() {
        let a = hash_bytes(b"klotho");
        let b = hash_bytes(b"klotho");
        assert_eq!(a, b);
        assert_eq!(blob_id_of(b"klotho").0, a.0);
    }

    #[test]
    fn different_bytes_different_hash() {
        assert_ne!(hash_bytes(b"klotho"), hash_bytes(b"Klotho"));
    }

    #[test]
    fn golden_empty_matches_blake3_test_vector() {
        // Official BLAKE3 empty-input vector. If this changes, every cook hash changes.
        let h = hash_bytes(b"");
        assert_eq!(
            h.to_string(),
            "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
        );
        assert_eq!(h, hash_bytes(&[]));
        assert_ne!(h, Hash::ZERO);
    }

    #[test]
    fn golden_blake3_of_klotho() {
        let h = hash_bytes(b"klotho");
        assert_eq!(
            h.to_string(),
            "a1fe095bcabc8478c57d16184c936c2f88d6886e029ae9dc7f943bac61f2c609"
        );
    }
}
